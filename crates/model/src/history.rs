use std::collections::{BTreeMap, BTreeSet};

use crate::HarnessId;
use crate::session::SessionSummary;

/// Where a session stands, as the History chips name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    Running,
    NeedsYou,
    Idle,
    Finished,
}

impl Phase {
    pub const ALL: [Self; 4] = [Self::Running, Self::NeedsYou, Self::Idle, Self::Finished];

    /// Waiting on the person outranks working, as the canvas dots rank them, and a session that
    /// is still working hasn't finished whatever its listing last said.
    pub fn of(needs_you: bool, working: bool, finished: bool) -> Self {
        match (needs_you, working, finished) {
            (true, _, _) => Self::NeedsYou,
            (false, true, _) => Self::Running,
            (false, false, true) => Self::Finished,
            (false, false, false) => Self::Idle,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::NeedsYou => "Needs you",
            Self::Idle => "Idle",
            Self::Finished => "Finished",
        }
    }
}

/// The chips picked above History. Picks in one row widen each other; the two rows narrow each
/// other; a row with nothing picked lets everything through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryFilter {
    harnesses: BTreeSet<HarnessId>,
    phases: BTreeSet<Phase>,
}

/// How many sessions each chip stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipCounts {
    /// Every harness in the list and every picked one, so a picked chip stays to be unpicked
    /// when its count drops to 0 or its sessions leave the list.
    pub harnesses: BTreeMap<HarnessId, usize>,
    pub phases: [(Phase, usize); 4],
}

impl HistoryFilter {
    pub fn toggle_harness(&mut self, harness: HarnessId) {
        if !self.harnesses.remove(&harness) {
            self.harnesses.insert(harness);
        }
    }

    pub fn toggle_phase(&mut self, phase: Phase) {
        if !self.phases.remove(&phase) {
            self.phases.insert(phase);
        }
    }

    pub fn has_harness(&self, harness: &HarnessId) -> bool {
        self.harnesses.contains(harness)
    }

    pub fn has_phase(&self, phase: Phase) -> bool {
        self.phases.contains(&phase)
    }

    fn takes_harness(&self, harness: &HarnessId) -> bool {
        self.harnesses.is_empty() || self.harnesses.contains(harness)
    }

    fn takes_phase(&self, phase: Phase) -> bool {
        self.phases.is_empty() || self.phases.contains(&phase)
    }

    /// Whether the session passes the picked chips and the typed filter.
    pub fn shows(&self, session: &SessionSummary, phase: Phase, typed: &str) -> bool {
        self.takes_harness(session.id.harness()) && self.takes_phase(phase) && session.matches(typed)
    }

    /// Each chip counts the typed-filter matches it would show with the other row's picks kept,
    /// so a count says what picking that chip brings into view.
    pub fn counts(
        &self,
        sessions: &[SessionSummary],
        phase: impl Fn(&SessionSummary) -> Phase,
        typed: &str,
    ) -> ChipCounts {
        let mut harnesses: BTreeMap<HarnessId, usize> = self
            .harnesses
            .iter()
            .map(|harness| (harness.clone(), 0))
            .collect();
        let mut phases = Phase::ALL.map(|phase| (phase, 0));
        for session in sessions {
            let harness = session.id.harness();
            let count = harnesses.entry(harness.clone()).or_insert(0);
            if !session.matches(typed) {
                continue;
            }
            let phase = phase(session);
            if self.takes_phase(phase) {
                *count += 1;
            }
            if self.takes_harness(harness)
                && let Some((_, count)) = phases.iter_mut().find(|(chip, _)| *chip == phase)
            {
                *count += 1;
            }
        }
        ChipCounts { harnesses, phases }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionStatus;
    use crate::{NativeSessionId, SessionId};

    fn session(harness: &str, title: &str, status: SessionStatus) -> SessionSummary {
        SessionSummary {
            id: SessionId::new(HarnessId::new(harness), NativeSessionId::new(title)),
            title: Some(title.into()),
            status,
            model: None,
            effort: None,
            cwd: None,
            updated_at_ms: 0,
            parent: None,
            node: None,
            blocked: false,
        }
    }

    #[test]
    fn chips_narrow_across_rows_and_count_with_the_other_row_kept() {
        let sessions = [
            session("claude-code", "fix login", SessionStatus::Active),
            session("claude-code", "docs", SessionStatus::Ended),
            session("codex", "fix build", SessionStatus::Ended),
            session("codex", "fix tests", SessionStatus::Idle),
        ];
        let phase = |session: &SessionSummary| {
            Phase::of(
                false,
                session.status == SessionStatus::Active,
                session.status.is_finished(),
            )
        };
        let mut filter = HistoryFilter::default();
        filter.toggle_phase(Phase::Finished);
        filter.toggle_harness(HarnessId::new("codex"));
        let shown: Vec<_> = sessions
            .iter()
            .filter(|session| filter.shows(session, phase(session), "fix"))
            .filter_map(|session| session.title.as_deref())
            .collect();
        assert_eq!(shown, ["fix build"]);

        let counts = filter.counts(&sessions, phase, "fix");
        let harnesses: Vec<_> = counts
            .harnesses
            .iter()
            .map(|(harness, count)| (harness.as_str(), *count))
            .collect();
        assert_eq!(harnesses, [("claude-code", 0), ("codex", 1)]);
        assert_eq!(
            counts.phases,
            [
                (Phase::Running, 0),
                (Phase::NeedsYou, 0),
                (Phase::Idle, 1),
                (Phase::Finished, 1),
            ]
        );

        // A picked harness whose sessions all left the list keeps its chip to unpick.
        filter.toggle_harness(HarnessId::new("grok-build"));
        let counts = filter.counts(&sessions, phase, "fix");
        assert_eq!(counts.harnesses.get(&HarnessId::new("grok-build")), Some(&0));
    }
}
