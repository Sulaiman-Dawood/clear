//! Shortcut strings ("chords"): parsing, validation and the standard spelling.
//!
//! Supported forms:
//! - Ctrl+Shift+K           modifiers + a key
//! - Insert, PageDown, F8   a single non-typing key on its own
//!
//! Tokens: Ctrl, Alt, Shift, Super (Win), and key names like K, 5, F12,
//! Space, PageDown.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// Letters, digits, punctuation, Space: on their own they're typing.
    Typing,
    /// Enter, Tab, Backspace, Delete, Escape, arrows: needed for editing.
    Editing,
    /// Insert, Home, End, Page Up/Down: usable alone, but lose their usual job.
    Navigation,
    /// F-keys, Pause, Scroll Lock, Print Screen, Menu: safe on their own.
    Function,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub name: &'static str,
    pub kind: KeyKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: Option<Key>,
}

const KEYS: &[(&str, KeyKind)] = {
    use KeyKind::*;
    &[
        ("A", Typing), ("B", Typing), ("C", Typing), ("D", Typing), ("E", Typing), ("F", Typing),
        ("G", Typing), ("H", Typing), ("I", Typing), ("J", Typing), ("K", Typing), ("L", Typing),
        ("M", Typing), ("N", Typing), ("O", Typing), ("P", Typing), ("Q", Typing), ("R", Typing),
        ("S", Typing), ("T", Typing), ("U", Typing), ("V", Typing), ("W", Typing), ("X", Typing),
        ("Y", Typing), ("Z", Typing),
        ("0", Typing), ("1", Typing), ("2", Typing), ("3", Typing), ("4", Typing),
        ("5", Typing), ("6", Typing), ("7", Typing), ("8", Typing), ("9", Typing),
        ("Space", Typing), ("Backquote", Typing), ("Minus", Typing), ("Equal", Typing),
        ("BracketLeft", Typing), ("BracketRight", Typing), ("Backslash", Typing), ("Semicolon", Typing),
        ("Quote", Typing), ("Comma", Typing), ("Period", Typing), ("Slash", Typing),
        ("Numpad0", Typing), ("Numpad1", Typing), ("Numpad2", Typing), ("Numpad3", Typing),
        ("Numpad4", Typing), ("Numpad5", Typing), ("Numpad6", Typing), ("Numpad7", Typing),
        ("Numpad8", Typing), ("Numpad9", Typing), ("NumpadMultiply", Typing), ("NumpadAdd", Typing),
        ("NumpadSubtract", Typing), ("NumpadDecimal", Typing), ("NumpadDivide", Typing),
        ("Enter", Editing), ("Tab", Editing), ("Backspace", Editing), ("Delete", Editing),
        ("Escape", Editing), ("ArrowUp", Editing), ("ArrowDown", Editing), ("ArrowLeft", Editing),
        ("ArrowRight", Editing),
        ("Insert", Navigation), ("Home", Navigation), ("End", Navigation),
        ("PageUp", Navigation), ("PageDown", Navigation),
        ("F1", Function), ("F2", Function), ("F3", Function), ("F4", Function), ("F5", Function),
        ("F6", Function), ("F7", Function), ("F8", Function), ("F9", Function), ("F10", Function),
        ("F11", Function), ("F12", Function), ("F13", Function), ("F14", Function), ("F15", Function),
        ("F16", Function), ("F17", Function), ("F18", Function), ("F19", Function), ("F20", Function),
        ("F21", Function), ("F22", Function), ("F23", Function), ("F24", Function),
        ("Pause", Function), ("ScrollLock", Function), ("PrintScreen", Function), ("ContextMenu", Function),
    ]
};

/// Other spellings people (and older versions) use for the same keys.
const ALIASES: &[(&str, &str)] = &[
    ("pgdn", "PageDown"), ("pagedn", "PageDown"), ("pgup", "PageUp"),
    ("ins", "Insert"), ("del", "Delete"), ("esc", "Escape"), ("return", "Enter"),
    ("up", "ArrowUp"), ("down", "ArrowDown"), ("left", "ArrowLeft"), ("right", "ArrowRight"),
    ("prtsc", "PrintScreen"), ("printscr", "PrintScreen"), ("scrlk", "ScrollLock"),
    ("apps", "ContextMenu"), ("menu", "ContextMenu"),
];

