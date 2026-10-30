mod blocks;
mod canvas;
mod form;
mod frame;
mod settings;

pub use canvas::{Activate, ClearSelection, DeleteSelection, FitView, ZoomIn, ZoomOut};
pub use frame::Frame;
pub use settings::{PluginSettings, SettingsEvent};

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use app::{Hub, NodeNotice, PluginWatch};
use gpui::{
    ClickEvent, ClipboardItem, Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};
use model::plugin::view::{Action, Answer, Confirm, Tone};
use model::plugin::{Context as About, Notice, PageKey, Params, Plugin, PluginRef, SessionAction};
use model::session::SessionSummary;
use model::{SessionId, TopicId};

use crate::brand;
use crate::theme::Theme;
use frame::{FrameEvent, Placement, Pressed};

const TOAST_FOR: Duration = Duration::from_secs(4);
const TOASTS_SHOWN: usize = 3;

pub enum PluginsEvent {
    /// The plug-ins the nodes list changed.
    Listed,
    ShowPage(PageKey),
    OpenSession(SessionId),
    OpenTopic(TopicId),
}

/// A sidebar entry for one of a plug-in's pages, labelled by the node it runs on.
pub struct Entry {
    pub key: PageKey,
    pub title: String,
    pub icon: Option<String>,
    pub node: String,
}

/// A card a plug-in draws in the topic rail.
pub struct Card {
    pub title: String,
    pub node: String,
    pub frame: Entity<Frame>,
}

/// Where a run of actions comes from, which decides where a page or a dialog it opens goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Origin {
    Page(PageKey),
    Dialog,
    Card,
    Session,
}

struct Shown {
    frame: Entity<Frame>,
    _events: Subscription,
}

struct Dialog {
    shown: Shown,
    /// Where the dialog was opened from, where what follows a `back` runs.
    under: Origin,
}

struct Toast {
    id: u64,
    text: SharedString,
    tone: Tone,
}

struct Confirming {
    confirm: Confirm,
    frame: WeakEntity<Frame>,
    pressed: Pressed,
}

/// The plug-ins of every connected node and everything of theirs on screen: their pages, the
/// topic cards, the dialog open and the toasts. Every action a plug-in asks for runs here.
pub struct Plugins {
    hub: Hub,
    installed: Vec<Plugin>,
    unreachable: Vec<String>,
    watch: PluginWatch,
    pages: HashMap<PageKey, Vec<Shown>>,
    cards: HashMap<(PluginRef, String, TopicId), Shown>,
    dialog: Option<Dialog>,
    confirming: Option<Confirming>,
    toasts: Vec<Toast>,
    toasted: u64,
    _notices: Task<()>,
}

impl EventEmitter<PluginsEvent> for Plugins {}

/// Where saved files go: `$XDG_DOWNLOAD_DIR`, else `~/Downloads`.
fn downloads() -> PathBuf {
    std::env::var_os("XDG_DOWNLOAD_DIR").map_or_else(
        || {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Downloads")
        },
        PathBuf::from,
    )
}

