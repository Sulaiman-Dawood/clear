//! The main user flows: open the action menu, run a direct shortcut,
//! generate, and put the result back.

use serde::Serialize;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::actions::{self, ActionInfo};
use crate::history::{self, HistoryEntry};
use crate::llm;
use crate::popup::{self, Mode};
use crate::{guard, secrets, winsys, AppState};

/// Longest selection we'll send. Larger selections are refused rather than
/// silently cut off.
pub const MAX_CHARS: usize = 20_000;

#[derive(Default, Clone)]
pub struct Session {
    /// Window the text came from (0 when there is none to paste into).
    pub target: isize,
    pub text: String,
    pub label: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPayload {
    pub id: u64,
    pub has_selection: bool,
    pub can_replace: bool,
    /// Whether the selection is in an editable field. When it isn't (a web
    /// page, a PDF), Copy becomes the main button instead of Replace.
    pub editable: bool,
    /// An action to start as soon as the popup opens.
    pub autorun: Option<AutoRun>,
    /// A finished result to show right away (an in-place shortcut that
    /// couldn't safely paste).
    pub ready: Option<ReadyResult>,
    pub preview: String,
    /// Full selection, used by the popup's "Changes" view.
    pub text: String,
    pub words: usize,
    pub chars: usize,
    pub actions: Vec<ActionInfo>,
    pub quick_languages: Vec<String>,
    pub provider_name: String,
    pub model: String,
    /// Set when something must be fixed before generating (no key, too long...).
    pub problem: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyResult {
    pub action: String,
    pub label: String,
    pub kind: actions::Kind,
    pub diff_first: bool,
    pub text: String,
    /// Why it's shown here instead of pasted.
    pub note: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoRun {
    pub action: String,
    pub language: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToastPayload {
    pub id: u64,
    /// "working", "done" or "error".
    pub state: String,
    pub text: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenEvent {
    pub id: u64,
    /// "delta", "done" or "error".
    pub kind: String,
    pub text: String,
}

pub fn llm_target(settings: &crate::settings::Settings) -> llm::Target {
    let id = settings.active_provider.clone();
    let profile = settings.profile(&id);
    let wire = llm::preset(&id).map(|p| p.wire).unwrap_or(llm::Wire::OpenAi);
    llm::Target {
        api_key: secrets::get(&id),
        provider: id,
        wire,
        base_url: profile.base_url,
        model: profile.model,
        effort: settings.anthropic_effort.clone(),
    }
}

/// Why generation can't run yet, if anything is missing.
fn provider_problem(settings: &crate::settings::Settings) -> Option<String> {
    let t = llm_target(settings);
    let preset = llm::preset(&t.provider)?;
    if preset.needs_key && t.api_key.is_none() {
        return Some(format!("Add your {} API key in Settings to start.", preset.name));
    }
    if t.model.trim().is_empty() {
        return Some("Choose a model in Settings > AI provider.".into());
    }
    if t.base_url.trim().is_empty() {
        return Some("Set the server URL in Settings > AI provider.".into());
    }
    None
}

fn preview(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 140 {
        format!("{}…", flat.chars().take(140).collect::<String>())
    } else {
        flat
    }
}

pub fn hide_popup(app: &AppHandle) {
    cancel_generation(app);
    let state = app.state::<AppState>();
    {
        let mut p = guard(&state.popup);
        p.mode = Mode::Hidden;
        p.pending = 0;
        p.presented = false;
    }
    if let Some(win) = popup::window(app) {
        let raw = popup::raw_hwnd(&win);
        if raw != 0 {
            // Hide via Win32 directly so Tao doesn't re-apply window styles.
            winsys::hide_window(raw);
        } else {
            let _ = win.hide();
        }
    }
    let _ = app.emit_to(popup::LABEL, "popup-clear", ());
}

pub fn cancel_generation(app: &AppHandle) {
    let state = app.state::<AppState>();
    let handle = guard(&state.generation).take();
    if let Some(handle) = handle {
        handle.abort();
    }
}

/// If the page doesn't present itself quickly (e.g. still loading),
/// show it anyway at a default size.
fn present_fallback(app: &AppHandle, id: u64, focus: bool) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(400));
        let state = app.state::<AppState>();
        let anchor = {
            let p = guard(&state.popup);
            if p.pending != id || p.presented {
                return;
            }
            p.anchor
        };
        if let Some(win) = popup::window(&app) {
            if focus {
                guard(&state.popup).flipped = popup::place(&win, anchor, 424.0, 260.0);
            } else {
                popup::place_toast(&win, anchor, 300.0, 72.0);
            }
            popup::show(&win, focus);
            let mut p = guard(&state.popup);
            p.presented = true;
            p.mode = if focus { Mode::Menu } else { Mode::Toast };
        }
    });
}

/// Open the action menu. With `capture`, the current selection is copied first.
pub fn open_menu(app: &AppHandle, capture: bool) {
    open_menu_with(app, capture, None);
}

/// Open the popup, optionally starting an action right away (shown as a result).
pub fn open_menu_with(app: &AppHandle, capture: bool, autorun: Option<AutoRun>) {
    let state = app.state::<AppState>();
    if guard(&state.popup).mode == Mode::Menu {
        hide_popup(app);
        return;
    }
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        let started = Instant::now();
        let state = app.state::<AppState>();
        let fg = winsys::foreground_window();
        let target = if fg == 0 || winsys::is_own_window(fg) { 0 } else { fg };
        let anchor = popup::anchor_for(if capture { target } else { 0 });
        let id = begin_popup(&app, anchor);
        let reading = capture && target != 0;
        // Ask whether the field is editable while the selection is copied.
        let editable_check = (target != 0).then(|| winsys::EditableCheck::start(target));
        let mut text = if reading { winsys::capture_selection(target) } else { None };
        let (editable, active_input) = match editable_check {
            Some(c) => c.wait_detailed(),
            None => (false, false),
        };
        // If nothing was selected and an editable input field is active, select
        // all text in that field so any action can operate on it directly.
        if reading && text.is_none() && active_input && !winsys::is_terminal_window(target) {
            text = winsys::capture_all_in_field(target);
        }
        let copied_at = started.elapsed();
        state.busy.store(false, Ordering::SeqCst);
        log_timing("menu", started, copied_at);

        let has_text = text.as_deref().is_some_and(|t| !t.trim().is_empty());
        // An action needs a selection; without one, just open the menu.
        let autorun = autorun.filter(|_| has_text);
        present_menu(&app, id, target, anchor, text.unwrap_or_default(), editable, autorun, None);
    });
}

