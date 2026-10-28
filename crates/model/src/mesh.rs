#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub url: String,
    pub online: bool,
    pub sessions: Option<u64>,
    /// The rh version the node's own health check reported, absent while it is offline or when
    /// its rh is too old to say.
    pub version: Option<String>,
    pub revision: Option<String>,
}

impl Node {
    /// The node's rh release as a person reads it, such as "rh 0.1.18 · 5216b74".
    pub fn release(&self) -> Option<String> {
        let version = self.version.as_deref()?;
        Some(match self.revision.as_deref() {
            Some(revision) => format!("rh {version} · {revision}"),
            None => format!("rh {version}"),
        })
    }

    pub fn answers_to(&self, name: &str) -> bool {
        self.id.eq_ignore_ascii_case(name) || self.name.eq_ignore_ascii_case(name)
    }
}
