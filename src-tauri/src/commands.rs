//! Commands the two web pages (popup and settings) can call.

use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::actions::{self, ActionInfo};
use crate::flows::{self, GenStart, OpenPayload};
use crate::history::HistoryEntry;
use crate::hotkeys;
use crate::llm::{self, ProviderPreset, PROVIDER_PRESETS};
use crate::popup::{self, Mode};
use crate::settings::{default_hotkeys, Settings};
use crate::{guard, secrets, winsys, AppState};

// ---------- popup ----------

#[tauri::command]
pub fn popup_sync(state: State<'_, AppState>) -> Option<OpenPayload> {
    let pending = guard(&state.popup).pending;
    let last = guard(&state.last_open).clone();
    last.filter(|p| p.id == pending)
}

/// Called by the popup page once it has rendered, with its card size.
/// `placement` is "menu" (next to the caret) or "toast" (bottom center).
/// A menu can be presented twice for one request: first as a loading card
/// without focus, then for real with focus, without moving in between.
#[tauri::command]
pub fn popup_present(
    app: AppHandle,
    state: State<'_, AppState>,
    id: u64,
    width: f64,
    height: f64,
    focus: bool,
    placement: String,
) {
    let toast = placement == "toast";
    let (anchor, already_shown, keep_bottom) = {
        let mut p = guard(&state.popup);
        if p.pending != id {
            return;
        }
        let already_shown = p.presented && p.mode == Mode::Menu && !toast;
        if !already_shown {
            p.user_moved = false;
        }
        p.presented = true;
        p.mode = if toast { Mode::Toast } else { Mode::Menu };
        (p.anchor, already_shown, p.flipped && !p.user_moved)
    };
    if let Some(win) = popup::window(&app) {
        if toast {
            popup::place_toast(&win, anchor, width, height);
        } else if already_shown {
            popup::resize_anchored(&win, width, height, keep_bottom);
        } else {
            let flipped = popup::place(&win, anchor, width, height);
            guard(&state.popup).flipped = flipped;
        }
        popup::show(&win, focus);
    }
}

/// Called when the card changes size while visible.
#[tauri::command]
pub fn popup_resize(app: AppHandle, state: State<'_, AppState>, width: f64, height: f64) {
    let (mode, anchor, keep_bottom) = {
        let p = guard(&state.popup);
        (p.mode, p.anchor, p.flipped && !p.user_moved)
    };
    if let Some(win) = popup::window(&app) {
        match mode {
            Mode::Hidden => {}
            // Never re-anchor while open: just grow or shrink from one edge.
            Mode::Menu => popup::resize_anchored(&win, width, height, keep_bottom),
            Mode::Toast => popup::place_toast(&win, anchor, width, height),
        }
    }
}

/// The user started dragging the output's resize grip. Give the window room
/// to grow downward up front, so the drag itself only changes the page.
/// Returns the height (CSS px) the window could grow to without moving.
#[tauri::command]
pub fn popup_begin_resize(app: AppHandle, state: State<'_, AppState>, width: f64, height: f64) -> f64 {
    guard(&state.popup).user_moved = true;
    match popup::window(&app) {
        Some(win) => popup::grow_down(&win, width, height),
        None => height,
    }
}

/// The user started dragging the popup by its header.
#[tauri::command]
pub fn popup_drag(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    guard(&state.popup).user_moved = true;
    let win = popup::window(&app).ok_or("No popup window.")?;
    win.start_dragging().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn popup_hide(app: AppHandle) {
    flows::hide_popup(&app);
}

#[tauri::command]
pub fn start_generation(
    app: AppHandle,
    action: String,
    instruction: Option<String>,
    language: Option<String>,
) -> Result<GenStart, String> {
    flows::start_generation(&app, &action, instruction.as_deref(), language.as_deref())
}

#[tauri::command]
pub fn cancel_generation(app: AppHandle) {
    flows::cancel_generation(&app);
}

/// Cancel a running in-place shortcut (clicking its "working" toast).
#[tauri::command]
pub fn cancel_direct(app: AppHandle) {
    flows::cancel_direct(&app);
}

#[tauri::command]
pub fn apply_result(app: AppHandle, text: String, mode: String) -> Result<(), String> {
    flows::apply_result(&app, &text, &mode)
}

// ---------- settings ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub settings: Settings,
    pub key_status: BTreeMap<String, bool>,
    pub hotkey_errors: BTreeMap<String, String>,
    pub providers: Vec<ProviderPreset>,
    pub catalog: Vec<ActionInfo>,
    pub version: String,
    pub config_dir: String,
    pub keychain_service: String,
}