/// Start a new popup request: returns its id.
fn begin_popup(app: &AppHandle, anchor: popup::Anchor) -> u64 {
    let state = app.state::<AppState>();
    let id = state.next_id();
    let mut p = guard(&state.popup);
    p.anchor = anchor;
    p.pending = id;
    p.presented = false;
    id
}

/// In development builds, print how long opening took (see the dev console).
fn log_timing(what: &str, started: Instant, copied_at: Duration) {
    if cfg!(debug_assertions) {
        eprintln!(
            "[clear] {what}: selection copied in {} ms, ready in {} ms",
            copied_at.as_millis(),
            started.elapsed().as_millis()
        );
    }
}

/// Store the session and ask the popup page to show itself.
#[allow(clippy::too_many_arguments)]
fn present_menu(
    app: &AppHandle,
    id: u64,
    target: isize,
    anchor: popup::Anchor,
    text: String,
    editable: bool,
    autorun: Option<AutoRun>,
    ready: Option<ReadyResult>,
) {
    let state = app.state::<AppState>();
    let settings = guard(&state.settings).clone();
    let chars = text.chars().count();
    let mut problem = provider_problem(&settings);
    if chars > MAX_CHARS {
        problem = Some(format!(
            "That selection is {chars} characters. Clear works on up to {MAX_CHARS} at a time; select a smaller part."
        ));
    }
    let label = ready.as_ref().map(|r| r.label.clone()).unwrap_or_default();
    *guard(&state.session) = Session { target, text: text.clone(), label };
    {
        // Keep this request current (a toast may have used the window meanwhile).
        let mut p = guard(&state.popup);
        if p.pending != id {
            p.pending = id;
            p.presented = false;
        }
        p.anchor = anchor;
    }
    let t = llm_target(&settings);
    let payload = OpenPayload {
        id,
        has_selection: !text.trim().is_empty(),
        can_replace: target != 0,
        editable,
        preview: preview(&text),
        text: text.clone(),
        words: text.split_whitespace().count(),
        chars,
        actions: actions::for_popup(&settings),
        quick_languages: settings.quick_languages.clone(),
        provider_name: llm::preset(&t.provider).map(|p| p.name.to_string()).unwrap_or_default(),
        model: t.model.clone(),
        autorun: if problem.is_some() { None } else { autorun },
        ready,
        problem,
    };
    *guard(&state.last_open) = Some(payload.clone());
    let _ = app.emit_to(popup::LABEL, "popup-open", payload);
    present_fallback(app, id, true);
}