pub fn key_by_name(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    let lower = lower.strip_prefix("key").filter(|r| r.len() == 1).map(str::to_string).unwrap_or(lower);
    let lower = lower.strip_prefix("digit").filter(|r| r.len() == 1).map(str::to_string).unwrap_or(lower);
    let canonical = ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map(|(_, real)| real.to_ascii_lowercase())
        .unwrap_or(lower);
    KEYS.iter()
        .find(|(n, _)| n.to_ascii_lowercase() == canonical)
        .map(|&(name, kind)| Key { name, kind })
}

pub fn parse(text: &str) -> Result<Chord, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("The shortcut is empty.".into());
    }
    let mut c = Chord::default();
    for raw in text.split('+') {
        let token = raw.trim();
        if token.is_empty() {
            return Err(format!("\"{text}\" isn't a valid shortcut."));
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => c.ctrl = true,
            "alt" | "option" => c.alt = true,
            "shift" => c.shift = true,
            "super" | "win" | "meta" | "cmd" | "command" => c.win = true,
            side if ["lctrl", "rctrl", "lalt", "ralt", "lshift", "rshift", "lwin", "rwin"].contains(&side) => {
                return Err("Left/right-specific keys aren't supported. Record the shortcut again.".into());
            }
            _ => {
                if c.key.is_some() {
                    return Err("A shortcut can have only one key besides Ctrl, Alt, Shift and Win.".into());
                }
                c.key = Some(key_by_name(token).ok_or_else(|| format!("\"{token}\" isn't a key Clear can use in a shortcut."))?);
            }
        }
    }
    let Some(key) = c.key else {
        return Err("Add a key: Ctrl, Alt, Shift and Win can't be a shortcut on their own.".into());
    };
    if matches!(key.kind, KeyKind::Typing | KeyKind::Editing) && !(c.ctrl || c.alt || c.win) {
        return Err(format!(
            "{} {} would get in the way of typing. Add Ctrl, Alt or Win.",
            key.name,
            if c.shift { "with only Shift" } else { "on its own" }
        ));
    }
    Ok(c)
}

impl Chord {
    /// The standard form stored in settings, e.g. "Ctrl+Shift+Space", "PageDown".
    pub fn canonical(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.win {
            parts.push("Super");
        }
        if let Some(key) = self.key {
            parts.push(key.name);
        }
        parts.join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_canonicalizes() {
        assert_eq!(parse("ctrl+shift+space").unwrap().canonical(), "Ctrl+Shift+Space");
        assert_eq!(parse("Shift+Ctrl+KeyK").unwrap().canonical(), "Ctrl+Shift+K");
        assert_eq!(parse("PgDn").unwrap().canonical(), "PageDown");
        assert_eq!(parse("Insert").unwrap().canonical(), "Insert");
        assert_eq!(parse("Win+Alt+Digit5").unwrap().canonical(), "Alt+Super+5");
    }

    #[test]
    fn rejects_what_isnt_supported() {
        // Would break typing.
        assert!(parse("K").is_err());
        assert!(parse("Shift+K").is_err());
        assert!(parse("Space").is_err());
        assert!(parse("Delete").is_err());
        // Left/right sides and modifier-only shortcuts aren't supported.
        assert!(parse("RCtrl+K").is_err());
        assert!(parse("RCtrl").is_err());
        assert!(parse("Ctrl+Shift").is_err());
        // Malformed.
        assert!(parse("Ctrl+K+J").is_err());
        assert!(parse("Ctrl+Banana").is_err());
        // Fine.
        assert!(parse("Alt+Delete").is_ok());
        assert!(parse("Shift+F3").is_ok());
        assert!(parse("Insert").is_ok());
        assert!(parse("PageDown").is_ok());
    }
}