fn key_status() -> BTreeMap<String, bool> {
    PROVIDER_PRESETS
        .iter()
        .map(|p| (p.id.to_string(), secrets::has(p.id)))
        .collect()
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        settings: guard(&state.settings).clone(),
        key_status: key_status(),
        hotkey_errors: guard(&state.hotkey_errors).clone(),
        providers: PROVIDER_PRESETS.to_vec(),
        catalog: actions::catalog(),
        version: app.package_info().version.to_string(),
        config_dir: state.config_dir.display().to_string(),
        keychain_service: secrets::service_name().to_string(),
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let mut next = settings;
    let previous = guard(&state.settings).clone();
    // Shortcuts change only through set_hotkey, which validates them.
    next.hotkeys = previous.hotkeys.clone();
    // Also drops shortcuts of presets that were just deleted.
    next.normalize();

    if next.launch_at_login != previous.launch_at_login {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = app.autolaunch();
        let result = if next.launch_at_login { autolaunch.enable() } else { autolaunch.disable() };
        result.map_err(|e| format!("Couldn't change launch at login: {e}"))?;
    }
    next.save(&state.settings_path)?;
    *guard(&state.settings) = next.clone();
    if next.theme != previous.theme {
        crate::apply_theme(&app, &next.theme);
    }
    if next.hotkeys != previous.hotkeys {
        hotkeys::register_all(&app);
    }
    Ok(next)
}

#[tauri::command]
pub fn get_theme(state: State<'_, AppState>) -> String {
    guard(&state.settings).theme.clone()
}

#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> Result<BTreeMap<String, bool>, String> {
    if llm::preset(&provider).is_none() {
        return Err("Unknown provider.".into());
    }
    secrets::set(&provider, &key)?;
    Ok(key_status())
}

#[tauri::command]
pub fn clear_api_key(provider: String) -> Result<BTreeMap<String, bool>, String> {
    secrets::delete(&provider)?;
    Ok(key_status())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub model: String,
    pub ms: u128,
    pub reply: String,
}

#[tauri::command]
pub async fn test_connection(app: AppHandle) -> Result<TestResult, String> {
    let state = app.state::<AppState>();
    let settings = guard(&state.settings).clone();
    let target = flows::llm_target(&settings);
    let http = state.http.clone();
    let start = Instant::now();
    let reply = llm::stream(
        &http,
        &target,
        "You are a connection test. Reply with exactly the word OK.",
        "Reply with OK.",
        |_| {},
    )
    .await?;
    Ok(TestResult {
        model: target.model,
        ms: start.elapsed().as_millis(),
        reply: reply.trim().chars().take(80).collect(),
    })
}

