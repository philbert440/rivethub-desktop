use app::Hub;
use gateway::GatewayError;
use gpui::{
    AnyElement, ClickEvent, Context, Entity, EventEmitter, IntoElement, MouseDownEvent, ParentElement,
    Pixels, Point, Render, Styled, Subscription, Window, anchored, deferred, div, prelude::*, px,
};
use model::TopicId;

use crate::memory::Focus;
use crate::theme::Theme;

pub enum MenuEvent {
    ViewInMemory(Focus),
    Deleted(TopicId),
    Closed,
}

/// The right-click menu on a topic or session: show it in Memory, or delete a topic after asking.
pub struct Menu {
    hub: Hub,
    focus: Focus,
    topic: Option<String>,
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
    /// `topic` names the focused topic when it can be deleted from here.
    pub fn new(hub: Hub, focus: Focus, topic: Option<String>) -> Self {
        Self {
            hub,
            focus,
            topic,
            confirming: false,
            busy: false,
            problem: None,
        }
    }

    fn delete(&mut self, cx: &mut Context<Self>) {
        let Focus::Topic(topic) = self.focus.clone() else {
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
        let focus = self.focus.clone();
        div()
            .flex()
            .flex_col()
            .child(Self::item("view-in-memory", "View in Memory").on_click(
                cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(MenuEvent::ViewInMemory(focus.clone()))),
            ))
            .when(self.topic.is_some(), |items| {
                items.child(
                    Self::item("delete-topic", "Delete topic")
                        .text_color(Theme::warn())
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.confirming = true;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    fn confirm(&self, name: &str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .max_w(px(320.))
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(format!("Delete topic “{name}”? Its sessions go back to Forming."))
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
        let body = match (&self.topic, self.confirming) {
            (Some(name), true) => self.confirm(name, cx),
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
            .on_mouse_down_out(cx.listener(|_, _: &MouseDownEvent, _, cx| cx.emit(MenuEvent::Closed)))
            .child(body)
    }
}
