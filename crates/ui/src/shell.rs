use std::sync::Arc;

use app::Hub;
use gpui::{
    Context, Div, Entity, Focusable, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div, img, prelude::*, px,
};
use model::notice::NoticeKind;
use model::{Page, SessionId};

use crate::memory::{MemoryEvent, MemoryPage};
use crate::settings::SettingsPage;
use crate::text::one_line;
use crate::workspace::{Attention, Listed, TopicDeleted, ViewInMemory, Workspace};
use crate::{brand, theme::Theme};

const OPEN_WIDTH: f32 = 208.;
const RAIL_WIDTH: f32 = 56.;

/// The window's root: the left sidebar and whichever page it has selected.
pub struct Shell {
    hub: Hub,
    page: Page,
    sidebar_open: bool,
    workspace: Entity<Workspace>,
    memory: Entity<MemoryPage>,
    settings: Entity<SettingsPage>,
    _routes: [Subscription; 5],
}

impl Shell {
    pub fn new(hub: Hub, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let memory = cx.new(|cx| MemoryPage::new(hub.clone(), cx));
        let settings = cx.new(|cx| SettingsPage::new(hub.clone(), cx));
        let workspace = cx.new(|cx| Workspace::new(hub.clone(), window, cx));
        window.focus(&workspace.focus_handle(cx), cx);
        let to_memory = cx.subscribe_in(&workspace, window, |this, _, ViewInMemory(focus), window, cx| {
            this.memory
                .update(cx, |memory, cx| memory.focus(focus.clone(), cx));
            this.show(Page::Memory, window, cx);
        });
        let deleted_in_workspace = cx.subscribe(&workspace, |this, _, TopicDeleted(topic), cx| {
            this.memory
                .update(cx, |memory, cx| memory.topic_deleted(topic, cx));
        });
        let listed = cx.subscribe(&workspace, |this, _, listed: &Arc<Listed>, cx| {
            this.memory
                .update(cx, |memory, cx| memory.listed(Arc::clone(listed), cx));
        });
        let from_memory = cx.subscribe_in(
            &memory,
            window,
            |this, memory, event: &MemoryEvent, window, cx| match event {
                MemoryEvent::OpenSession(session) => {
                    if !this.open_listed(session, window, cx) {
                        memory.update(cx, MemoryPage::unlisted);
                    }
                }
                MemoryEvent::OpenTopic(topic) => {
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.open_topic(topic.clone(), window, cx);
                    });
                    this.turn_to(Page::Workspace, cx);
                }
                MemoryEvent::TopicDeleted(topic) => {
                    this.workspace
                        .update(cx, |workspace, cx| workspace.topic_deleted(topic, window, cx));
                }
                MemoryEvent::Refiled => {
                    this.workspace.update(cx, Workspace::refiled);
                }
            },
        );
        let notices = cx.subscribe_in(
            &workspace,
            window,
            |this, _, attention: &Attention, window, cx| {
                this.notify(attention, window, cx);
            },
        );
        Self {
            hub,
            page: Page::Workspace,
            sidebar_open: true,
            workspace,
            memory,
            settings,
            _routes: [to_memory, deleted_in_workspace, listed, from_memory, notices],
        }
    }

    fn open_listed(&mut self, session: &SessionId, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let opened = self
            .workspace
            .update(cx, |workspace, cx| workspace.open_listed(session, window, cx));
        if opened {
            self.turn_to(Page::Workspace, cx);
        }
        opened
    }

    /// Raises a desktop notification unless the person turned its kind off or is already looking
    /// at the session; clicking it brings the window forward on that session.
    fn notify(&mut self, attention: &Attention, window: &mut Window, cx: &mut Context<Self>) {
        let on_screen = if self.page == Page::Workspace {
            self.workspace.read(cx).on_screen().cloned()
        } else {
            None
        };
        let notice = &attention.notice;
        if !notice.worth_showing(
            &self.hub.preferences(),
            window.is_window_active(),
            on_screen.as_ref(),
        ) {
            return;
        }
        let headline = match notice.kind {
            NoticeKind::Finished => "Agent finished replying",
            NoticeKind::NeedsYou => "A session needs you",
        };
        let body = one_line(attention.title.as_deref()).to_string();
        let hub = self.hub.clone();
        let session = notice.session.clone();
        cx.spawn_in(window, async move |this, cx| {
            if hub.notify(headline.to_owned(), body).await {
                this.update_in(cx, |this, window, cx| {
                    window.activate_window();
                    this.open_listed(&session, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page == self.page {
            return;
        }
        self.turn_to(page, cx);
        match page {
            Page::Workspace => window.focus(&self.workspace.focus_handle(cx), cx),
            Page::Memory => window.focus(&self.memory.focus_handle(cx), cx),
            Page::Settings => {}
        }
    }

    /// Selects the page without moving focus, for routes that focus something on it themselves.
    fn turn_to(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        self.memory
            .update(cx, |memory, cx| memory.set_shown(page == Page::Memory, cx));
        cx.notify();
    }

    fn nav_item(
        &self,
        page: Page,
        icon: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let here = self.page == page;
        let ink = if here {
            Theme::on_selected()
        } else {
            Theme::ink_dim()
        };
        div()
            .id(label)
            .h(px(44.))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .rounded(px(22.))
            .cursor_pointer()
            .text_color(ink)
            .map(|item| {
                if self.sidebar_open {
                    item.px(px(12.))
                } else {
                    item.w(px(44.)).justify_center()
                }
            })
            .map(|item| {
                if here {
                    item.bg(Theme::selected())
                } else {
                    item.hover(|item| item.bg(Theme::card()))
                }
            })
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                this.show(page, window, cx);
            }))
            .child(brand::icon(icon, 20., ink))
            .when(self.sidebar_open, |item| item.child(label))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.sidebar_open;
        let mark = div()
            .id("toggle-sidebar")
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .child(img(brand::rh()).size(px(32.)))
            .when(open, |mark| {
                mark.child(div().text_base().text_color(Theme::ink()).child("RivetHub"))
            });
        div()
            .w(px(if open { OPEN_WIDTH } else { RAIL_WIDTH }))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .pt_3()
            .pb_3()
            .bg(Theme::window())
            .map(|bar| if open { bar.px_3() } else { bar.items_center() })
            .child(if open {
                div().flex().items_center().justify_between().pb_3().child(mark)
            } else {
                div().flex().flex_col().items_center().gap_2().pb_3().child(mark)
            })
            .child(self.nav_item(Page::Workspace, "icons/message-square.svg", "Workspace", cx))
            .child(self.nav_item(Page::Memory, "icons/library.svg", "Memory", cx))
            .child(div().flex_1())
            .child(self.nav_item(Page::Settings, "icons/settings.svg", "Settings", cx))
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match self.page {
            Page::Workspace => self.workspace.clone().into_any_element(),
            Page::Memory => self.memory.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .bg(Theme::window())
            .font_family(Theme::SANS)
            .text_color(Theme::ink())
            .text_sm()
            .child(self.sidebar(cx))
            .child(div().flex_1().min_w_0().h_full().child(page))
    }
}

/// The bar across the top of every page.
pub fn header() -> Div {
    div().h(px(52.)).flex_none().flex().items_center().gap_4().px_5()
}

pub fn title(text: impl Into<SharedString>) -> Div {
    div().text_lg().text_color(Theme::ink()).child(text.into())
}

/// The rounded surface each page draws its content on.
pub fn tile() -> Div {
    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .mr_3()
        .mb_3()
        .rounded(px(28.))
        .bg(Theme::surface())
        .overflow_hidden()
}
