use crate::HarnessId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub color: Option<u32>,
    pub harness: Option<HarnessId>,
    pub model: Option<String>,
    pub node: Option<String>,
    pub directory: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    pub node: String,
    pub agents: Vec<Agent>,
}

impl Agent {
    pub fn launchable_on(&self, node: &str) -> bool {
        self.harness.is_some()
            && self
                .node
                .as_deref()
                .is_none_or(|home| home.eq_ignore_ascii_case(node))
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
            directory: None,
        }
    }

    #[test]
    fn only_agents_with_a_harness_on_this_node_launch_here() {
        assert!(agent(Some("cursor"), Some("Phildesk")).launchable_on("phildesk"));
        assert!(!agent(Some("claude-code"), Some("ct115")).launchable_on("phildesk"));
        assert!(!agent(None, Some("phildesk")).launchable_on("phildesk"));
        assert_eq!(parse_color("#b86e19"), Some(0x00b8_6e19));
        assert_eq!(parse_color("teal"), None);
    }
}