pub fn show_toast(app: &AppHandle, state_name: &str, text: &str) {
    let state = app.state::<AppState>();
    let id = state.next_id();
    {
        let mut p = guard(&state.popup);
        if p.mode == Mode::Menu {
            return;
        }
        p.pending = id;
        p.presented = false;
        p.toast_id = id;
    }
    let _ = app.emit_to(
        popup::LABEL,
        "toast",
        ToastPayload { id, state: state_name.into(), text: text.into() },
    );
    present_fallback(app, id, false);

    // Finished toasts go away on their own; a newer toast cancels this timer.
    let linger = match state_name {
        "done" => Some(Duration::from_millis(1400)),
        "error" => Some(Duration::from_millis(5000)),
        _ => None,
    };
    if let Some(linger) = linger {
        let app = app.clone();
        thread::spawn(move || {
            thread::sleep(linger);
            let state = app.state::<AppState>();
            let still_showing = {
                let p = guard(&state.popup);
                p.toast_id == id && p.mode != Mode::Menu
            };
            if still_showing {
                hide_popup(&app);
            }
        });
    }
}

fn record_history(app: &AppHandle, action: &str, input: &str, output: &str) {
    let state = app.state::<AppState>();
    if !guard(&state.settings).history_enabled {
        return;
    }
    let mut list = guard(&state.history);
    list.insert(
        0,
        HistoryEntry {
            ts: history::now_ms(),
            action: action.to_string(),
            input: input.to_string(),
            output: output.to_string(),
        },
    );
    list.truncate(history::MAX_ENTRIES);
    let _ = history::save(&state.history_path, &list);
}

