#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Degraded,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryHealth {
    pub status: Status,
    /// Embeddings are up, so search ranks by meaning and not just keywords.
    pub semantic: bool,
    pub conversations: u64,
    pub messages: u64,
}
