//! API keys live in the OS credential store (Windows Credential Manager),
//! never in the settings file and never sent to the UI.

const SERVICE: &str = "app.clear.writer";

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, provider).map_err(|e| e.to_string())
}

pub fn get(provider: &str) -> Option<String> {
    entry(provider)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

pub fn has(provider: &str) -> bool {
    get(provider).is_some()
}

pub fn set(provider: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("The key is empty.".into());
    }
    entry(provider)?
        .set_password(key)
        .map_err(|e| format!("Couldn't save the key: {e}"))
}

pub fn delete(provider: &str) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Couldn't remove the key: {e}")),
    }
}

pub fn service_name() -> &'static str {
    SERVICE
}