/// A shortcut that transforms the selection in place without opening the menu.
pub fn run_direct(app: &AppHandle, action: &str) {
    let state = app.state::<AppState>();
    // Pressing an in-place shortcut again while one runs cancels it.
    if state.direct_running.load(Ordering::SeqCst) {
        cancel_direct(app);
        return;
    }
    if guard(&state.popup).mode == Mode::Menu || state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    state.direct_cancel.store(false, Ordering::SeqCst);
    state.direct_running.store(true, Ordering::SeqCst);
    let app = app.clone();
    let action = action.to_string();
    thread::spawn(move || {
        let state = app.state::<AppState>();
        let done = || {
            state.direct_running.store(false, Ordering::SeqCst);
            state.busy.store(false, Ordering::SeqCst);
        };

        let fg = winsys::foreground_window();
        if fg == 0 || winsys::is_own_window(fg) {
            done();
            return;
        }
        let started = Instant::now();
        let anchor = popup::anchor_for(fg);
        guard(&state.popup).anchor = anchor;

        let settings = guard(&state.settings).clone();
        if let Some(problem) = provider_problem(&settings) {
            show_toast(&app, "error", &problem);
            done();
            return;
        }
        // Ask whether the field is editable while the selection is copied.
        let editable_check = winsys::EditableCheck::start(fg);
        let mut captured = winsys::capture_selection(fg);
        let (editable, active_input) = editable_check.wait_detailed();
        // If nothing was selected and an editable input field is active, select
        // all text in that field so the shortcut operates on all of it.
        if captured.is_none() && active_input && !winsys::is_terminal_window(fg) {
            captured = winsys::capture_all_in_field(fg);
        }
        let input_after_copy = winsys::last_input_tick();
        let copied_at = started.elapsed();
        log_timing("shortcut", started, copied_at);
        let Some(text) = captured else {
            show_toast(&app, "error", "Select some text first, then press the shortcut.");
            done();
            return;
        };
        if text.chars().count() > MAX_CHARS {
            show_toast(&app, "error", &format!("Selection is too long. Clear works on up to {MAX_CHARS} characters."));
            done();
            return;
        }
        // Text on a web page or in a PDF can't be pasted over, so show the
        // result in the popup instead of pasting it.
        if !editable {
            done();
            let autorun = AutoRun { action: action.clone(), language: None };
            let id = begin_popup(&app, anchor);
            present_menu(&app, id, fg, anchor, text, false, Some(autorun), None);
            return;
        }
        let resolved = match actions::resolve(&settings, &action, None, None) {
            Ok(r) => r,
            Err(e) => {
                show_toast(&app, "error", &e);
                done();
                return;
            }
        };
        show_toast(&app, "working", &format!("{}…", resolved.label));

        let (system, user) = actions::build_prompt(&resolved, &text);
        let target = llm_target(&settings);
        let http = state.http.clone();
        let cancel = &state.direct_cancel;
        let result = tauri::async_runtime::block_on(async {
            tokio::select! {
                r = llm::stream(&http, &target, &system, &user, |_| {}) => Some(r),
                _ = async {
                    while !cancel.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                } => None,
            }
        });
        let Some(result) = result else {
            // Cancelled: just put the toast away.
            hide_popup(&app);
            done();
            return;
        };
        match result {
            Ok(out) => {
                let cleaned = actions::clean_output(&text, &out);
                // Only paste if the user is still where they were, with the
                // same text selected. If they never touched the keyboard/mouse
                // after the copy (or it's a terminal that clears selection on
                // copy), skip the second Ctrl+C so flaky re-copying never
                // bounces an in-place edit into the popup.
                let still_there = if winsys::foreground_window() != fg {
                    false
                } else if winsys::last_input_tick() == input_after_copy || winsys::is_terminal_window(fg) {
                    true
                } else {
                    winsys::verify_selection(fg).is_some_and(|now| same_text(&now, &text))
                };
                if !still_there {
                    let note = "You moved on before it finished, so it wasn't pasted. Replace it or copy it from here.";
                    show_ready_result(&app, fg, &action, &resolved, text, cleaned, note);
                    done();
                    return;
                }
                let final_text = actions::wrap_like(&text, &cleaned);
                match winsys::paste_into(fg, &final_text) {
                    Ok(()) => {
                        record_history(&app, &resolved.label, &text, &cleaned);
                        show_toast(&app, "done", "Done");
                    }
                    Err(e) => {
                        let _ = winsys::set_clipboard_text(&cleaned);
                        show_toast(&app, "error", &format!("{e} The result was copied to your clipboard."));
                    }
                }
            }
            Err(e) => show_toast(&app, "error", &e),
        }
        done();
    });
}

/// Same text, ignoring line-ending style and surrounding whitespace.
fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n").trim() == b.replace("\r\n", "\n").trim()
}

