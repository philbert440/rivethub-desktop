mod sharing;
mod words;

pub use sharing::Sharing;
pub use words::Noun;

use std::future::Future;

use app::Hub;
use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Div, Entity, EventEmitter, IntoElement, MouseDownEvent, ParentElement,
    Pixels, Point, Render, SharedString, Styled, Subscription, Window, anchored, deferred, div, prelude::*,
    px,
};
use model::plugin::view::Action;
use model::plugin::{PluginRef, SessionAction};
use model::session::{SessionSummary, TaskStop};
use model::trash::Deletion;
use model::visibility::Offer;
use model::{SessionId, TaskId, TopicId};

use words::Doomed;

use crate::brand;
use crate::memory::Focus;
use crate::theme::Theme;

pub enum MenuEvent {
    ViewInMemory(Focus),
    Deleted(TopicId),
    /// rh deleted the session, which leaves the lists until it is used again.
    SessionDeleted(SessionId),
    /// The delegated task the session ran was stopped.
    TaskStopped,
    /// A topic was closed, reopened, moved to the top level, shared or marked private, or a session
    /// marked as history, made active, shared or marked private.
    Filed,
    Rename(TopicId),
    MoveInto(TopicId),
    MergeInto(TopicId),
    Open(Box<SessionSummary>),
    MoveSession(SessionId),
    Unplace(SessionId),
    /// A plug-in's session-menu entry, to run on the session.
    PluginAction {
        plugin: PluginRef,
        action: Action,
        session: SessionId,
    },
    Closed,
}

/// Where Move up a level takes a subtopic: under its grandparent, or to the top level when it has
/// none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lift(pub Option<TopicId>);

/// The right-click menu on a topic or session: show it in Memory; rename, move, merge, close or
/// delete a topic; open, move or file a session, or run a plug-in's entry on it; share either, or
/// mark it private.
pub struct Menu {
    hub: Hub,
    view: Option<Focus>,
    delete: Option<Doomed>,
    stop: Option<Doomed>,
    close: Option<(TopicId, bool)>,
    history: Option<(SessionId, bool)>,
    sharing: Option<(Sharing, Offer)>,
    edits: Option<(TopicId, Option<Lift>, bool)>,
    moves: Option<(SessionSummary, bool)>,
    noun: Noun,
    plugin_actions: Vec<(PluginRef, SessionAction)>,
    confirming: Option<Doomed>,
    busy: bool,
    problem: Option<String>,
}

impl EventEmitter<MenuEvent> for Menu {}

/// An open menu and where it was opened.
pub struct OpenMenu {
    pub menu: Entity<Menu>,
    pub at: Point<Pixels>,
    pub _events: Subscription,
}

impl OpenMenu {
    pub fn overlay(&self) -> impl IntoElement {
        deferred(anchored().position(self.at).child(self.menu.clone()))
    }
}

impl Menu {
    /// `view` is what View in Memory shows, left out inside Memory itself, where a click already opens it.
    /// `delete` names a topic that can be deleted from here.
    pub fn new(hub: Hub, view: Option<Focus>, delete: Option<(TopicId, String)>) -> Self {
        Self {
            hub,
            view,
            delete: delete.map(|(topic, name)| Doomed::Topic(topic, name)),
            stop: None,
            close: None,
            history: None,
            sharing: None,
            edits: None,
            moves: None,
            noun: Noun::Topic,
            plugin_actions: Vec::new(),
            confirming: None,
            busy: false,
            problem: None,
        }
    }

    /// Words the moves and the delete for a type or an entity rather than a topic.
    pub fn naming(mut self, noun: Noun) -> Self {
        self.noun = noun;
        self
    }

    /// Offers Close topic, or Reopen topic when the topic is `closed`.
    pub fn closing(mut self, topic: TopicId, closed: bool) -> Self {
        self.close = Some((topic, closed));
        self
    }

    /// Offers Mark as history, or Make active when the session is already `history`.
    pub fn filing(mut self, session: SessionId, history: bool) -> Self {
        self.history = Some((session, history));
        self
    }

    pub fn sharing(mut self, item: Sharing, offer: Offer) -> Self {
        self.sharing = Some((item, offer));
        self
    }

    /// Offers Rename and Merge into, Move into when the topic fits under some other topic, and
    /// Move up a level for a subtopic, which `up` says where to.
    pub fn editing(mut self, topic: TopicId, up: Option<Lift>, movable: bool) -> Self {
        self.edits = Some((topic, up, movable));
        self
    }

    /// Offers Open and Move to topic, and Remove from topic for a session `placed` in one.
    pub fn moving(mut self, session: SessionSummary, placed: bool) -> Self {
        self.moves = Some((session, placed));
        self
    }

    /// Offers Delete session, which asks first.
    pub fn deleting(mut self, session: SessionSummary) -> Self {
        self.delete = Some(Doomed::Session(Box::new(session)));
        self
    }

    /// Offers Stop task for a session running a delegated task, which asks first.
    pub fn stopping(mut self, session: SessionSummary, task: TaskId) -> Self {
        self.stop = Some(Doomed::Task(Box::new(session), task));
        self
    }

