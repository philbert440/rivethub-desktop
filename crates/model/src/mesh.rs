#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub url: String,
    pub online: bool,
    pub sessions: Option<u64>,
}

impl Node {
    pub fn answers_to(&self, name: &str) -> bool {
        self.id.eq_ignore_ascii_case(name) || self.name.eq_ignore_ascii_case(name)
    }
}
