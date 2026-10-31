use gpui::Svg;
use model::SessionId;
use model::topics::Topic;
use model::visibility::Offer;

use super::Workspace;
use crate::memory::Focus;
use crate::menu::Sharing;
use crate::visibility::glyph;

impl Workspace {
    pub(super) fn sharing(&self, focus: &Focus) -> Option<(Sharing, Offer)> {
        let me = self.hub.me()?;
        match focus {
            Focus::Topic(topic) => {
                let known = self
                    .topic(topic)
                    .filter(|known| me.owns(known.owner.as_deref()))?;
                Some((
                    Sharing::Topic(topic.clone()),
                    me.offer(known.visibility.as_ref())?,
                ))
            }
            Focus::Session(session) => {
                let shared = self.visibility.get(&session.id);
                let offer = me.offer(shared.map(|shared| &shared.effective))?;
                let item = Sharing::Session {
                    session: session.id.clone(),
                    marked: shared.is_some_and(|shared| shared.marked),
                };
                Some((item, offer))
            }
            Focus::Conversation(_) => None,
        }
    }

    pub(super) fn session_glyph(&self, session: &SessionId) -> Option<Svg> {
        let shared = self.visibility.get(session).map(|shared| &shared.effective);
        glyph(self.hub.me().as_ref(), shared)
    }

    pub(super) fn topic_glyph(&self, topic: &Topic) -> Option<Svg> {
        glyph(self.hub.me().as_ref(), topic.visibility.as_ref())
    }
}
