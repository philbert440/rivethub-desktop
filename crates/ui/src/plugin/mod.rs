mod blocks;
mod canvas;
mod form;
mod frame;
mod overlay;
mod run;
mod settings;
mod web_panel;

pub use canvas::{Activate, ClearSelection, DeleteSelection, FitView, ZoomIn, ZoomOut};
pub use frame::Frame;
pub use settings::{PluginSettings, SettingsEvent, adds};
pub use web_panel::move_focus;

use std::collections::HashMap;

use app::{Hub, NodeNotice, PluginWatch};
use gpui::{
    Context, Entity, EventEmitter, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Task, WeakEntity, Window, div, prelude::*,
};
use model::plugin::view::{Action, Confirm, Tone};
use model::plugin::{Context as About, Notice, PageKey, Params, Plugin, PluginRef, SessionAction};
use model::session::SessionSummary;
use model::{SessionId, TopicId};

use crate::theme::Theme;
use frame::{FrameEvent, Placement, Pressed};

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

impl Plugins {
    pub fn new(hub: Hub, cx: &mut Context<Self>) -> Self {
        let (watch, mut notices) = hub.watch_plugins();
        let follow = cx.spawn(async move |this, cx| {
            while let Some(NodeNotice { node, notice }) = notices.recv().await {
                let followed = this.update(cx, |this, cx| match notice {
                    None => this.load(cx),
                    // A plug-in that starts running, stops or finishes a job: its views on
                    // screen, such as a card that said it isn't running, show where it is now.
                    Some(Notice::Changed(plugin)) => {
                        this.load(cx);
                        this.refresh(&PluginRef { node, id: plugin }, None, cx);
                    }
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
                    Some(card.title.clone()),
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
        let page = self.find(&key.plugin).and_then(|plugin| {
            plugin
                .pages
                .iter()
                .find(|page| Some(&page.id) == key.entry.as_ref())
                .map(|page| (page.view.clone(), page.title.clone()))
        });
        let (view, title) = page.unzip();
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
                title,
                cx,
            );
            let shown = Self::show(frame, Origin::Page(key.clone()), cx);
            self.pages.insert(key.clone(), vec![shown]);
            cx.notify();
        }
    }

    /// A frame for one of `plugin`'s views; `named` is what its header says until the view
    /// answers, the plug-in's name when it's `None`.
    #[expect(clippy::too_many_arguments, reason = "each is part of what a frame shows")]
    fn frame(
        &self,
        plugin: PluginRef,
        view: String,
        params: Option<Params>,
        context: About,
        placement: Placement,
        named: Option<String>,
        cx: &mut Context<Self>,
    ) -> Entity<Frame> {
        let hub = self.hub.clone();
        let named = named
            .or_else(|| self.find(&plugin).map(|known| known.name.clone()))
            .unwrap_or_default();
        cx.new(|cx| {
            let mut frame = Frame::new(hub, plugin, view, params, context, placement, cx);
            frame.named = named;
            frame
        })
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