impl Plugins {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let (watch, mut notices) = hub.watch_plugins();
        let follow = cx.spawn(async move |this, cx| {
            while let Some(NodeNotice { node, notice }) = notices.recv().await {
                let followed = this.update(cx, |this, cx| match notice {
                    None | Some(Notice::Changed(_)) => this.load(cx),
                    Some(Notice::Views { plugin, views }) => {
                        let plugin = PluginRef { node, id: plugin };
                        this.refresh(&plugin, Some(&views), cx);
                    }
                });
                if followed.is_err() {
                    break;
                }
            }
        });
        let plugins = Self {
            hub,
            installed: Vec::new(),
            unreachable: Vec::new(),
            watch,
            pages: HashMap::new(),
            cards: HashMap::new(),
            dialog: None,
            confirming: None,
            toasts: Vec::new(),
            toasted: 0,
            _notices: follow,
        };
        plugins.load(cx);
        plugins
    }

    pub fn load(&self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let installed = hub.plugins().await;
            this.update(cx, |this, cx| {
                this.watch.follow(&installed.nodes);
                this.installed = installed.plugins;
                this.unreachable = installed.unreachable;
                let installed = &this.installed;
                this.pages
                    .retain(|key, _| installed.iter().any(|plugin| plugin.at == key.plugin));
                this.cards
                    .retain(|(plugin, ..), _| installed.iter().any(|known| &known.at == plugin));
                cx.emit(PluginsEvent::Listed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn installed(&self) -> &[Plugin] {
        &self.installed
    }

    pub fn unreachable(&self) -> &[String] {
        &self.unreachable
    }

    pub fn find(&self, plugin: &PluginRef) -> Option<&Plugin> {
        self.installed.iter().find(|known| &known.at == plugin)
    }

    fn live(&self) -> impl Iterator<Item = &Plugin> {
        self.installed.iter().filter(|plugin| plugin.state.is_live())
    }

    /// A sidebar entry for every page a running plug-in adds.
    pub fn entries(&self) -> Vec<Entry> {
        self.live()
            .flat_map(|plugin| {
                plugin.pages.iter().map(|page| Entry {
                    key: PageKey {
                        plugin: plugin.at.clone(),
                        entry: Some(page.id.clone()),
                    },
                    title: page.title.clone(),
                    icon: page.icon.clone().or_else(|| plugin.icon.clone()),
                    node: plugin.at.node.clone(),
                })
            })
            .collect()
    }

    /// The session-menu entries of the plug-ins on the session's node; `local` names this
    /// client's node, where a session without one lives.
    pub fn session_actions(
        &self,
        session: &SessionSummary,
        local: Option<&str>,
    ) -> Vec<(PluginRef, SessionAction)> {
        let Some(node) = session.node.as_deref().or(local) else {
            return Vec::new();
        };
        self.live()
            .filter(|plugin| plugin.at.node.eq_ignore_ascii_case(node))
            .flat_map(|plugin| {
                plugin
                    .session_actions
                    .iter()
                    .map(|entry| (plugin.at.clone(), entry.clone()))
            })
            .collect()
    }

    pub fn run_session_action(
        &mut self,
        plugin: PluginRef,
        action: Action,
        session: SessionId,
        cx: &mut Context<Self>,
    ) {
        let context = About {
            topic: None,
            session: Some(session),
        };
        self.run(Origin::Session, plugin, context, vec![action], cx);
    }

    /// The cards running plug-ins draw for `topic`, each fetched once while the topic is shown.
    pub fn cards(&mut self, topic: &TopicId, cx: &mut Context<Self>) -> Vec<Card> {
        self.cards.retain(|(_, _, shown), _| shown == topic);
        let wanted: Vec<(PluginRef, model::plugin::TopicCard)> = self
            .live()
            .flat_map(|plugin| {
                plugin
                    .topic_cards
                    .iter()
                    .map(|card| (plugin.at.clone(), card.clone()))
            })
            .collect();
        let mut cards = Vec::new();
        for (plugin, card) in wanted {
            let key = (plugin.clone(), card.id.clone(), topic.clone());
            if !self.cards.contains_key(&key) {
                let context = About {
                    topic: Some(topic.clone()),
                    session: None,
                };
                let frame = self.frame(
                    plugin.clone(),
                    card.view.clone(),
                    None,
                    context,
                    Placement::Card,
                    cx,
                );
                let shown = Self::show(frame, Origin::Card, cx);
                self.cards.insert(key.clone(), shown);
            }
            if let Some(shown) = self.cards.get(&key) {
                cards.push(Card {
                    title: card.title,
                    node: plugin.node,
                    frame: shown.frame.clone(),
                });
            }
        }
        cards
    }

    /// The page shown for `key`: the top of its stack.
    pub fn top(&self, key: &PageKey) -> Option<Entity<Frame>> {
        self.pages
            .get(key)
            .and_then(|stack| stack.last())
            .map(|shown| shown.frame.clone())
    }

    /// Opens a sidebar entry's page, at its first view when it isn't open yet or `to_start`.
    pub fn open_page(&mut self, key: &PageKey, to_start: bool, cx: &mut Context<Self>) {
        let view = self.find(&key.plugin).and_then(|plugin| {
            plugin
                .pages
                .iter()
                .find(|page| Some(&page.id) == key.entry.as_ref())
                .map(|page| page.view.clone())
        });
        if let Some(stack) = self.pages.get_mut(key).filter(|stack| !stack.is_empty()) {
            if !to_start {
                return;
            }
            stack.truncate(1);
            if let (Some(root), Some(view)) = (stack.first(), view) {
                root.frame.update(cx, |frame, cx| frame.retarget(view, None, cx));
            }
            self.settle(key, cx);
            return;
        }
        if let Some(view) = view {
            let frame = self.frame(
                key.plugin.clone(),
                view,
                None,
                About::default(),
                Placement::Page,
                cx,
            );
            let shown = Self::show(frame, Origin::Page(key.clone()), cx);
            self.pages.insert(key.clone(), vec![shown]);
            cx.notify();
        }
    }

    fn frame(
        &self,
        plugin: PluginRef,
        view: String,
        params: Option<Params>,
        context: About,
        placement: Placement,
        cx: &mut Context<Self>,
    ) -> Entity<Frame> {
        let hub = self.hub.clone();
        cx.new(|cx| Frame::new(hub, plugin, view, params, context, placement, cx))
    }

    fn show(frame: Entity<Frame>, origin: Origin, cx: &mut Context<Self>) -> Shown {
        let events = cx.subscribe(&frame, move |this, frame, event: &FrameEvent, cx| match event {
            FrameEvent::Run(actions) => {
                let (plugin, context) = {
                    let frame = frame.read(cx);
                    (frame.plugin().clone(), frame.context.clone())
                };
                this.run(origin.clone(), plugin, context, actions.clone(), cx);
            }
            FrameEvent::Confirm(confirm, pressed) => {
                this.confirming = Some(Confirming {
                    confirm: confirm.clone(),
                    frame: frame.downgrade(),
                    pressed: pressed.clone(),
                });
                cx.notify();
            }
        });
        Shown {
            frame,
            _events: events,
        }
    }

    fn run(
        &mut self,
        origin: Origin,
        plugin: PluginRef,
        context: About,
        actions: Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        let mut origin = origin;
        let mut queue: VecDeque<Action> = actions.into();
        while let Some(action) = queue.pop_front() {
            let leaves = matches!(
                action,
                Action::Navigate { .. } | Action::OpenSession(_) | Action::OpenTopic(_)
            );
            if leaves && origin == Origin::Dialog {
                origin = self.close_dialog(cx);
            }
            match action {
                Action::Call { action, params } => {
                    self.call(origin, plugin, context, (action, params), queue.into(), cx);
                    return;
                }
                Action::Navigate {
                    view,
                    params,
                    replace,
                } => self.navigate(&origin, &plugin, &context, (view, params), replace, cx),
                Action::Dialog { view, params } => {
                    self.open_dialog(&origin, plugin.clone(), context.clone(), (view, params), cx);
                }
                Action::Back => match &origin {
                    Origin::Dialog => origin = self.close_dialog(cx),
                    Origin::Page(key) => self.pop(&key.clone(), cx),
                    Origin::Card | Origin::Session => {}
                },
                Action::Refresh => self.refresh(&plugin, None, cx),
                Action::Toast { text, tone } => self.toast(text, tone, cx),
                Action::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                Action::OpenSession(session) => cx.emit(PluginsEvent::OpenSession(session)),
                Action::OpenTopic(topic) => cx.emit(PluginsEvent::OpenTopic(topic)),
                Action::OpenUrl(url) => cx.open_url(&url),
                Action::Download { path, filename } => self.download(plugin.clone(), path, filename, cx),
            }
        }
    }

    /// Runs one of a plug-in's actions that no form holds, such as a session-menu entry, then
    /// what it answers and what was waiting behind it.
    fn call(
        &self,
        origin: Origin,
        plugin: PluginRef,
        context: About,
        (action, params): (String, Option<Params>),
        rest: Vec<Action>,
        cx: &mut Context<Self>,
    ) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let answered = hub
                .plugin_action(plugin.clone(), action, params, BTreeMap::new(), context.clone())
                .await;
            this.update(cx, |this, cx| match answered {
                Ok(Answer::Done(mut actions)) => {
                    actions.retain(|action| !matches!(action, Action::Call { .. }));
                    actions.extend(rest);
                    this.run(origin, plugin, context, actions, cx);
                }
                Ok(Answer::Refused { message, .. }) => this.toast(message, Tone::Danger, cx),
                Err(err) => this.toast(err.to_string(), Tone::Danger, cx),
            })
            .ok();
        })
        .detach();
    }

    /// The page a navigate from outside a page opens in: the plug-in's first sidebar entry, else
    /// a page of its own.
    fn page_for(&mut self, plugin: &PluginRef, cx: &mut Context<Self>) -> PageKey {
        let entry = self
            .find(plugin)
            .and_then(|known| known.pages.first().map(|page| page.id.clone()));
        let key = PageKey {
            plugin: plugin.clone(),
            entry,
        };
        if key.entry.is_some() {
            self.open_page(&key, false, cx);
        }
        key
    }

    fn navigate(
        &mut self,
        origin: &Origin,
        plugin: &PluginRef,
        context: &About,
        (view, params): (String, Option<Params>),
        replace: bool,
        cx: &mut Context<Self>,
    ) {
        let (key, elsewhere) = match origin {
            Origin::Page(key) => (key.clone(), false),
            Origin::Dialog | Origin::Card | Origin::Session => (self.page_for(plugin, cx), true),
        };
        let top = self.top(&key).filter(|_| replace);
        if let Some(top) = top {
            top.update(cx, |frame, cx| frame.retarget(view, params, cx));
        } else {
            let frame = self.frame(plugin.clone(), view, params, context.clone(), Placement::Page, cx);
            let shown = Self::show(frame, Origin::Page(key.clone()), cx);
            self.pages.entry(key.clone()).or_default().push(shown);
            self.settle(&key, cx);
        }
        if elsewhere {
            cx.emit(PluginsEvent::ShowPage(key));
        }
        cx.notify();
    }

    /// Goes back a page; the page uncovered is fetched again, since what was done above may have
    /// changed it.
    fn pop(&mut self, key: &PageKey, cx: &mut Context<Self>) {
        let Some(stack) = self.pages.get_mut(key).filter(|stack| stack.len() > 1) else {
            return;
        };
        stack.pop();
        if let Some(top) = stack.last() {
            top.frame.update(cx, Frame::load);
        }
        self.settle(key, cx);
    }

    /// Tells the page on top how many lie under it, for its back button.
    fn settle(&self, key: &PageKey, cx: &mut Context<Self>) {
        if let Some(stack) = self.pages.get(key)
            && let Some(top) = stack.last()
        {
            let depth = stack.len() - 1;
            top.frame.update(cx, |frame, cx| frame.set_depth(depth, cx));
        }
        cx.notify();
    }

    fn open_dialog(
        &mut self,
        origin: &Origin,
        plugin: PluginRef,
        context: About,
        (view, params): (String, Option<Params>),
        cx: &mut Context<Self>,
    ) {
        let under = match origin {
            Origin::Dialog => self
                .dialog
                .as_ref()
                .map_or(Origin::Card, |dialog| dialog.under.clone()),
            other => other.clone(),
        };
        let frame = self.frame(plugin, view, params, context, Placement::Dialog, cx);
        let shown = Self::show(frame, Origin::Dialog, cx);
        self.dialog = Some(Dialog { shown, under });
        cx.notify();
    }

    /// Closes the dialog; where it was opened from, for what runs next.
    fn close_dialog(&mut self, cx: &mut Context<Self>) -> Origin {
        cx.notify();
        self.dialog.take().map_or(Origin::Card, |dialog| dialog.under)
    }

    /// Fetches again every view of the plug-in on screen, or only those named.
    fn refresh(&self, plugin: &PluginRef, views: Option<&[String]>, cx: &mut Context<Self>) {
        let shown: Vec<Entity<Frame>> = self
            .pages
            .values()
            .filter_map(|stack| stack.last())
            .chain(self.dialog.iter().map(|dialog| &dialog.shown))
            .chain(self.cards.values())
            .map(|shown| shown.frame.clone())
            .collect();
        for frame in shown {
            let wanted = {
                let frame = frame.read(cx);
                frame.plugin() == plugin
                    && views.is_none_or(|views| views.iter().any(|view| view == frame.view_name()))
            };
            if wanted {
                frame.update(cx, Frame::load);
            }
        }
    }

    fn toast(&mut self, text: impl Into<SharedString>, tone: Tone, cx: &mut Context<Self>) {
        self.toasted += 1;
        let id = self.toasted;
        self.toasts.push(Toast {
            id,
            text: text.into(),
            tone,
        });
        let over = self.toasts.len().saturating_sub(TOASTS_SHOWN);
        self.toasts.drain(..over);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            this.update(cx, |this, cx| {
                this.toasts.retain(|toast| toast.id != id);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Saves one of the plug-in's files where the person picks, or in Downloads when the system
    /// can't ask.
    fn download(&self, plugin: PluginRef, path: String, filename: Option<String>, cx: &mut Context<Self>) {
        let name = filename.unwrap_or_else(|| {
            let route = path.split('?').next().unwrap_or_default();
            let last = route.rsplit('/').next().unwrap_or_default();
            if last.is_empty() {
                "download".to_owned()
            } else {
                last.to_owned()
            }
        });
        let folder = downloads();
        let asked = cx.prompt_for_new_path(&folder, Some(&name));
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let target = match asked.await {
                Ok(Ok(Some(target))) => target,
                Ok(Ok(None)) => return,
                _ => folder.join(&name),
            };
            let saved = match hub.plugin_file(plugin, path).await {
                Ok((bytes, _)) => cx
                    .background_executor()
                    .spawn(async move { std::fs::write(&target, bytes).map(|()| target) })
                    .await
                    .map_err(|err| err.to_string()),
                Err(err) => Err(err.to_string()),
            };
            this.update(cx, |this, cx| match saved {
                Ok(target) => this.toast(format!("Saved {}", target.display()), Tone::Success, cx),
                Err(problem) => this.toast(format!("Couldn't save {name}: {problem}"), Tone::Danger, cx),
            })
            .ok();
        })
        .detach();
    }

    fn confirmed(&mut self, cx: &mut Context<Self>) {
        if let Some(confirming) = self.confirming.take()
            && let Some(frame) = confirming.frame.upgrade()
        {
            frame.update(cx, |frame, cx| frame.run_pressed(confirming.pressed, cx));
        }
        cx.notify();
    }

    fn confirm_card(confirming: &Confirming, cx: &mut Context<Self>) -> impl IntoElement {
        let pill = |id: &'static str| {
            div()
                .id(id)
                .h(px(34.))
                .px_4()
                .rounded(px(17.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_sm()
        };
        div()
            .absolute()
            .inset_0()
            .bg(Theme::scrim())
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(400.))
                    .p_6()
                    .rounded(px(28.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .child(confirming.confirm.title.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(Theme::ink_dim())
                            .child(confirming.confirm.text.clone()),
                    )
                    .child(
                        div()
                            .pt_2()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                pill("confirm-cancel")
                                    .text_color(Theme::ink_dim())
                                    .hover(|pill| pill.text_color(Theme::ink()))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.confirming = None;
                                        cx.notify();
                                    }))
                                    .child("Cancel"),
                            )
                            .child(
                                pill("confirm-go")
                                    .bg(Theme::danger_container())
                                    .text_color(Theme::danger())
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.confirmed(cx)))
                                    .child(
                                        confirming
                                            .confirm
                                            .confirm_label
                                            .clone()
                                            .unwrap_or_else(|| "Confirm".to_owned()),
                                    ),
                            ),
                    ),
            )
    }

    fn toasts(&self) -> impl IntoElement {
        div()
            .absolute()
            .bottom(px(24.))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .children(self.toasts.iter().map(|toast| {
                let (ink, _) = Theme::tone(toast.tone);
                let icon = match toast.tone {
                    Tone::Success => "check_circle",
                    Tone::Warning => "warning",
                    Tone::Danger => "error",
                    Tone::Neutral | Tone::Info => "info",
                };
                div()
                    .max_w(px(520.))
                    .px_4()
                    .py_2()
                    .rounded(px(18.))
                    .bg(Theme::raised())
                    .border_1()
                    .border_color(Theme::line())
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(Theme::ink())
                    .child(brand::icon(brand::symbol(Some(icon)), 18., ink))
                    .child(div().min_w_0().child(toast.text.clone()))
            }))
    }
}

impl Render for Plugins {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .children(self.dialog.as_ref().map(|dialog| {
                div()
                    .absolute()
                    .inset_0()
                    .bg(Theme::scrim())
                    .occlude()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(dialog.shown.frame.clone())
            }))
            .children(
                self.confirming
                    .as_ref()
                    .map(|confirming| Self::confirm_card(confirming, cx)),
            )
            .when(!self.toasts.is_empty(), |layer| layer.child(self.toasts()))
    }
}
