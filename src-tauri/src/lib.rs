//! Clear: select text in any app, press a shortcut, get better text.

mod actions;
mod chord;
mod commands;
mod flows;
mod history;
mod hotkeys;
mod llm;
mod popup;
mod secrets;
mod settings;
mod winsys;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

use crate::flows::{OpenPayload, Session};
use crate::history::HistoryEntry;
use crate::popup::{Mode, PopupState};
use crate::settings::Settings;

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
    pub history_path: PathBuf,
    pub config_dir: PathBuf,
    pub history: Mutex<Vec<HistoryEntry>>,
    pub session: Mutex<Session>,
    pub popup: Mutex<PopupState>,
    pub last_open: Mutex<Option<OpenPayload>>,
    pub generation: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    pub hotkey_map: Mutex<HashMap<u32, String>>,
    pub hotkey_errors: Mutex<BTreeMap<String, String>>,
    pub hotkeys_paused: AtomicBool,
    /// Set while copying the selection or running a direct shortcut.
    pub busy: AtomicBool,
    /// An in-place shortcut (e.g. Fix) is running.
    pub direct_running: AtomicBool,
    /// The user asked to cancel the running in-place shortcut.
    pub direct_cancel: AtomicBool,
    counter: AtomicU64,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn next_id(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// Switch both windows (pages and the Settings title bar) to a theme:
/// "light", "dark", or "system" to follow Windows.
pub fn apply_theme(app: &tauri::AppHandle, theme: &str) {
    use tauri::Emitter;
    let native = match theme {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    };
    // Only Settings has a native Windows title bar. Calling set_theme on the
    // frameless transparent popup invokes Tao's DefWindowProcW(WM_NCACTIVATE)
    // hack, which paints a classic light-blue title bar onto its GDI surface.
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.set_theme(native);
    }
    let _ = app.emit("theme-changed", theme.to_string());
}

/// Lock a mutex, recovering the data if a previous holder panicked.
pub fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            commands::open_settings(app, None);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| hotkeys::handle(app, shortcut, event))
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("clear"));
            let settings_path = config_dir.join("settings.json");
            let history_path = config_dir.join("history.json");
            let settings = Settings::load(&settings_path);
            let history = history::load(&history_path);

            app.manage(AppState {
                settings: Mutex::new(settings.clone()),
                settings_path,
                history_path,
                config_dir,
                history: Mutex::new(history),
                session: Mutex::new(Session::default()),
                popup: Mutex::new(PopupState::default()),
                last_open: Mutex::new(None),
                generation: Mutex::new(None),
                hotkey_map: Mutex::new(HashMap::new()),
                hotkey_errors: Mutex::new(BTreeMap::new()),
                hotkeys_paused: AtomicBool::new(false),
                busy: AtomicBool::new(false),
                direct_running: AtomicBool::new(false),
                direct_cancel: AtomicBool::new(false),
                counter: AtomicU64::new(0),
                http: llm::http_client(),
            });

            // Windows are created here, after the state exists, so the pages
            // can never call a command before it's ready.
            for window in app.config().app.windows.clone() {
                let built = tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?.build()?;
                if window.label == popup::LABEL {
                    winsys::configure_popup_window(popup::raw_hwnd(&built));
                }
            }

            hotkeys::register_all(app.handle());
            apply_theme(app.handle(), &settings.theme);

            let ask = MenuItem::with_id(app, "ask", "Ask Clear…", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Clear", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&ask, &settings_item, &separator, &quit])?;

            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("Clear")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "ask" => flows::open_menu(app, false),
                    "settings" => commands::open_settings(app, None),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        commands::open_settings(tray.app_handle(), None);
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // First run (or no key yet): open Settings so the user can set up.
            let started_hidden = std::env::args().any(|a| a == "--hidden");
            let needs_setup = {
                let id = &settings.active_provider;
                llm::preset(id).map(|p| p.needs_key).unwrap_or(false) && !secrets::has(id)
            };
            if !started_hidden && needs_setup {
                commands::open_settings(app.handle(), Some("provider"));
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                // Closing a window only hides it; Clear keeps running in the tray.
                api.prevent_close();
                if window.label() == popup::LABEL {
                    flows::hide_popup(window.app_handle());
                } else {
                    let _ = window.hide();
                }
            }
            WindowEvent::Focused(false) if window.label() == popup::LABEL => {
                let state = window.app_handle().state::<AppState>();
                let in_menu = guard(&state.popup).mode == Mode::Menu;
                if in_menu {
                    flows::hide_popup(window.app_handle());
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::popup_sync,
            commands::popup_present,
            commands::popup_resize,
            commands::popup_hide,
            commands::popup_drag,
            commands::popup_begin_resize,
            commands::start_generation,
            commands::cancel_generation,
            commands::cancel_direct,
            commands::apply_result,
            commands::get_app_info,
            commands::save_settings,
            commands::get_theme,
            commands::set_api_key,
            commands::clear_api_key,
            commands::test_connection,
            commands::list_models,
            commands::set_hotkey,
            commands::reset_hotkeys,
            commands::pause_hotkeys,
            commands::get_history,
            commands::clear_history,
            commands::copy_text,
            commands::open_config_dir,
            commands::open_repo,
            commands::reset_all,
            commands::open_menu_from_settings,
            commands::show_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Clear");
}
