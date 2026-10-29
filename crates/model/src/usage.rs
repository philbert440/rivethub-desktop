use std::ops::AddAssign;

/// A model call shorter than this is too quick to time, so its tokens leave the pace alone.
const TIMED_MS: u64 = 250;

/// The tokens model calls report: what they read, how much of that came from cache, and what
/// they wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

impl Usage {
    /// The share of input served from cache, as a whole percent, when there was input.
    pub fn cached_percent(&self) -> Option<u64> {
        (self.input > 0).then(|| self.cached.min(self.input) * 100 / self.input)
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, other: Self) {
        self.input += other.input;
        self.cached += other.cached;
        self.output += other.output;
    }
}

/// How fast output came: the tokens written over the time spent writing them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pace {
    tokens: u64,
    ms: u64,
}

impl Pace {
    /// One model call's pace, when it wrote something and lasted long enough to time.
    pub fn of(tokens: u64, ms: u64) -> Option<Self> {
        (tokens > 0 && ms >= TIMED_MS).then_some(Self { tokens, ms })
    }

    pub fn tokens_per_second(&self) -> Option<u64> {
        (self.ms > 0).then(|| self.tokens * 1000 / self.ms)
    }
}

impl AddAssign for Pace {
    fn add_assign(&mut self, other: Self) {
        self.tokens += other.tokens;
        self.ms += other.ms;
    }
}

/// A token count as people read it: "812", "1.2k", "24k", "1.3M".
pub fn tokens(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..10_000 => format!("{}.{}k", count / 1_000, count % 1_000 / 100),
        10_000..1_000_000 => format!("{}k", count / 1_000),
        _ => format!("{}.{}M", count / 1_000_000, count % 1_000_000 / 100_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_weighs_calls_by_their_time_and_skips_ones_too_quick_to_time() {
        let mut pace = Pace::default();
        for call in [Pace::of(400, 10_000), Pace::of(200, 0), Pace::of(100, 2_500)] {
            pace += call.unwrap_or_default();
        }
        assert_eq!(pace.tokens_per_second(), Some(40));
        assert_eq!(Pace::default().tokens_per_second(), None);
    }

    #[test]
    fn counts_read_short_and_cache_share_is_capped_at_the_input() {
        assert_eq!(
            [812, 1_250, 24_100, 1_310_000].map(tokens),
            ["812", "1.2k", "24k", "1.3M"]
        );
        let usage = Usage {
            input: 200,
            cached: 500,
            ..Usage::default()
        };
        assert_eq!(usage.cached_percent(), Some(100));
        assert_eq!(Usage::default().cached_percent(), None);
    }
}
