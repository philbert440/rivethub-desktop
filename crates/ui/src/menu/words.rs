use model::TopicId;
use model::session::SessionSummary;

/// What the menu's topic is, which words its moves and its delete use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Noun {
    #[default]
    Topic,
    Type,
    Entity,
}

impl Noun {
    pub(super) fn moves(self) -> &'static str {
        match self {
            Self::Topic | Self::Type => "Move into…",
            Self::Entity => "Change type…",
        }
    }

    fn deletes(self) -> &'static str {
        match self {
            Self::Topic => "Delete topic",
            Self::Type => "Delete type",
            Self::Entity => "Delete entity",
        }
    }

    fn warning(self) -> &'static str {
        match self {
            Self::Topic => {
                "Delete this topic? Its sessions go back to Uncategorized, and any subtopics move up a level."
            }
            Self::Type => {
                "Delete this type? Its subtypes and entities move up a level, so a top-level type's entities need another type first."
            }
            Self::Entity => "Delete this entity? Its page goes too, and the wiki won't make it again.",
        }
    }
}

/// What the menu's Delete removes, and what its confirm names.
#[derive(Debug, Clone)]
pub(super) enum Doomed {
    Topic(TopicId, String),
    Session(Box<SessionSummary>),
}

impl Doomed {
    pub(super) fn label(&self, noun: Noun) -> &'static str {
        match self {
            Self::Topic(..) => noun.deletes(),
            Self::Session(_) => "Delete session",
        }
    }

    pub(super) fn name(&self) -> String {
        match self {
            Self::Topic(_, name) => name.clone(),
            Self::Session(session) => session.name(),
        }
    }

    pub(super) fn warning(&self, noun: Noun) -> &'static str {
        match self {
            Self::Topic(..) => noun.warning(),
            Self::Session(_) => {
                "Delete this session? Memory forgets it and it leaves every list. Its harness keeps the transcript, so using the session again brings it back."
            }
        }
    }
}
