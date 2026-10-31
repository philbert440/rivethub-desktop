//! Keys by the names the app's toolkit gives them, as X keysyms (GDK's keyvals).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per modifier key, as the app's toolkit reports them"
)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Modifiers {
    /// As a GDK modifier state.
    pub fn state(self) -> u32 {
        const SHIFT: u32 = 1;
        const CONTROL: u32 = 1 << 2;
        const ALT: u32 = 1 << 3;
        const SUPER: u32 = 1 << 26;
        [
            (self.shift, SHIFT),
            (self.control, CONTROL),
            (self.alt, ALT),
            (self.super_key, SUPER),
        ]
        .into_iter()
        .filter(|(held, _)| *held)
        .fold(0, |state, (_, bit)| state | bit)
    }
}

fn named(key: &str, modifiers: Modifiers) -> Option<u32> {
    let keyval = match key {
        "enter" => 0xff0d,
        "backspace" => 0xff08,
        "tab" if modifiers.shift => 0xfe20,
        "tab" => 0xff09,
        "escape" => 0xff1b,
        "delete" => 0xffff,
        "insert" => 0xff63,
        "left" => 0xff51,
        "up" => 0xff52,
        "right" => 0xff53,
        "down" => 0xff54,
        "home" => 0xff50,
        "end" => 0xff57,
        "pageup" => 0xff55,
        "pagedown" => 0xff56,
        "menu" => 0xff67,
        "space" => 0x20,
        _ => {
            let number: u32 = key.strip_prefix('f')?.parse().ok()?;
            return (1..=24).contains(&number).then(|| 0xffbd + number);
        }
    };
    Some(keyval)
}

/// The keysym for `key`, or for the one character it types. Characters past Latin-1 use the
/// Unicode keysym range, which GDK turns back into the same character.
pub(crate) fn keyval(key: &str, text: Option<&str>, modifiers: Modifiers) -> Option<u32> {
    if let Some(keyval) = named(key, modifiers) {
        return Some(keyval);
    }
    let typed = text.filter(|text| !text.is_empty()).unwrap_or(key);
    let mut chars = typed.chars();
    let (Some(character), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let code = u32::from(character);
    Some(match code {
        0x20..=0x7e | 0xa0..=0xff => code,
        _ => 0x0100_0000 | code,
    })
}

#[cfg(test)]
mod tests {
    use super::{Modifiers, keyval};

    #[test]
    fn a_key_is_its_keysym_and_text_wins_over_the_key_name() {
        let none = Modifiers::default();
        assert_eq!(keyval("a", Some("A"), none), Some(u32::from('A')));
        assert_eq!(keyval("enter", Some("\n"), none), Some(0xff0d));
        assert_eq!(keyval("f5", None, none), Some(0xffc2));
        assert_eq!(keyval("é", None, none), Some(0xe9));
        assert_eq!(keyval("好", None, none), Some(0x0100_597d));
        assert_eq!(keyval("ab", None, none), None);
    }
}
