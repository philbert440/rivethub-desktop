use std::time::Duration;

const FIRST: Duration = Duration::from_millis(500);
/// This client's own node comes back within moments of a restart, such as an update, and asking it
/// again costs next to nothing.
const LOCAL_CAP: Duration = Duration::from_secs(3);
const REMOTE_CAP: Duration = Duration::from_secs(15);

/// How long a stream waits before reconnecting: twice as long after each failed try, up to a cap
/// that depends on which node it follows.
#[derive(Debug)]
pub struct Backoff {
    next: Duration,
    cap: Duration,
}

impl Backoff {
    pub fn new(local: bool) -> Self {
        Self {
            next: FIRST,
            cap: if local { LOCAL_CAP } else { REMOTE_CAP },
        }
    }

    pub fn reset(&mut self) {
        self.next = FIRST;
    }

    pub fn next_wait(&mut self) -> Duration {
        let wait = self.next;
        self.next = (self.next * 2).min(self.cap);
        wait
    }
}
