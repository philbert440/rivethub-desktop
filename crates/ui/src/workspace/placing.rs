use gpui::{AppContext, Context, Entity, Focusable, Subscription, Window};
use model::{SessionId, TopicId};

use super::Workspace;
use crate::topic_picker::{PickerEvent, TopicPicker};

pub(super) struct Placing {
    pub(super) session: SessionId,
    pub(super) picker: Entity<TopicPicker>,
    _events: Subscription,
}

impl Workspace {
    pub(super) fn begin_placing(&mut self, session: SessionId, window: &mut Window, cx: &mut Context<Self>) {
        let topics = self.topics.clone();
        let picker = cx.new(|cx| TopicPicker::new(topics, None, cx));
        let target = session.clone();
        let events = cx.subscribe(&picker, move |this, _, event: &PickerEvent, cx| {
            this.placing = None;
            match event {
                PickerEvent::Chose(topic) => this.place(target.clone(), topic.clone(), cx),
                PickerEvent::Create(name) => this.place_in_new(target.clone(), name.clone(), cx),
                PickerEvent::NoTopic | PickerEvent::Dismissed => {}
            }
            cx.notify();
        });
        window.focus(&picker.focus_handle(cx), cx);
        self.placing = Some(Placing {
            session,
            picker,
            _events: events,
        });
        cx.notify();
    }

    pub(super) fn place(&mut self, session: SessionId, topic: TopicId, cx: &mut Context<Self>) {
        self.failed = None;
        self.placements.insert(session.clone(), topic.clone());
        self.sync_panel(cx);
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let placed = hub.place(session, topic).await;
            this.update(cx, |this, cx| {
                if let Err(err) = placed {
                    this.failed = Some(format!("placing the session failed: {err}"));
                }
                this.refile(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn unplace(&mut self, session: SessionId, cx: &mut Context<Self>) {
        self.failed = None;
        self.placements.remove(&session);
        self.sync_panel(cx);
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let unplaced = hub.unplace(session).await;
            this.update(cx, |this, cx| {
                if let Err(err) = unplaced {
                    this.failed = Some(format!("taking the session out of its topic failed: {err}"));
                }
                this.refile(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn place_in_new(&mut self, session: SessionId, name: String, cx: &mut Context<Self>) {
        self.failed = None;
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let created = hub.place_in_new(session, name).await;
            this.update(cx, |this, cx| {
                if let Err(err) = created {
                    this.failed = Some(format!("creating the topic failed: {err}"));
                }
                this.refile(cx);
            })
            .ok();
        })
        .detach();
    }
}
