use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HarnessId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NativeSessionId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId {
    harness: HarnessId,
    native: NativeSessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TopicId(String);

impl HarnessId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name people know the harness by, such as "Grok" for `grok-build`.
    #[must_use]
    pub fn display_name(&self) -> &str {
        match self.0.as_str() {
            "claude-code" => "Claude",
            "grok-build" => "Grok",
            "codex" => "Codex",
            "kimi-code" => "Kimi",
            "hermes" => "Hermes",
            "opencode" => "opencode",
            "cursor" => "Cursor",
            other => other,
        }
    }
}

impl NativeSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl SessionId {
    pub fn new(harness: HarnessId, native: NativeSessionId) -> Self {
        Self { harness, native }
    }

    pub fn harness(&self) -> &HarnessId {
        &self.harness
    }

    pub fn native(&self) -> &NativeSessionId {
        &self.native
    }
}

impl TopicId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.harness.0, self.native.0)
    }
}
