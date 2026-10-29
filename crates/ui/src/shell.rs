use std::sync::Arc;

use app::Hub;
use gpui::{
    Context, Div, Entity, Focusable, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div, img, prelude::*, px,
};
use model::notice::NoticeKind;
use model::{Page, SessionId};

use crate::memory::{MemoryEvent, MemoryPage};
use crate::plugin::{Entry, Plugins, PluginsEvent};
use crate::settings::SettingsPage;
use crate::text::one_line;
use crate::workspace::{Attention, Listed, TopicDeleted, ViewInMemory, Workspace};
use crate::{brand, theme::Theme};

const OPEN_WIDTH: f32 = 208.;
const RAIL_WIDTH: f32 = 56.;

/// The window's root: the left sidebar and whichever page it has selected, with the plug-ins'
/// dialogs and toasts over it.
pub struct Shell {
    hub: Hub,
    page: Page,
    sidebar_open: bool,
    workspace: Entity<Workspace>,
    memory: Entity<MemoryPage>,
    settings: Entity<SettingsPage>,
    plugins: Entity<Plugins>,
    _routes: [Subscription; 6],
}

impl Shell {
    pub fn new(hub: Hub, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let plugins = cx.new(|cx| Plugins::new(hub.clone(), cx));
        let memory = cx.new(|cx| MemoryPage::new(hub.clone(), cx));
        let settings = cx.new(|cx| SettingsPage::new(hub.clone(), plugins.clone(), cx));
        let workspace = cx.new(|cx| Workspace::new(hub.clone(), plugins.clone(), window, cx));
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
            },
        );
        let notices = cx.subscribe_in(
            &workspace,
            window,
            |this, _, attention: &Attention, window, cx| {
                this.notify(attention, window, cx);
            },
        );
        let from_plugins =
            cx.subscribe_in(
                &plugins,
                window,
                |this, _, event: &PluginsEvent, window, cx| match event {
                    PluginsEvent::Listed => cx.notify(),
                    PluginsEvent::ShowPage(key) => this.turn_to(Page::Plugin(key.clone()), cx),
                    PluginsEvent::OpenSession(session) => {
                        this.open_listed(session, window, cx);
                    }
                    PluginsEvent::OpenTopic(topic) => {
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.open_topic(topic.clone(), window, cx);
                        });
                        this.turn_to(Page::Workspace, cx);
                    }
                },
            );
        Self {
            hub,
            page: Page::Workspace,
            sidebar_open: true,
            workspace,
            memory,
            settings,
            plugins,
            _routes: [
                to_memory,
                deleted_in_workspace,
                listed,
                from_memory,
                notices,
                from_plugins,
            ],
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
        if let Page::Plugin(key) = &page {
            // A page's entry clicked while on it goes back to the page's start.
            let again = page == self.page;
            self.plugins
                .update(cx, |plugins, cx| plugins.open_page(key, again, cx));
        }
        if page == self.page {
            return;
        }
        let focus = match page {
            Page::Workspace => Some(self.workspace.focus_handle(cx)),
            Page::Memory => Some(self.memory.focus_handle(cx)),
            Page::Settings | Page::Plugin(_) => None,
        };
        self.turn_to(page, cx);
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
    }

    /// Selects the page without moving focus, for routes that focus something on it themselves.
    fn turn_to(&mut self, page: Page, cx: &mut Context<Self>) {
        let memory = page == Page::Memory;
        self.page = page;
        self.memory
            .update(cx, |memory_page, cx| memory_page.set_shown(memory, cx));
        cx.notify();
    }

    fn nav_item(
        &self,
        page: Page,
        icon: impl Into<SharedString>,
        label: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let here = self.page == page;
        let label = label.into();
        let ink = if here {
            Theme::on_selected()
        } else {
            Theme::ink_dim()
        };
        let id: SharedString = match &page {
            Page::Plugin(key) => format!(
                "nav-{}-{}-{}",
                key.plugin.node,
                key.plugin.id,
                key.entry.as_deref().unwrap_or_default()
            )
            .into(),
            Page::Workspace | Page::Memory | Page::Settings => format!("nav-{label}").into(),
        };
        div()
            .id(id)
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
                this.show(page.clone(), window, cx);
            }))
            .child(brand::icon(icon, 20., ink))
            .when(self.sidebar_open, |item| {
                item.child(div().min_w_0().truncate().child(label))
            })
    }

    /// A plug-in page's entry, with the node it runs on beneath its title.
    fn plugin_item(&self, entry: Entry, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let here = self.page == Page::Plugin(entry.key.clone());
        let item = self.nav_item(
            Page::Plugin(entry.key),
            brand::symbol(entry.icon.as_deref()),
            entry.title,
            cx,
        );
        item.when(self.sidebar_open, |item| {
            item.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(if here {
                        Theme::on_selected()
                    } else {
                        Theme::ink_dim()
                    })
                    .child(entry.node),
            )
        })
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
            .children(
                self.plugins
                    .read(cx)
                    .entries()
                    .into_iter()
                    .map(|entry| self.plugin_item(entry, cx))
                    .collect::<Vec<_>>(),
            )
            .child(div().flex_1())
            .child(self.nav_item(Page::Settings, "icons/settings.svg", "Settings", cx))
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = match &self.page {
            Page::Workspace => self.workspace.clone().into_any_element(),
            Page::Memory => self.memory.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
            Page::Plugin(key) => self.plugins.read(cx).top(key).map_or_else(
                || {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(Theme::ink_dim())
                        .child("This plug-in isn't running on its node now.")
                        .into_any_element()
                },
                IntoElement::into_any_element,
            ),
        };
        div()
            .relative()
            .size_full()
            .flex()
            .bg(Theme::window())
            .font_family(Theme::SANS)
            .text_color(Theme::ink())
            .text_sm()
            .child(self.sidebar(cx))
            .child(div().flex_1().min_w_0().h_full().child(page))
            .child(self.plugins.clone())
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