    /// Offers the entries plug-ins add to the menu of the session `moving` names.
    pub fn plugin_actions(mut self, actions: Vec<(PluginRef, SessionAction)>) -> Self {
        self.plugin_actions = actions;
        self
    }

    fn run(
        &mut self,
        work: impl Future<Output = Result<(), GatewayError>> + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.problem = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let done = work.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match done {
                    Ok(()) => cx.emit(MenuEvent::Filed),
                    Err(GatewayError::Conflict(message)) => this.problem = Some(message),
                    Err(err) => this.problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn carry_out(&mut self, doomed: Doomed, cx: &mut Context<Self>) {
        let topic = match doomed {
            Doomed::Topic(topic, _) => topic,
            Doomed::Session(session) => return self.delete_session(*session, cx),
            Doomed::Task(session, task) => return self.stop_task(*session, task, cx),
        };
        self.busy = true;
        self.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let deleted = hub.delete_topic(topic.clone()).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match deleted {
                    Ok(()) => cx.emit(MenuEvent::Deleted(topic)),
                    Err(GatewayError::Conflict(message)) => this.problem = Some(message),
                    Err(err) => this.problem = Some(format!("deleting failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn delete_session(&mut self, session: SessionSummary, cx: &mut Context<Self>) {
        self.busy = true;
        self.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let deleted = hub.delete_session(&session).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match deleted {
                    Ok(Deletion::Deleted) => cx.emit(MenuEvent::SessionDeleted(session.id.clone())),
                    Ok(stayed) => {
                        let why = stayed.refusal().unwrap_or_default();
                        this.problem = Some(format!("It stays: {why}."));
                    }
                    Err(err) => this.problem = Some(format!("deleting failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn stop_task(&mut self, session: SessionSummary, task: TaskId, cx: &mut Context<Self>) {
        self.busy = true;
        self.problem = None;
        cx.notify();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let stopped = hub.stop_task(&session, task).await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match stopped {
                    Ok(TaskStop::Stopped) => cx.emit(MenuEvent::TaskStopped),
                    Ok(TaskStop::AlreadyOver) => {
                        this.problem = Some("That task had already ended.".to_owned());
                    }
                    Err(err) => this.problem = Some(format!("stopping failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn item(id: &'static str, label: impl Into<SharedString>) -> gpui::Stateful<Div> {
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|item| item.bg(Theme::selected()).text_color(Theme::on_selected()))
            .child(label.into())
    }

    fn emits(
        id: &'static str,
        label: &'static str,
        event: impl Fn() -> MenuEvent + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        Self::item(id, label)
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(event())))
            .into_any_element()
    }

    fn button(id: &'static str, label: &'static str) -> gpui::Stateful<Div> {
        div()
            .id(id)
            .h(px(32.))
            .px_4()
            .rounded(px(16.))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(label)
    }

    fn divider() -> Div {
        div().h(px(1.)).mx_2().my_1().bg(Theme::line())
    }

    /// Rename, Move into, Move to top level and Merge into.
    fn topic_edits(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some((topic, up, movable)) = self.edits.clone() else {
            return Vec::new();
        };
        let (renamed, moved, merged, lifted) = (topic.clone(), topic.clone(), topic.clone(), topic);
        let mut items = vec![Self::emits(
            "rename-topic",
            "Rename",
            move || MenuEvent::Rename(renamed.clone()),
            cx,
        )];
        if movable {
            items.push(Self::emits(
                "move-topic",
                self.noun.moves(),
                move || MenuEvent::MoveInto(moved.clone()),
                cx,
            ));
        }
        if let Some(Lift(up)) = up {
            items.push(
                Self::item("lift-topic", "Move up a level")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let hub = this.hub.clone();
                        let (topic, up) = (lifted.clone(), up.clone());
                        this.run(async move { hub.move_topic(topic, up).await }, cx);
                    }))
                    .into_any_element(),
            );
        }
        items.push(Self::emits(
            "merge-topic",
            "Merge into…",
            move || MenuEvent::MergeInto(merged.clone()),
            cx,
        ));
        items
    }

    /// Open, Move to topic and Remove from topic.
    fn session_moves(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some((session, placed)) = self.moves.clone() else {
            return Vec::new();
        };
        let (moved, unplaced) = (session.id.clone(), session.id.clone());
        let opened = Box::new(session);
        let mut items = vec![
            Self::emits(
                "open-session",
                "Open",
                move || MenuEvent::Open(opened.clone()),
                cx,
            ),
            Self::emits(
                "move-session",
                "Move to topic…",
                move || MenuEvent::MoveSession(moved.clone()),
                cx,
            ),
        ];
        if placed {
            items.push(Self::emits(
                "unplace-session",
                "Remove from topic",
                move || MenuEvent::Unplace(unplaced.clone()),
                cx,
            ));
        }
        items
    }

    /// Each plug-in's entry, labelled by the node it runs on.
    fn plugin_items(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some((session, _)) = &self.moves else {
            return Vec::new();
        };
        self.plugin_actions
            .iter()
            .enumerate()
            .map(|(index, (plugin, entry))| {
                let (plugin, action, session) = (plugin.clone(), entry.action.clone(), session.id.clone());
                div()
                    .id(("plugin-action", index))
                    .px_3()
                    .py_2()
                    .rounded(px(10.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|item| item.bg(Theme::selected()).text_color(Theme::on_selected()))
                    .child(brand::icon(
                        brand::symbol(entry.icon.as_deref()),
                        16.,
                        Theme::ink_dim(),
                    ))
                    .child(div().flex_1().child(entry.title.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(Theme::ink_dim())
                            .child(plugin.node.clone()),
                    )
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(MenuEvent::PluginAction {
                            plugin: plugin.clone(),
                            action: action.clone(),
                            session: session.clone(),
                        });
                    }))
                    .into_any_element()
            })
            .collect()
    }

    fn items(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut open = self.session_moves(cx);
        let moves = open.split_off(open.len().min(1));
        let edits = self.topic_edits(cx);
        let plugged = self.plugin_items(cx);
        let shares = self.sharing_items(cx);
        let grouped = !open.is_empty() || !edits.is_empty();
        div()
            .flex()
            .flex_col()
            .children(open)
            .when_some(self.view.clone(), |items, focus| {
                items.child(Self::emits(
                    "view-in-memory",
                    "View in Memory",
                    move || MenuEvent::ViewInMemory(focus.clone()),
                    cx,
                ))
            })
            .children(moves)
            .children(edits)
            .when(!plugged.is_empty(), |items| {
                items.child(Self::divider()).children(plugged)
            })
            .when(
                grouped && (self.close.is_some() || self.history.is_some() || !shares.is_empty()),
                |items| items.child(Self::divider()),
            )
            .when_some(
                self.close
                    .as_ref()
                    .map(|(topic, closed)| (topic.clone(), *closed)),
                |items, (topic, closed)| {
                    let label = if closed { "Reopen topic" } else { "Close topic" };
                    items.child(Self::item("close-topic", label).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let hub = this.hub.clone();
                            let topic = topic.clone();
                            this.run(async move { hub.set_topic_closed(topic, !closed).await }, cx);
                        },
                    )))
                },
            )
            .when_some(
                self.history
                    .as_ref()
                    .map(|(session, history)| (session.clone(), *history)),
                |items, (session, history)| {
                    let label = if history { "Make active" } else { "Mark as history" };
                    items.child(Self::item("mark-history", label).on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            let hub = this.hub.clone();
                            let session = session.clone();
                            this.run(async move { hub.set_history(session, !history).await }, cx);
                        },
                    )))
                },
            )
            .children(shares)
            .children(
                [("stop", &self.stop), ("delete", &self.delete)]
                    .into_iter()
                    .filter_map(|(id, doomed)| Some((id, doomed.clone()?)))
                    .map(|(id, doomed)| {
                        Self::item(id, doomed.label(self.noun))
                            .text_color(Theme::warn())
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.confirming = Some(doomed.clone());
                                cx.notify();
                            }))
                    }),
            )
            .children(
                self.problem
                    .clone()
                    .filter(|_| self.confirming.is_none())
                    .map(|problem| {
                        div()
                            .max_w(px(280.))
                            .px_3()
                            .py_2()
                            .text_xs()
                            .text_color(Theme::warn())
                            .child(problem)
                    }),
            )
            .into_any_element()
    }

    fn confirm(&self, doomed: &Doomed, cx: &mut Context<Self>) -> AnyElement {
        div()
            .max_w(px(320.))
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().font_weight(gpui::FontWeight::MEDIUM).child(doomed.name()))
            .child(
                div()
                    .text_color(Theme::ink_dim())
                    .child(doomed.warning(self.noun)),
            )
            .children(
                self.problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        Self::button("delete-topic-cancel", "Cancel")
                            .text_color(Theme::ink_dim())
                            .hover(|button| button.text_color(Theme::ink()))
                            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(MenuEvent::Closed))),
                    )
                    .child({
                        let doomed = doomed.clone();
                        Self::button("delete-topic-confirm", doomed.verb())
                            .bg(Theme::warn_container())
                            .text_color(Theme::warn())
                            .when(self.busy, |button| button.opacity(0.5))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if !this.busy {
                                    this.carry_out(doomed.clone(), cx);
                                }
                            }))
                    }),
            )
            .into_any_element()
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.confirming.clone() {
            Some(doomed) => self.confirm(&doomed, cx),
            None => self.items(cx),
        };
        div()
            .min_w(px(200.))
            .p_1()
            .rounded(px(16.))
            .bg(Theme::card())
            .border_1()
            .border_color(Theme::line())
            .text_color(Theme::ink())
            .occlude()
            .on_mouse_down_out(cx.listener(|_, _: &MouseDownEvent, _, cx| cx.emit(MenuEvent::Closed)))
            .child(body)
    }
}
