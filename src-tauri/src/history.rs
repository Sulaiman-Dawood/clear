//! The last few transformations, kept on disk only when the user allows it.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const MAX_ENTRIES: usize = 50;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// Unix time in milliseconds.
    pub ts: u64,
    pub action: String,
    pub input: String,
    pub output: String,
}

pub fn load(path: &Path) -> Vec<HistoryEntry> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, entries: &[HistoryEntry]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(entries).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
