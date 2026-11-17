//! User settings, stored as JSON in the app config folder.
//! API keys are never stored here; see `secrets.rs`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::llm::PROVIDER_PRESETS;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderProfile {
    pub base_url: String,
    pub model: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub label: String,
    pub instruction: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Id of the provider preset in use (see `llm::PROVIDER_PRESETS`).
    pub active_provider: String,
    /// Per-provider base URL and model, so switching back keeps your choices.
    pub providers: BTreeMap<String, ProviderProfile>,
    /// Effort for Claude models: "low", "medium" or "high".
    pub anthropic_effort: String,
    /// Hotkey action id -> accelerator string, e.g. "menu" -> "Ctrl+Shift+Space".
    pub hotkeys: BTreeMap<String, String>,
    pub quick_languages: Vec<String>,
    pub enabled_extras: Vec<String>,
    /// Default ("core") actions the user turned off in the menu.
    pub disabled_actions: Vec<String>,
    /// Per-shortcut behavior: action id -> "replace" (in place) or "popup".
    pub hotkey_modes: BTreeMap<String, String>,
    pub custom_presets: Vec<Preset>,
    pub history_enabled: bool,
    pub launch_at_login: bool,
    /// "system" (follow Windows), "light" or "dark".
    pub theme: String,
}

pub fn default_hotkeys() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("menu".to_string(), "Ctrl+Shift+Space".to_string()),
        ("fix".to_string(), "Ctrl+Alt+Space".to_string()),
        ("translate".to_string(), "Ctrl+Alt+T".to_string()),
    ])
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            active_provider: "anthropic".into(),
            providers: BTreeMap::new(),
            anthropic_effort: "low".into(),
            hotkeys: default_hotkeys(),
            quick_languages: vec!["English".into(), "Arabic".into(), "Spanish".into()],
            enabled_extras: vec!["friendly".into(), "bullets".into()],
            disabled_actions: Vec::new(),
            hotkey_modes: BTreeMap::new(),
            custom_presets: Vec::new(),
            history_enabled: false,
            launch_at_login: false,
            theme: "system".into(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut s: Settings = fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        s.normalize();
        s
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Fill in anything missing so the rest of the app can rely on it.
    pub fn normalize(&mut self) {
        for preset in PROVIDER_PRESETS {
            let profile = self.providers.entry(preset.id.to_string()).or_default();
            if profile.base_url.trim().is_empty() {
                profile.base_url = preset.base_url.to_string();
            }
            if profile.model.trim().is_empty() {
                profile.model = preset.default_model.to_string();
            }
        }
        if !PROVIDER_PRESETS.iter().any(|p| p.id == self.active_provider) {
            self.active_provider = "anthropic".into();
        }
        if !["low", "medium", "high"].contains(&self.anthropic_effort.as_str()) {
            self.anthropic_effort = "low".into();
        }
        for (action, accel) in default_hotkeys() {
            self.hotkeys.entry(action).or_insert(accel);
        }
        let defaults = Settings::default().quick_languages;
        self.quick_languages.truncate(3);
        while self.quick_languages.len() < 3 {
            let next = defaults[self.quick_languages.len()].clone();
            self.quick_languages.push(next);
        }
        self.custom_presets.retain(|p| !p.label.trim().is_empty() && !p.instruction.trim().is_empty());
        self.hotkey_modes.retain(|_, mode| mode == "replace" || mode == "popup");
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        // Drop shortcuts for presets that were deleted.
        let presets: Vec<String> = self.custom_presets.iter().map(|p| format!("preset:{}", p.id)).collect();
        self.hotkeys.retain(|action, _| !action.starts_with("preset:") || presets.contains(action));
        self.hotkey_modes.retain(|action, _| !action.starts_with("preset:") || presets.contains(action));
    }

    pub fn profile(&self, provider: &str) -> ProviderProfile {
        self.providers.get(provider).cloned().unwrap_or_default()
    }
}
