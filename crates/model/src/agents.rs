use std::fmt;

use crate::HarnessId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub color: Option<u32>,
    pub harness: Option<HarnessId>,
    pub model: Option<String>,
    pub node: Option<String>,
    pub effort: Option<String>,
    pub directory: Option<String>,
    pub instructions: Option<String>,
}

/// An agent as typed into the editor: everything needed to create or update one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentDraft {
    pub name: String,
    pub color: u32,
    pub harness: Option<HarnessId>,
    pub node: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub directory: Option<String>,
    pub instructions: Option<String>,
}

impl AgentDraft {
    #[must_use]
    pub fn from_agent(agent: &Agent) -> Self {
        Self {
            name: agent.name.clone(),
            color: agent.color.unwrap_or(DEFAULT_COLOR),
            harness: agent.harness.clone(),
            node: agent.node.clone(),
            model: agent.model.clone(),
            effort: agent.effort.clone(),
            directory: agent.directory.clone(),
            instructions: agent.instructions.clone(),
        }
    }

    #[must_use]
    pub fn color_hex(&self) -> String {
        format!("#{:06x}", self.color & 0x00ff_ffff)
    }

    /// rh refuses an agent without a name or a harness, and resolves its folder as given.
    pub fn check(&self) -> Result<(), DraftProblem> {
        if self.name.trim().is_empty() {
            return Err(DraftProblem::NoName);
        }
        if self.harness.is_none() {
            return Err(DraftProblem::NoHarness);
        }
        if self
            .directory
            .as_deref()
            .is_some_and(|folder| !folder.starts_with('/'))
        {
            return Err(DraftProblem::RelativeFolder);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftProblem {
    NoName,
    NoHarness,
    RelativeFolder,
}

impl fmt::Display for DraftProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoName => "Give the agent a name.",
            Self::NoHarness => "Pick a harness.",
            Self::RelativeFolder => "Use an absolute working folder, starting with /.",
        })
    }
}

pub const DEFAULT_COLOR: u32 = 0x006c_dbac;

pub const EFFORTS: [&str; 5] = ["off", "low", "medium", "high", "xhigh"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    pub node: String,
    pub agents: Vec<Agent>,
}

impl Agent {
    /// An agent with a harness starts on its own node, whichever node this client talks to.
    pub fn launchable(&self) -> bool {
        self.harness.is_some()
    }
}

pub fn parse_color(hex: &str) -> Option<u32> {
    let digits = hex.trim().strip_prefix('#')?;
    (digits.len() == 6)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::{Agent, AgentDraft, DraftProblem, parse_color};
    use crate::HarnessId;

    fn agent(harness: Option<&str>, node: Option<&str>) -> Agent {
        Agent {
            id: "a".into(),
            name: "A".into(),
            color: None,
            harness: harness.map(HarnessId::new),
            model: None,
            node: node.map(str::to_owned),
            effort: None,
            directory: None,
            instructions: None,
        }
    }

    #[test]
    fn agents_with_a_harness_launch_from_any_node() {
        assert!(agent(Some("cursor"), Some("Phildesk")).launchable());
        assert!(agent(Some("claude-code"), Some("ct115")).launchable());
        assert!(!agent(None, Some("phildesk")).launchable());
        assert_eq!(parse_color("#b86e19"), Some(0x00b8_6e19));
        assert_eq!(parse_color("teal"), None);
    }

    #[test]
    fn a_draft_needs_a_name_a_harness_and_an_absolute_folder() {
        let mut draft = AgentDraft {
            name: "  ".into(),
            harness: Some(HarnessId::new("codex")),
            ..AgentDraft::default()
        };
        assert_eq!(draft.check(), Err(DraftProblem::NoName));
        draft.name = "Builder".into();
        draft.directory = Some("work/rh".into());
        assert_eq!(draft.check(), Err(DraftProblem::RelativeFolder));
        draft.directory = Some("/home/phil/work/rh".into());
        assert_eq!(draft.check(), Ok(()));
        draft.harness = None;
        assert_eq!(draft.check(), Err(DraftProblem::NoHarness));
    }
}
