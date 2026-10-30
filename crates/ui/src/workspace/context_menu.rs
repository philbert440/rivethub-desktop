use gpui::{App, AppContext, Context, MouseDownEvent, Pixels, Point, Window};
use model::session::SessionStatus;
use model::topics::{Atlas, in_history};

use super::topic_edit::Pick;
use super::{TopicDeleted, ViewInMemory, Workspace};
use crate::memory::Focus;
use crate::menu::{Lift, Menu, MenuEvent, OpenMenu};

impl Workspace {
    /// Right-clicking opens a menu that can show this topic or session in Memory, or delete the topic.
    pub(super) fn on_right_click(
        focus: Focus,
        cx: &mut Context<Self>,
    ) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
            this.open_menu(focus.clone(), event.position, window, cx);
            cx.stop_propagation();
        })
    }

    pub(super) fn open_menu(
        &mut self,
        focus: Focus,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.choosing = None;
        let topic = match &focus {
            Focus::Topic(topic) => self.topic(topic).map(|known| {
                (
                    topic.clone(),
                    known.name.clone(),
                    known.parent.clone(),
                    self.closed(known),
                )
            }),
            Focus::Session(_) | Focus::Conversation(_) => None,
        };
        let session = match &focus {
            Focus::Session(session) => {
                let latest = self
                    .sessions
                    .iter()
                    .find(|known| known.id == session.id)
                    .unwrap_or(session.as_ref());
                Some((
                    latest.clone(),
                    self.placements.contains_key(&latest.id),
                    in_history(latest, &self.marks),
                ))
            }
            Focus::Topic(_) | Focus::Conversation(_) => None,
        };
        let delete = topic.as_ref().map(|(id, name, ..)| (id.clone(), name.clone()));
        let mut menu = Menu::new(self.hub.clone(), Some(focus), delete);
        if let Some((id, _, parent, closed)) = topic {
            let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
            let movable = self
                .topics
                .iter()
                .any(|under| Some(&under.id) != parent.as_ref() && atlas.can_nest(&id, &under.id));
            let up = parent.map(|parent| Lift(self.topic(&parent).and_then(|parent| parent.parent.clone())));
            menu = menu.editing(id.clone(), up, movable).closing(id, closed);
        }
        if let Some((latest, placed, history)) = session {
            let id = latest.id.clone();
            let local = self.hub.local_node();
            let plugged = self.plugins.read(cx).session_actions(&latest, local.as_deref());
            if let Some(task) = latest
                .task
                .clone()
                .filter(|_| latest.status == SessionStatus::Active)
            {
                menu = menu.stopping(latest.clone(), task);
            }
            menu = menu
                .deleting(latest.clone())
                .moving(latest, placed)
                .filing(id, history)
                .plugin_actions(plugged);
        }
        let menu = cx.new(|_| menu);
        let events = cx.subscribe_in(&menu, window, move |this, menu, event: &MenuEvent, window, cx| {
            if this.menu.as_ref().is_none_or(|open| &open.menu != menu) {
                return;
            }
            this.menu = None;
            match event {
                MenuEvent::ViewInMemory(focus) => cx.emit(ViewInMemory(focus.clone())),
                MenuEvent::Deleted(topic) => {
                    this.topic_deleted(topic, window, cx);
                    cx.emit(TopicDeleted(topic.clone()));
                }
                MenuEvent::SessionDeleted(session) => {
                    this.sessions_deleted(std::slice::from_ref(session), window, cx);
                }
                MenuEvent::TaskStopped => this.load(cx),
                MenuEvent::Filed => this.refile(cx),
                MenuEvent::Rename(topic) => this.begin_retitle(topic.clone(), window, cx),
                MenuEvent::MoveInto(topic) => this.choose(Pick::MoveInto(topic.clone()), at, window, cx),
                MenuEvent::MergeInto(topic) => this.choose(Pick::MergeInto(topic.clone()), at, window, cx),
                MenuEvent::Open(session) => this.open((**session).clone(), window, cx),
                MenuEvent::MoveSession(session) => this.choose(Pick::File(session.clone()), at, window, cx),
                MenuEvent::Unplace(session) => this.unplace(session.clone(), cx),
                MenuEvent::PluginAction {
                    plugin,
                    action,
                    session,
                } => this.plugins.update(cx, |plugins, cx| {
                    plugins.run_session_action(plugin.clone(), action.clone(), session.clone(), cx);
                }),
                MenuEvent::Closed => {}
            }
            cx.notify();
        });
        self.menu = Some(OpenMenu {
            menu,
            at,
            _events: events,
        });
        cx.notify();
    }
}