pub fn cancel_direct(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.direct_running.load(Ordering::SeqCst) {
        state.direct_cancel.store(true, Ordering::SeqCst);
    }
}

/// Show a finished in-place result in the popup, next to the user's caret.
fn show_ready_result(
    app: &AppHandle,
    target: isize,
    action: &str,
    resolved: &actions::Resolved,
    input: String,
    output: String,
    note: &str,
) {
    hide_popup(app);
    let anchor = popup::anchor_for(winsys::foreground_window());
    let id = begin_popup(app, anchor);
    let ready = ReadyResult {
        action: action.to_string(),
        label: resolved.label.clone(),
        kind: resolved.kind,
        diff_first: resolved.diff_first,
        text: output,
        note: note.to_string(),
    };
    present_menu(app, id, target, anchor, input, true, None, Some(ready));
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenStart {
    pub id: u64,
    pub label: String,
    pub kind: actions::Kind,
    pub diff_first: bool,
}

pub fn start_generation(
    app: &AppHandle,
    action: &str,
    instruction: Option<&str>,
    language: Option<&str>,
) -> Result<GenStart, String> {
    let state = app.state::<AppState>();
    let settings = guard(&state.settings).clone();
    if let Some(problem) = provider_problem(&settings) {
        return Err(problem);
    }
    let resolved = actions::resolve(&settings, action, instruction, language)?;
    let text = {
        let mut s = guard(&state.session);
        if s.text.trim().is_empty() && action != "custom" {
            return Err("Select some text first, or type a request.".into());
        }
        if s.text.chars().count() > MAX_CHARS {
            return Err(format!("Selection is too long. Clear works on up to {MAX_CHARS} characters."));
        }
        s.label = resolved.label.clone();
        s.text.clone()
    };
    let (system, user) = actions::build_prompt(&resolved, &text);
    let target = llm_target(&settings);
    let http = state.http.clone();
    let id = state.next_id();

    cancel_generation(app);
    let app2 = app.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let emit = |kind: &str, text: String| {
            let _ = app2.emit_to(popup::LABEL, "gen", GenEvent { id, kind: kind.into(), text });
        };
        let result = llm::stream(&http, &target, &system, &user, |delta| emit("delta", delta.to_string())).await;
        match result {
            Ok(full) => emit("done", actions::clean_output(&text, &full)),
            Err(e) => emit("error", e),
        }
    });
    *guard(&state.generation) = Some(handle);

    Ok(GenStart { id, label: resolved.label, kind: resolved.kind, diff_first: resolved.diff_first })
}

/// Use a result: paste it over the selection ("replace") or copy it ("copy").
pub fn apply_result(app: &AppHandle, text: &str, mode: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let session = guard(&state.session).clone();
    let label = if session.label.is_empty() { "Custom".to_string() } else { session.label.clone() };
    match mode {
        "copy" => {
            winsys::set_clipboard_text(text)?;
            record_history(app, &label, &session.text, text);
            hide_popup(app);
            Ok(())
        }
        "replace" => {
            if session.target == 0 {
                return Err("There's no window to paste into. Use Copy instead.".into());
            }
            let final_text = if session.text.trim().is_empty() {
                text.to_string()
            } else {
                actions::wrap_like(&session.text, text)
            };
            hide_popup(app);
            record_history(app, &label, &session.text, text);
            let app = app.clone();
            let text = text.to_string();
            thread::spawn(move || {
                // Let the popup finish hiding before switching windows.
                thread::sleep(Duration::from_millis(60));
                if let Err(e) = winsys::paste_into(session.target, &final_text) {
                    let _ = winsys::set_clipboard_text(&text);
                    show_toast(&app, "error", &format!("{e} The result was copied to your clipboard."));
                }
            });
            Ok(())
        }
        _ => Err(format!("Unknown mode: {mode}")),
    }
}
