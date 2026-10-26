use gpui::{
    Context, FocusHandle, Focusable, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    actions, div, img, prelude::*, px,
};
use model::Altitude;

use crate::{brand, theme::Theme};

actions!(workspace, [ZoomOut]);

pub struct Workspace {
    focus: FocusHandle,
    altitude: Altitude,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            altitude: Altitude::Everything,
        }
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.altitude = self.altitude.zoom_out();
        cx.notify();
    }

    fn breadcrumb(&self) -> SharedString {
        match &self.altitude {
            Altitude::Everything => "Everything".into(),
            Altitude::Topic(topic) => format!("Everything › {}", topic.as_str()).into(),
            Altitude::Session {
                topic: Some(topic),
                session,
            } => format!("Everything › {} › {session}", topic.as_str()).into(),
            Altitude::Session { topic: None, session } => format!("Everything › Forming › {session}").into(),
        }
    }

    fn rail() -> impl IntoElement {
        div()
            .w(px(56.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .pt_3()
            .bg(Theme::window())
            .child(img(brand::rh()).size(px(32.)))
    }

    fn header(&self) -> impl IntoElement {
        div()
            .h(px(52.))
            .flex_none()
            .flex()
            .items_center()
            .gap_4()
            .px_5()
            .child(div().text_color(Theme::ink()).child(self.breadcrumb()))
            .child(div().flex_1())
            .child(
                div()
                    .h(px(34.))
                    .px_4()
                    .rounded(px(17.))
                    .bg(Theme::accent())
                    .text_color(Theme::on_accent())
                    .flex()
                    .items_center()
                    .child("+ New session"),
            )
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::zoom_out))
            .size_full()
            .flex()
            .bg(Theme::window())
            .text_color(Theme::ink())
            .text_sm()
            .child(Self::rail())
            .child(
                div().flex_1().flex().flex_col().child(self.header()).child(
                    div()
                        .flex_1()
                        .mr_3()
                        .mb_3()
                        .rounded(px(28.))
                        .bg(Theme::surface())
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(img(brand::ghost()).size(px(96.)))
                        .child(div().text_color(Theme::ink()).child("No topics yet"))
                        .child(
                            div()
                                .text_color(Theme::ink_dim())
                                .child("Start a session and it will find its topic."),
                        ),
                ),
            )
    }
}
