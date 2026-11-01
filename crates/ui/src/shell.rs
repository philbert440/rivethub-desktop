use std::sync::Arc;

use app::Hub;
use gpui::{
    Context, Div, Entity, Focusable, FontWeight, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div, prelude::*, px,
};
use model::notice::{NoticeKind, TaskFailure};
use model::{Page, SessionId};

use crate::memory::{MemoryEvent, MemoryPage};
use crate::plugin::{Entry, Plugins, PluginsEvent, move_focus};
use crate::settings::{SettingsEvent, SettingsPage};
use crate::text::one_line;
use crate::tip::Tip;
use crate::workspace::{Attention, Listed, TaskFailed, TopicDeleted, ViewInMemory, Workspace};
use crate::{appearance, brand, theme::Theme};

gpui::actions!(shell, [OpenSettings, FocusNext, FocusPrevious]);

const OPEN_WIDTH: f32 = 224.;
const RAIL_WIDTH: f32 = 48.;

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
    _routes: [Subscription; 9],
    /// Restyles the window when the followed Omarchy theme changes.
    _omarchy: gpui::Task<()>,
}

impl Shell {
    pub fn new(hub: Hub, window: &mut Window, cx: &mut Context<Self>) -> Self {
        appearance::read_omarchy(cx);
        appearance::apply(&hub, window, cx);
        let desktop = cx.observe_window_appearance(window, |this, window, cx| {
            appearance::apply(&this.hub, window, cx);
        });
        let omarchy = Self::follow_omarchy(window, cx);
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
        let from_memory = cx.subscribe_in(&memory, window, Self::memory_asked);
        let from_settings = cx.subscribe(&settings, |this, _, event: &SettingsEvent, cx| match event {
            SettingsEvent::AgentsChanged => {
                this.workspace.update(cx, Workspace::agents_changed);
            }
        });
        let notices = cx.subscribe_in(
            &workspace,
            window,
            |this, _, attention: &Attention, window, cx| {
                this.notify(attention, window, cx);
            },
        );
        let failures = cx.subscribe_in(&workspace, window, |this, _, failed: &TaskFailed, window, cx| {
            this.notify_failure(&failed.0, window, cx);
        });
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
                failures,
                from_settings,
                desktop,
                from_plugins,
            ],
            _omarchy: omarchy,
        }
    }

    /// What Memory asks of the workspace.
    fn memory_asked(
        &mut self,
        memory: &Entity<MemoryPage>,
        event: &MemoryEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            MemoryEvent::OpenSession(session) => {
                if !self.open_listed(session, window, cx) {
                    memory.update(cx, MemoryPage::unlisted);
                }
            }
            MemoryEvent::OpenTopic(topic) => {
                self.workspace.update(cx, |workspace, cx| {
                    workspace.open_topic(topic.clone(), window, cx);
                });
                self.turn_to(Page::Workspace, cx);
            }
            MemoryEvent::TopicDeleted(topic) => {
                self.workspace
                    .update(cx, |workspace, cx| workspace.topic_deleted(topic, window, cx));
            }
            MemoryEvent::Refiled => {
                self.workspace.update(cx, Workspace::refiled);
            }
            MemoryEvent::SessionsDeleted(sessions) => {
                self.workspace.update(cx, |workspace, cx| {
                    workspace.sessions_deleted(sessions, window, cx);
                });
            }
        }
    }

    /// Restyles the window when the Omarchy theme it follows changes.
    fn follow_omarchy(window: &mut Window, cx: &mut Context<Self>) -> gpui::Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(appearance::WATCH).await;
                let Ok(following) = this.update(cx, |this, _| appearance::follows_omarchy(&this.hub)) else {
                    break;
                };
                if !following {
                    continue;
                }
                let theme = cx
                    .background_executor()
                    .spawn(async { app::omarchy::current_theme() })
                    .await;
                let followed = this.update_in(cx, |this, window, cx| {
                    if cx.global::<appearance::Omarchy>().0 != theme {
                        cx.set_global(appearance::Omarchy(theme));
                        appearance::apply(&this.hub, window, cx);
                    }
                });
                if followed.is_err() {
                    break;
                }
            }
        })
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
        if self.hub.preferences().notify_sound {
            self.hub.chime();
        }
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

    /// Raises a desktop notification for a failed task when the person wants to hear what needs
    /// them; clicking it opens the session that delegated the task.
    fn notify_failure(&mut self, failure: &TaskFailure, window: &mut Window, cx: &mut Context<Self>) {
        let prefs = self.hub.preferences();
        if !prefs.wants(NoticeKind::NeedsYou) {
            return;
        }
        if prefs.notify_sound {
            self.hub.chime();
        }
        let hub = self.hub.clone();
        let parent = failure.parent.clone();
        let body = failure.said();
        cx.spawn_in(window, async move |this, cx| {
            if hub.notify("A delegated task failed".to_owned(), body).await
                && let Some(parent) = parent
            {
                this.update_in(cx, |this, window, cx| {
                    window.activate_window();
                    this.open_listed(&parent, window, cx);
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
            .flex_none()
            .flex()
            .items_center()
            .py(px(8.))
            .rounded_full()
            .cursor_pointer()
            .font_weight(FontWeight::MEDIUM)
            .text_color(ink)
            .map(|item| {
                if self.sidebar_open {
                    item.px(px(16.)).gap(px(12.))
                } else {
                    item.justify_center()
                }
            })
            .map(|item| match &page {
                Page::Settings => item.tooltip(Tip::text(format!("{label} · Ctrl+,"))),
                _ if !self.sidebar_open => item.tooltip(Tip::text(label.clone())),
                _ => item,
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
            .child(brand::icon(icon, 18., ink))
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

    /// The wordmark open and its `rh` folded down: either one is the sidebar's toggle.
    fn mark(open: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let (accent, dim) = if open { ("rivet", "hub") } else { ("r", "h") };
        div()
            .id("toggle-sidebar")
            .flex()
            .cursor_pointer()
            .text_xl()
            .font_weight(FontWeight::BOLD)
            .tooltip(Tip::text(if open {
                "Collapse sidebar"
            } else {
                "Expand sidebar"
            }))
            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .child(div().text_color(Theme::accent()).child(accent))
            .child(div().text_color(Theme::ink_dim()).child(dim))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.sidebar_open;
        let inset = px(if open { 8. } else { 4. });
        div()
            .w(px(if open { OPEN_WIDTH } else { RAIL_WIDTH }))
            .flex_none()
            .flex()
            .flex_col()
            .bg(Theme::window())
            .child(
                div()
                    .flex()
                    .items_center()
                    .py(px(16.))
                    .map(|head| {
                        if open {
                            head.px(px(16.))
                        } else {
                            head.justify_center()
                        }
                    })
                    .child(Self::mark(open, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px(inset)
                    .child(self.nav_item(Page::Workspace, "icons/message-square.svg", "Workspace", cx))
                    .child(self.nav_item(Page::Memory, "icons/library.svg", "Memory", cx))
                    .children(
                        self.plugins
                            .read(cx)
                            .entries()
                            .into_iter()
                            .map(|entry| self.plugin_item(entry, cx))
                            .collect::<Vec<_>>(),
                    ),
            )
            .child(div().flex_1())
            .child(div().flex().flex_col().pb_1().px(inset).child(self.nav_item(
                Page::Settings,
                "icons/settings.svg",
                "Settings",
                cx,
            )))
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
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.show(Page::Settings, window, cx);
            }))
            .on_action(|_: &FocusNext, window, cx| move_focus(true, window, cx))
            .on_action(|_: &FocusPrevious, window, cx| move_focus(false, window, cx))
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
