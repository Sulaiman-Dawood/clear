//! Global shortcuts: registering them and routing presses to actions.
//! Any action can have a shortcut: the menu, Translate, every built-in
//! action and every custom preset. Shortcuts are validated by chord.rs and
//! registered as Windows' standard hotkeys.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::settings::Settings;
use crate::{actions, chord};
use crate::{flows, guard, AppState};

/// Can a shortcut be bound to this action id?
pub fn is_target(settings: &Settings, action: &str) -> bool {
    actions::shortcut_targets(settings).iter().any(|t| t.0 == action)
}

pub fn label(settings: &Settings, action: &str) -> String {
    actions::shortcut_targets(settings)
        .into_iter()
        .find(|t| t.0 == action)
        .map(|t| t.1)
        .unwrap_or_else(|| action.to_string())
}

fn to_shortcut(accel: &str) -> Result<Shortcut, String> {
    let canonical = chord::parse(accel)?.canonical();
    canonical
        .parse::<Shortcut>()
        .map_err(|_| format!("\"{accel}\" isn't a valid shortcut."))
}

/// (Re)register every shortcut from settings. Returns action -> error for
/// any that couldn't be registered (usually because another app owns it).
pub fn register_all(app: &AppHandle) -> BTreeMap<String, String> {
    let state = app.state::<AppState>();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    let settings = guard(&state.settings).clone();
    let mut map = HashMap::new();
    let mut errors = BTreeMap::new();
    for (action, accel) in &settings.hotkeys {
        if accel.trim().is_empty() || !is_target(&settings, action) {
            continue;
        }
        match to_shortcut(accel) {
            Ok(shortcut) => match gs.register(shortcut) {
                Ok(()) => {
                    map.insert(shortcut.id(), action.clone());
                }
                Err(_) => {
                    errors.insert(action.clone(), format!("{accel} is already used by another app. Pick a different shortcut."));
                }
            },
            Err(e) => {
                errors.insert(action.clone(), e);
            }
        }
    }
    *guard(&state.hotkey_map) = map;
    *guard(&state.hotkey_errors) = errors.clone();
    errors
}

/// Stop shortcuts from firing (while recording a new one in Settings).
pub fn pause(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    state.hotkeys_paused.store(paused, Ordering::SeqCst);
    if paused {
        let _ = app.global_shortcut().unregister_all();
    }
}

pub fn handle(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state() != ShortcutState::Pressed {
        return;
    }
    let state = app.state::<AppState>();
    if state.hotkeys_paused.load(Ordering::SeqCst) {
        return;
    }
    let Some(action) = guard(&state.hotkey_map).get(&shortcut.id()).cloned() else { return };
    if action == "menu" {
        return flows::open_menu(app, true);
    }
    let settings = guard(&state.settings).clone();
    match actions::shortcut_mode(&settings, &action) {
        "replace" => flows::run_direct(app, &action),
        _ => flows::open_menu_with(app, true, Some(flows::AutoRun { action, language: None })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' standard hotkeys accept every kind of shortcut chord.rs allows.
    #[test]
    fn standard_hotkeys_accept_supported_shortcuts() {
        for accel in ["Ctrl+Shift+Space", "Ctrl+Alt+T", "PageDown", "PageUp", "Insert", "Home", "End", "F8", "Pause", "Alt+Super+5", "Ctrl+Backquote", "Shift+F3"] {
            assert!(to_shortcut(accel).is_ok(), "standard hotkeys rejected {accel}");
        }
    }
}
