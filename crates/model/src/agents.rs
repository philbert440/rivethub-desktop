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
}

pub const DEFAULT_COLOR: u32 = 0x006c_dbac;

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
    use super::{Agent, parse_color};
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
}
