use gpui::{
    App, Context, IntoElement, MouseButton, ParentElement, SharedString, Styled, Window, div, prelude::*, px,
};
use model::topics::Atlas;
use model::{Altitude, SessionId, TopicId};

use super::{ToggleSidePanel, Workspace};
use crate::memory::Focus;
use crate::text::{middle, one_line};

/// About how many characters the location bar's topic and session chips share.
const CRUMB_CHARS: usize = 72;
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
            .max_w(gpui::relative(0.75))
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
        if self.altitude == Altitude::Everything {
            return bar;
        }
        let (levels, session) = self.location();
        let crumbs = levels.len().max(1) + usize::from(session.is_some());
        let fits = (CRUMB_CHARS / crumbs).clamp(10, 28);
        if levels.is_empty() {
            bar = bar.child(separator()).child(
                Self::crumb("crumb-uncategorized", "Uncategorized".into(), false).on_click(cx.listener(
                    |this, _: &gpui::ClickEvent, window, cx| {
                        this.uncategorized_open = true;
                        this.uncategorized_folded = false;
                        this.leave_to(Altitude::Everything, window, cx);
                    },
                )),
            );
        }
        let deepest = levels.len().saturating_sub(1);
        let root = levels.first().cloned();
        for (depth, level) in levels.into_iter().enumerate() {
            let name = self
                .topic(&level)
                .map_or_else(|| level.as_str().to_owned(), |known| known.name.clone());
            let here = session.is_none() && depth == deepest;
            let root = root.clone().unwrap_or_else(|| level.clone());
            let crumb = Self::crumb(("crumb-topic", depth), middle(&name, fits).into(), here)
                .on_mouse_down(
                    MouseButton::Right,
                    Self::on_right_click(Focus::Topic(level.clone()), cx),
                )
                .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    if here && event.click_count() == 2 {
                        this.begin_retitle(level.clone(), window, cx);
                    } else if !here {
                        this.go_to_level(&root, level.clone(), window, cx);
                    }
                }));
            bar = bar.child(separator()).child(crumb);
        }
        if let Some(session) = session {
            let known = self.sessions.iter().find(|known| known.id == session);
            let title = known.map_or_else(|| "Untitled".into(), |known| one_line(known.title.as_deref()));
            bar = bar.child(separator()).child(
                Self::crumb("crumb-session", middle(&title, fits).into(), true).when_some(
                    known.cloned(),
                    |crumb, known| {
                        crumb.on_mouse_down(
                            MouseButton::Right,
                            Self::on_right_click(Focus::Session(Box::new(known)), cx),
                        )
                    },
                ),
            );
        }
        bar
    }

    /// Every topic level above where the workspace stands, root first, and the open session.
    fn location(&self) -> (Vec<TopicId>, Option<SessionId>) {
        let atlas = Atlas::new(&self.topics, &self.sessions, &self.placements);
        let (deepest, session) = match &self.altitude {
            Altitude::Everything => (None, None),
            Altitude::Topic(root) => (Some(self.step_in(root)), None),
            Altitude::Session { topic, session } => {
                let own = self
                    .sessions
                    .iter()
                    .find(|known| &known.id == session)
                    .and_then(|known| atlas.topic_of(known))
                    .cloned();
                (own.or_else(|| topic.clone()), Some(session.clone()))
            }
        };
        let mut levels: Vec<TopicId> = deepest
            .as_ref()
            .map(|deepest| atlas.lineage(deepest).into_iter().cloned().collect())
            .unwrap_or_default();
        levels.reverse();
        (levels, session)
    }

    /// Opens the topic screen at `level`, a topic at or under `root`.
    fn go_to_level(&mut self, root: &TopicId, level: TopicId, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_to(Altitude::Topic(root.clone()), window, cx);
        self.screen.step = (&level != root).then_some(level);
        self.screen.all_history = false;
        self.sync_step(cx);
        cx.notify();
    }

    fn crumb(id: impl Into<gpui::ElementId>, label: SharedString, here: bool) -> gpui::Stateful<gpui::Div> {
        let ink = if here { Theme::on_selected() } else { Theme::ink() };
        let id = id.into();
        let everything = id == gpui::ElementId::from("crumb-everything");
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
            .when(everything, |chip| {
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