#[tauri::command]
pub async fn list_models(app: AppHandle, provider: String, base_url: String) -> Result<Vec<String>, String> {
    let preset = llm::preset(&provider).ok_or("Unknown provider.")?;
    let state = app.state::<AppState>();
    let http = state.http.clone();
    let target = llm::Target {
        provider: provider.clone(),
        wire: preset.wire,
        base_url,
        model: String::new(),
        api_key: secrets::get(&provider),
        effort: String::new(),
    };
    llm::list_models(&http, &target).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyUpdate {
    pub settings: Settings,
    pub hotkey_errors: BTreeMap<String, String>,
}

#[tauri::command]
pub fn set_hotkey(app: AppHandle, state: State<'_, AppState>, action: String, accelerator: String) -> Result<HotkeyUpdate, String> {
    if !hotkeys::is_target(&guard(&state.settings), &action) {
        return Err("That action no longer exists.".into());
    }
    let mut accelerator = accelerator.trim().to_string();
    if !accelerator.is_empty() {
        // Validate and store one standard spelling, e.g. "LCtrl+RCtrl".
        accelerator = crate::chord::parse(&accelerator)?.canonical();
        let settings = guard(&state.settings).clone();
        for (other, accel) in &settings.hotkeys {
            let same = crate::chord::parse(accel).map(|c| c.canonical() == accelerator).unwrap_or(false);
            if other != &action && same {
                let name = hotkeys::label(&settings, other);
                return Err(format!("{accelerator} is already used for {name}."));
            }
        }
    }

    let previous = {
        let mut s = guard(&state.settings);
        let previous = s.hotkeys.get(&action).cloned().unwrap_or_default();
        s.hotkeys.insert(action.clone(), accelerator.clone());
        previous
    };
    let errors = hotkeys::register_all(&app);
    if let Some(err) = errors.get(&action) {
        guard(&state.settings).hotkeys.insert(action.clone(), previous);
        hotkeys::register_all(&app);
        return Err(err.clone());
    }
    let settings = guard(&state.settings).clone();
    settings.save(&state.settings_path)?;
    Ok(HotkeyUpdate { settings, hotkey_errors: errors })
}

#[tauri::command]
pub fn reset_hotkeys(app: AppHandle, state: State<'_, AppState>) -> Result<HotkeyUpdate, String> {
    {
        let mut s = guard(&state.settings);
        s.hotkeys = default_hotkeys();
        s.hotkey_modes.clear();
    }
    let errors = hotkeys::register_all(&app);
    let settings = guard(&state.settings).clone();
    settings.save(&state.settings_path)?;
    Ok(HotkeyUpdate { settings, hotkey_errors: errors })
}

/// Pause shortcuts while the user records a new one, so the keys reach the page.
#[tauri::command]
pub fn pause_hotkeys(app: AppHandle, paused: bool) -> BTreeMap<String, String> {
    hotkeys::pause(&app, paused);
    if paused {
        BTreeMap::new()
    } else {
        hotkeys::register_all(&app)
    }
}

#[tauri::command]
pub fn get_history(state: State<'_, AppState>) -> Vec<HistoryEntry> {
    guard(&state.history).clone()
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    guard(&state.history).clear();
    match std::fs::remove_file(&state.history_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn copy_text(text: String) -> Result<(), String> {
    winsys::set_clipboard_text(&text)
}

pub const REPO_URL: &str = "https://github.com/Sulaiman-Dawood/clear";

/// Open the project's GitHub page in the default browser.
#[tauri::command]
pub fn open_repo() -> Result<(), String> {
    #[cfg(windows)]
    let result = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", REPO_URL]).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(REPO_URL).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(REPO_URL).spawn();
    result.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_config_dir(state: State<'_, AppState>) -> Result<(), String> {
    std::fs::create_dir_all(&state.config_dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(&state.config_dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Erase settings, history and every stored API key.
#[tauri::command]
pub fn reset_all(app: AppHandle, state: State<'_, AppState>) -> Result<AppInfo, String> {
    for p in PROVIDER_PRESETS {
        let _ = secrets::delete(p.id);
    }
    let _ = std::fs::remove_file(&state.settings_path);
    let _ = std::fs::remove_file(&state.history_path);
    guard(&state.history).clear();
    let mut fresh = Settings::default();
    fresh.normalize();
    {
        use tauri_plugin_autostart::ManagerExt;
        let _ = app.autolaunch().disable();
    }
    *guard(&state.settings) = fresh;
    hotkeys::register_all(&app);
    Ok(get_app_info(app.clone(), state))
}

#[tauri::command]
pub fn open_menu_from_settings(app: AppHandle) {
    flows::open_menu(&app, false);
}

pub fn open_settings(app: &AppHandle, section: Option<&str>) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        if let Some(section) = section {
            let _ = app.emit_to("settings", "navigate", section.to_string());
        }
    }
}

#[tauri::command]
pub fn show_settings(app: AppHandle, section: Option<String>) {
    flows::hide_popup(&app);
    open_settings(&app, section.as_deref());
}
