use std::future::Future;

use app::Hub;
use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Div, Entity, EventEmitter, IntoElement, MouseDownEvent, ParentElement,
    Pixels, Point, Render, Styled, Subscription, Window, anchored, deferred, div, prelude::*, px,
};
use model::session::SessionSummary;
use model::{SessionId, TopicId};

use crate::memory::Focus;
use crate::theme::Theme;

pub enum MenuEvent {
    ViewInMemory(Focus),
    Deleted(TopicId),
    /// A topic was closed, reopened or moved to the top level, or a session marked as history or
    /// made active.
    Filed,
    Rename(TopicId),
    MoveInto(TopicId),
    MergeInto(TopicId),
    Open(Box<SessionSummary>),
    MoveSession(SessionId),
    Unplace(SessionId),
    Closed,
}

/// The right-click menu on a topic or session: show it in Memory; rename, move, merge, close or
/// delete a topic; open, move or file a session.
pub struct Menu {
    hub: Hub,
    view: Option<Focus>,
    delete: Option<(TopicId, String)>,
    close: Option<(TopicId, bool)>,
    history: Option<(SessionId, bool)>,
    edits: Option<(TopicId, bool)>,
    moves: Option<(SessionSummary, bool)>,
    confirming: bool,
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
            delete,
            close: None,
            history: None,
            edits: None,
            moves: None,
            confirming: false,
            busy: false,
            problem: None,
        }
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

    /// Offers Rename, Move into and Merge into, and Move to top level for a `subtopic`.
    pub fn editing(mut self, topic: TopicId, subtopic: bool) -> Self {
        self.edits = Some((topic, subtopic));
        self
    }

    /// Offers Open and Move to topic, and Remove from topic for a session `placed` in one.
    pub fn moving(mut self, session: SessionSummary, placed: bool) -> Self {
        self.moves = Some((session, placed));
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

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Some((topic, _)) = self.delete.clone() else {
            return;
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

    fn item(id: &'static str, label: &'static str) -> gpui::Stateful<Div> {
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|item| item.bg(Theme::selected()).text_color(Theme::on_selected()))
            .child(label)
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
        let Some((topic, subtopic)) = self.edits.clone() else {
            return Vec::new();
        };
        let (renamed, moved, merged, lifted) = (topic.clone(), topic.clone(), topic.clone(), topic);
        let mut items = vec![
            Self::emits(
                "rename-topic",
                "Rename",
                move || MenuEvent::Rename(renamed.clone()),
                cx,
            ),
            Self::emits(
                "move-topic",
                "Move into…",
                move || MenuEvent::MoveInto(moved.clone()),
                cx,
            ),
        ];
        if subtopic {
            items.push(
                Self::item("lift-topic", "Move to top level")
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let hub = this.hub.clone();
                        let topic = lifted.clone();
                        this.run(async move { hub.move_topic(topic, None).await }, cx);
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

    fn items(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut open = self.session_moves(cx);
        let moves = open.split_off(open.len().min(1));
        let edits = self.topic_edits(cx);
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
            .when(
                grouped && (self.close.is_some() || self.history.is_some()),
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
            .when(self.delete.is_some(), |items| {
                items.child(
                    Self::item("delete-topic", "Delete topic")
                        .text_color(Theme::warn())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirming = true;
                            cx.notify();
                        })),
                )
            })
            .children(self.problem.clone().filter(|_| !self.confirming).map(|problem| {
                div()
                    .max_w(px(280.))
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(Theme::warn())
                    .child(problem)
            }))
            .into_any_element()
    }

    fn confirm(&self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .max_w(px(320.))
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().font_weight(gpui::FontWeight::MEDIUM).child(name.to_owned()))
            .child(
                div()
                    .text_color(Theme::ink_dim())
                    .child("Delete this topic? Its sessions go back to Uncategorized, and any subtopics move to the top level."),
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
                    .child(
                        Self::button("delete-topic-confirm", "Delete")
                            .bg(Theme::warn_container())
                            .text_color(Theme::warn())
                            .when(self.busy, |button| button.opacity(0.5))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if !this.busy {
                                    this.delete(cx);
                                }
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match (&self.delete, self.confirming) {
            (Some((_, name)), true) => self.confirm(name, cx),
            _ => self.items(cx),
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
