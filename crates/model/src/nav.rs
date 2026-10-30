use crate::{SessionId, TopicId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Workspace,
    Memory,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Altitude {
    #[default]
    Everything,
    Topic(TopicId),
    Session {
        topic: Option<TopicId>,
        session: SessionId,
    },
}

impl Altitude {
    #[must_use]
    pub fn zoom_out(&self) -> Self {
        match self {
            Self::Everything | Self::Topic(_) | Self::Session { topic: None, .. } => Self::Everything,
            Self::Session {
                topic: Some(topic), ..
            } => Self::Topic(topic.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Altitude;
    use crate::{HarnessId, NativeSessionId, SessionId, TopicId};

    fn session() -> SessionId {
        SessionId::new(HarnessId::new("claude-code"), NativeSessionId::new("a5fdd046"))
    }

    #[test]
    fn zoom_out_climbs_one_level_at_a_time() {
        let topic = TopicId::new("rivethub-desktop");
        let inside = Altitude::Session {
            topic: Some(topic.clone()),
            session: session(),
        };
        assert_eq!(inside.zoom_out(), Altitude::Topic(topic));
        assert_eq!(
            Altitude::Topic(TopicId::new("x")).zoom_out(),
            Altitude::Everything
        );
        assert_eq!(Altitude::Everything.zoom_out(), Altitude::Everything);
    }

    #[test]
    fn zoom_out_from_an_uncategorized_session_returns_to_everything() {
        let uncategorized = Altitude::Session {
            topic: None,
            session: session(),
        };
        assert_eq!(uncategorized.zoom_out(), Altitude::Everything);
    }
}
