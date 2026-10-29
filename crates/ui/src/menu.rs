use app::Hub;
use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, IntoElement, MouseDownEvent, ParentElement,
    Pixels, Point, Render, Styled, Subscription, Window, anchored, deferred, div, prelude::*, px,
};
use model::{SessionId, TopicId};

use crate::memory::Focus;
use crate::theme::Theme;

pub enum MenuEvent {
    ViewInMemory(Focus),
    Deleted(TopicId),
    /// A topic was closed or reopened, or a session marked as history or made active.
    Filed,
    Closed,
}

/// The right-click menu on a topic or session: show it in Memory, close or reopen a topic, mark a
/// session as history or make it active, or delete a topic after asking.
pub struct Menu {
    hub: Hub,
    view: Option<Focus>,
    delete: Option<(TopicId, String)>,
    close: Option<(TopicId, bool)>,
    history: Option<(SessionId, bool)>,
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

    fn file(&mut self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let close = self.close.clone();
        let history = self.history.clone();
        self.busy = true;
        self.problem = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let filed = match (close, history) {
                (Some((topic, closed)), _) => hub.set_topic_closed(topic, !closed).await,
                (None, Some((session, history))) => hub.set_history(session, !history).await,
                (None, None) => Ok(()),
            };
            this.update(cx, |this, cx| {
                this.busy = false;
                match filed {
                    Ok(()) => cx.emit(MenuEvent::Filed),
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

    fn item(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|item| item.bg(Theme::selected()).text_color(Theme::on_selected()))
            .child(label)
    }

    fn button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
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

    fn items(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .when_some(self.view.clone(), |items, focus| {
                items.child(
                    Self::item("view-in-memory", "View in Memory").on_click(cx.listener(
                        move |_, _: &ClickEvent, _, cx| cx.emit(MenuEvent::ViewInMemory(focus.clone())),
                    )),
                )
            })
            .when_some(self.close.as_ref().map(|(_, closed)| *closed), |items, closed| {
                let label = if closed { "Reopen topic" } else { "Close topic" };
                items.child(Self::item("close-topic", label).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        if !this.busy {
                            this.file(cx);
                        }
                    },
                )))
            })
            .when_some(
                self.history.as_ref().map(|(_, history)| *history),
                |items, history| {
                    let label = if history { "Make active" } else { "Mark as history" };
                    items.child(Self::item("mark-history", label).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            if !this.busy {
                                this.file(cx);
                            }
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
                    .child("Delete this topic? Its sessions go back to Uncategorized."),
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
