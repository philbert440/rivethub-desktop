use gpui::{AnyElement, ClickEvent, Context, IntoElement, prelude::*};
use model::visibility::Offer;
use model::{SessionId, TopicId};

use super::Menu;

#[derive(Debug, Clone)]
pub enum Sharing {
    Topic(TopicId),
    Session { session: SessionId, marked: bool },
}

impl Menu {
    pub(super) fn sharing_items(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some((item, Offer { to, label })) = self.sharing.clone() else {
            return Vec::new();
        };
        let follows = match &item {
            Sharing::Session {
                session,
                marked: true,
            } => Some(session.clone()),
            Sharing::Topic(_) | Sharing::Session { .. } => None,
        };
        let mut items = vec![
            Self::item("share", label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let (hub, to) = (this.hub.clone(), to.clone());
                    match item.clone() {
                        Sharing::Topic(topic) => this.run(async move { hub.mark_topic(topic, to).await }, cx),
                        Sharing::Session { session, .. } => {
                            this.run(async move { hub.mark_session(session, Some(to)).await }, cx);
                        }
                    }
                }))
                .into_any_element(),
        ];
        items.extend(follows.map(|session| {
            Self::item("follow-topic", "Follow topic")
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let (hub, session) = (this.hub.clone(), session.clone());
                    this.run(async move { hub.mark_session(session, None).await }, cx);
                }))
                .into_any_element()
        }));
        items
    }
}
