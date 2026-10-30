/// Whether this node can pair a phone: it can when it has a certificate authority to give the
/// phone its own certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingAvailability {
    /// `gateway` is the address the phone will dial.
    Available {
        gateway: String,
    },
    Unavailable {
        reason: String,
    },
}

/// A one-time code a phone scans to get its own certificate for this node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode {
    /// The name the person gave the phone.
    pub name: String,
    pub device_id: String,
    pub gateway: String,
    pub expires_at_ms: i64,
    /// What the QR code holds.
    pub qr_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingState {
    Pending,
    Paired,
    Expired,
}

impl PairingCode {
    /// Whole minutes until the code lapses, rounded up, so it reads "1m" until it is gone.
    pub fn minutes_left(&self, now_ms: i64) -> u64 {
        (self.expires_at_ms - now_ms)
            .max(0)
            .unsigned_abs()
            .div_ceil(60_000)
    }

    pub fn lapsed(&self, now_ms: i64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

/// The names rh accepts for a device: 1 to 64 letters, digits, `.`, `_` and `-`.
pub fn valid_phone_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::{PairingCode, valid_phone_name};

    #[test]
    fn phone_names_and_minutes_left_follow_the_node() {
        assert!(valid_phone_name("pixel-8.home_1"));
        assert!(!valid_phone_name(""));
        assert!(!valid_phone_name("my phone"));
        assert!(!valid_phone_name(&"a".repeat(65)));
        let code = PairingCode {
            name: "pixel".into(),
            device_id: "d".into(),
            gateway: "https://10.0.0.2:5174".into(),
            expires_at_ms: 600_000,
            qr_text: String::new(),
        };
        assert_eq!(code.minutes_left(0), 10);
        assert_eq!(code.minutes_left(599_999), 1);
        assert_eq!(code.minutes_left(700_000), 0);
        assert!(code.lapsed(600_000));
    }
}
