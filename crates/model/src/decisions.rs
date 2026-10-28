use crate::TopicId;
use crate::topics::Topic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Active,
    Superseded,
}

/// A decision kept on a topic. rh renders a root topic's decisions, and its sub-topics', as the
/// page's Decisions section; they change only through rh's decision routes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub id: String,
    pub topic: TopicId,
    pub text: String,
    pub status: Status,
    pub by: String,
    pub updated_ms: i64,
}

impl Decision {
    /// A superseded decision stays on the page as history and can no longer be edited or superseded.
    pub fn is_active(&self) -> bool {
        self.status == Status::Active
    }
}

/// The decisions of one topic, under its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub topic: TopicId,
    pub name: String,
    pub decisions: Vec<Decision>,
}

/// A decision's text as rh takes it: trimmed, and never empty.
pub fn text(typed: &str) -> Option<String> {
    let typed = typed.trim();
    (!typed.is_empty()).then(|| typed.to_owned())
}

/// The topics whose decisions a root topic's page holds: the root, then its sub-topics by name.
pub fn on_page<'a>(topics: &'a [Topic], root: &TopicId) -> Vec<&'a Topic> {
    let mut subtopics: Vec<&Topic> = topics
        .iter()
        .filter(|topic| topic.parent.as_ref() == Some(root))
        .collect();
    subtopics.sort_by_key(|topic| topic.name.to_lowercase());
    topics
        .iter()
        .filter(|topic| &topic.id == root)
        .chain(subtopics)
        .collect()
}
