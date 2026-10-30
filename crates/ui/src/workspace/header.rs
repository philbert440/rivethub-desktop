use gpui::{
    App, Context, IntoElement, MouseButton, ParentElement, SharedString, Styled, Window, div, prelude::*, px,
};
use model::Altitude;

use super::{ToggleSidePanel, Workspace};
use crate::memory::Focus;
use crate::text::one_line;
use crate::theme::Theme;
use crate::{brand, shell};

impl Workspace {
    /// The location bar, one chip per level; any step above the current one zooms out to it.
    fn crumbs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let separator = || div().flex_none().child("›");
        let mut bar = div()
            .flex()
            .flex_nowrap()
            .items_center()
            .gap_1()
            .min_w_0()
            .max_w(gpui::relative(0.6))
            .p_1()
            .rounded(px(16.))
            .border_1()
            .border_color(Theme::line())
            .bg(Theme::panel())
            .shadow_md()
            .text_sm()
            .text_color(Theme::ink_dim())
            .child(
                Self::crumb(
                    "crumb-everything",
                    "Everything".into(),
                    self.altitude == Altitude::Everything,
                )
                .flex_none()
                .on_click(cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                    this.leave_to(Altitude::Everything, window, cx);
                })),
            );
        let (topic, session) = match &self.altitude {
            Altitude::Everything => return bar,
            Altitude::Topic(topic) => (Some(topic.clone()), None),
            Altitude::Session { topic, session } => (topic.clone(), Some(session.clone())),
        };
        let topic_crumb = match topic {
            Some(topic) => {
                let name = self
                    .topic(&topic)
                    .map_or_else(|| topic.as_str().to_owned(), |known| known.name.clone());
                let here = session.is_none();
                match self.retitle_field(&topic, cx) {
                    Some(field) => div().id("crumb-topic").child(field),
                    None => Self::crumb("crumb-topic", name.into(), here)
                        .on_mouse_down(
                            MouseButton::Right,
                            Self::on_right_click(Focus::Topic(topic.clone()), cx),
                        )
                        .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            if here && event.click_count() == 2 {
                                this.begin_retitle(topic.clone(), window, cx);
                            } else {
                                this.leave_to(Altitude::Topic(topic.clone()), window, cx);
                            }
                        })),
                }
            }
            None => Self::crumb("crumb-uncategorized", "Uncategorized".into(), false).on_click(cx.listener(
                |this, _: &gpui::ClickEvent, window, cx| {
                    this.uncategorized_open = true;
                    this.uncategorized_folded = false;
                    this.leave_to(Altitude::Everything, window, cx);
                },
            )),
        };
        bar = bar.child(separator()).child(topic_crumb);
        if let Some(session) = session {
            let known = self.sessions.iter().find(|known| known.id == session);
            let title = known.map_or_else(|| "Untitled".into(), |known| one_line(known.title.as_deref()));
            bar = bar
                .child(separator())
                .child(Self::crumb("crumb-session", title, true).when_some(
                    known.cloned(),
                    |crumb, known| {
                        crumb.on_mouse_down(
                            MouseButton::Right,
                            Self::on_right_click(Focus::Session(Box::new(known)), cx),
                        )
                    },
                ));
        }
        bar
    }

    fn crumb(id: &'static str, label: SharedString, here: bool) -> gpui::Stateful<gpui::Div> {
        let ink = if here { Theme::on_selected() } else { Theme::ink() };
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_1p5()
            .min_w_0()
            .px_3()
            .py_1()
            .rounded(px(999.))
            .cursor_pointer()
            .map(|chip| {
                if here {
                    chip.bg(Theme::selected())
                        .text_color(Theme::on_selected())
                        .font_weight(gpui::FontWeight::MEDIUM)
                } else {
                    chip.bg(Theme::card())
                        .text_color(Theme::ink())
                        .hover(|chip| chip.bg(Theme::raised()))
                }
            })
            .when(id == "crumb-everything", |chip| {
                chip.child(brand::icon("icons/workspace.svg", 14., ink))
            })
            .child(div().min_w_0().truncate().child(label))
    }

    fn icon_button(
        id: &'static str,
        icon: &'static str,
        listener: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(36.))
            .rounded(px(18.))
            .bg(Theme::card())
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .on_click(listener)
            .child(brand::icon(icon, 18., Theme::ink()))
    }

    pub(super) fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let in_session = matches!(self.altitude, Altitude::Session { .. });
        let back = match self.altitude.zoom_out() {
            Altitude::Topic(_) => "Ctrl ↑ Topic",
            _ => "Ctrl ↑ Everything",
        };
        shell::header()
            .child(self.crumbs(cx))
            // Everything shows problems on the page; elsewhere they'd have nowhere else to go.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .text_color(Theme::warn())
                    .children(self.problems().filter(|_| self.altitude != Altitude::Everything)),
            )
            .when(self.altitude != Altitude::Everything, |header| {
                header.child(div().text_xs().text_color(Theme::ink_dim()).child(back))
            })
            .when(!in_session && !self.panel_open, |header| {
                header
                    .child(Self::icon_button(
                        "open-search",
                        "icons/search.svg",
                        cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.reveal_panel(true, window, cx);
                        }),
                    ))
                    .child(Self::icon_button(
                        "open-history",
                        "icons/history.svg",
                        cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                            this.reveal_panel(false, window, cx);
                        }),
                    ))
            })
            .when(!in_session, |header| {
                header.child(Self::icon_button(
                    "toggle-panel",
                    "icons/side-panel.svg",
                    cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                        this.toggle_panel(&ToggleSidePanel, window, cx);
                    }),
                ))
            })
            .child(
                div()
                    .id("new-session")
                    .h(px(34.))
                    .px_4()
                    .rounded(px(17.))
                    .bg(Theme::accent())
                    .text_color(Theme::on_accent())
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(Self::start))
                    .child(brand::icon("icons/plus.svg", 16., Theme::on_accent()))
                    .child("New session"),
            )
    }
}
