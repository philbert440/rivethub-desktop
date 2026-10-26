#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    fn any(self) -> bool {
        self.ctrl || self.alt || self.shift
    }

    fn csi_param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

fn cursor(letter: char, mods: Modifiers, app_cursor: bool) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[1;{}{letter}", mods.csi_param()).into_bytes()
    } else if app_cursor {
        format!("\x1bO{letter}").into_bytes()
    } else {
        format!("\x1b[{letter}").into_bytes()
    }
}

fn tilde(code: u8, mods: Modifiers) -> Vec<u8> {
    if mods.any() {
        format!("\x1b[{code};{}~", mods.csi_param()).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn control(key: &str) -> Option<u8> {
    let mut chars = key.chars();
    let only = chars.next().filter(|_| chars.next().is_none())?;
    match only {
        'a'..='z' => Some(only as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' => Some(0x1f),
        _ => None,
    }
}

pub fn encode_key(key: &str, typed: Option<&str>, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let alt_prefixed = |mut bytes: Vec<u8>| {
        if mods.alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };
    let named = match key {
        "enter" => Some(alt_prefixed(b"\r".to_vec())),
        "backspace" => Some(alt_prefixed(if mods.ctrl { vec![0x08] } else { vec![0x7f] })),
        "tab" if mods.shift => Some(b"\x1b[Z".to_vec()),
        "tab" => Some(alt_prefixed(b"\t".to_vec())),
        "escape" => Some(alt_prefixed(vec![0x1b])),
        "up" => Some(cursor('A', mods, app_cursor)),
        "down" => Some(cursor('B', mods, app_cursor)),
        "right" => Some(cursor('C', mods, app_cursor)),
        "left" => Some(cursor('D', mods, app_cursor)),
        "home" => Some(cursor('H', mods, app_cursor)),
        "end" => Some(cursor('F', mods, app_cursor)),
        "insert" => Some(tilde(2, mods)),
        "delete" => Some(tilde(3, mods)),
        "pageup" => Some(tilde(5, mods)),
        "pagedown" => Some(tilde(6, mods)),
        "f1" => Some(b"\x1bOP".to_vec()),
        "f2" => Some(b"\x1bOQ".to_vec()),
        "f3" => Some(b"\x1bOR".to_vec()),
        "f4" => Some(b"\x1bOS".to_vec()),
        "f5" => Some(tilde(15, mods)),
        "f6" => Some(tilde(17, mods)),
        "f7" => Some(tilde(18, mods)),
        "f8" => Some(tilde(19, mods)),
        "f9" => Some(tilde(20, mods)),
        "f10" => Some(tilde(21, mods)),
        "f11" => Some(tilde(23, mods)),
        "f12" => Some(tilde(24, mods)),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if mods.ctrl {
        return control(key).map(|byte| alt_prefixed(vec![byte]));
    }
    let text = typed
        .filter(|text| !text.is_empty())
        .or_else(|| (key == "space").then_some(" "))?;
    Some(alt_prefixed(text.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::{Modifiers, encode_key};

    const NONE: Modifiers = Modifiers {
        ctrl: false,
        alt: false,
        shift: false,
    };
    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        alt: false,
        shift: false,
    };
    const ALT: Modifiers = Modifiers {
        ctrl: false,
        alt: true,
        shift: false,
    };

    #[test]
    fn arrows_follow_the_application_cursor_mode() {
        assert_eq!(encode_key("up", None, NONE, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode_key("up", None, NONE, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(encode_key("up", None, CTRL, true), Some(b"\x1b[1;5A".to_vec()));
    }

    #[test]
    fn control_and_alt_chords_map_to_terminal_bytes() {
        assert_eq!(encode_key("c", Some("c"), CTRL, false), Some(vec![0x03]));
        assert_eq!(encode_key("b", Some("b"), ALT, false), Some(b"\x1bb".to_vec()));
        assert_eq!(encode_key("enter", None, NONE, false), Some(b"\r".to_vec()));
        assert_eq!(
            encode_key("é", Some("é"), NONE, false),
            Some("é".as_bytes().to_vec())
        );
    }
}
