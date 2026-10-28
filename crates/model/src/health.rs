/// An observation older than this no longer tells you how memory is doing now.
const STALE_MS: i64 = 180_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Degraded,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryHealth {
    pub status: Status,
    pub observed_at_ms: Option<i64>,
    /// Embeddings are up, so search ranks by meaning and not just keywords.
    pub semantic: bool,
    pub conversations: u64,
    pub messages: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    NeedsAttention,
    Stale,
}

impl MemoryHealth {
    pub fn verdict(&self, now_ms: i64) -> Verdict {
        match self.observed_at_ms {
            Some(at) if now_ms - at <= STALE_MS => {
                if self.status == Status::Ok {
                    Verdict::Ok
                } else {
                    Verdict::NeedsAttention
                }
            }
            _ => Verdict::Stale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryHealth, STALE_MS, Status, Verdict};

    fn health(status: Status, observed_at_ms: Option<i64>) -> MemoryHealth {
        MemoryHealth {
            status,
            observed_at_ms,
            semantic: true,
            conversations: 0,
            messages: 0,
        }
    }

    #[test]
    fn only_a_fresh_observation_can_say_memory_is_ok() {
        let now = 10 * STALE_MS;
        assert_eq!(health(Status::Ok, Some(now - 1)).verdict(now), Verdict::Ok);
        assert_eq!(
            health(Status::Degraded, Some(now)).verdict(now),
            Verdict::NeedsAttention
        );
        assert_eq!(
            health(Status::Ok, Some(now - STALE_MS - 1)).verdict(now),
            Verdict::Stale
        );
        assert_eq!(health(Status::Error, None).verdict(now), Verdict::Stale);
    }
}
