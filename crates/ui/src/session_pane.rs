use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use app::{Hub, SessionHandle};
use gpui::{
    Context, Entity, IntoElement, ListAlignment, ListState, ParentElement, Pixels, Render, Styled,
    Subscription, Task, Window, div, prelude::*, px,
};
use model::chat::{ExchangeState, arrived, exchanges};
use model::harness::{HarnessOffer, ModelChoice};
use model::prompt::{Attachment, Prompt, Switch};
use model::session::{SessionSummary, Switches};

use crate::input::{PastedImage, Submitted, TextInput};
use crate::question_card::QuestionCard;
use crate::terminal_view::TerminalView;
use crate::theme::Theme;
use crate::transcript::{ExchangeView, Folds, Running};

mod attach;
mod composer;
mod driving;
mod earlier;
mod header;
mod keys;
pub(crate) mod plan;
mod rename;
mod seek;
mod switch;
mod terminal;
mod turns;

use rename::Renaming;
use switch::Picker;

gpui::actions!(session, [ToggleTerminal, EscapeComposer, SteerDraft]);

/// What the pane says of a session that runs in a process its node doesn't own.
const ELSEWHERE: &str = "This session is running in another terminal.";
const ELSEWHERE_WHY: &str = "You can read it here. It takes messages here again once that copy ends, so two copies \
     never write to one transcript.";

#[derive(Debug, Clone)]
pub enum PaneEvent {
    Renamed(Box<SessionSummary>),
    /// The session's terminal rang its bell while the window was in the background.
    Bell,
    /// A fork of the conversation started, as this new session.
    Forked(Box<SessionSummary>),
    /// Something asked of the pane's node failed, as said.
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Chat,
    Terminal,
}

pub struct SessionPane {
    hub: Hub,
    summary: SessionSummary,
    mode: Mode,
    terminal: Option<Entity<TerminalView>>,
    confirm_spawn: bool,
    handle: SessionHandle,
    questions: Entity<QuestionCard>,
    turns: ListState,
    exchanges: Arc<Vec<ExchangeView>>,
    folds: Arc<Folds>,
    fetched: Arc<HashMap<String, model::tools::ToolOutput>>,
    previews: Arc<HashMap<String, Arc<gpui::Image>>>,
    requested_previews: HashSet<String>,
    hovered_turn: Option<usize>,
    width: Option<Pixels>,
    composer: Entity<TextInput>,
    offer: Option<HarnessOffer>,
    switches: Switches,
    picked: Switch,
    picking: Option<Picker>,
    attachments: Vec<Attachment>,
    uploading: usize,
    attach_problem: Option<String>,
    renaming: Option<Renaming>,
    /// The exchange a fork is being started after, until the fork opens or fails.
    forking: Option<usize>,
    /// For a fork made here: the conversation it carries from the chat it was forked from, shown to
    /// read above its own, the line that ends it, and that chat.
    carried: Vec<model::chat::Exchange>,
    forked_from: Option<String>,
    fork_source: Option<SessionSummary>,
    /// Whether the session's stream was down when the transcript was last folded.
    running: Running,
    awaiting_title: bool,
    /// The session runs in a process its node doesn't own, so the pane only reads it.
    elsewhere: bool,
    /// When Esc was last pressed in the composer during a turn; a second one soon after stops it.
    escaped: Option<std::time::Instant>,
    /// The time of a search hit's message the transcript is being opened at, until it shows.
    seeking: Option<i64>,
    /// The exchange a search hit opened the transcript at.
    found: Option<usize>,
    _revisions: Task<()>,
    _submitted: Subscription,
    _pasted: Subscription,
    _typed: Subscription,
    _asking: Subscription,
}

impl SessionPane {
    pub fn new(hub: Hub, summary: SessionSummary, cx: &mut Context<Self>) -> Self {
        let handle = hub.open_session(&summary);
        let placeholder = format!("Message {}…", summary.id.harness().display_name());
        let composer = cx.new(|cx| TextInput::new(placeholder, cx).multiline());
        let submitted = cx.subscribe(&composer, |this, _, Submitted(text), cx| {
            this.submit(text.clone(), cx);
        });
        let pasted = cx.subscribe(&composer, |this, _, PastedImage(image), cx| {
            this.attach_pasted(image, cx);
        });
        let typed = cx.observe(&composer, |_, _, cx| cx.notify());
        let mut revisions = handle.revisions();
        let watcher = cx.spawn(async move |this, cx| {
            while revisions.changed().await.is_ok() {
                if this.update(cx, SessionPane::refresh).is_err() {
                    break;
                }
            }
        });
        Self::read_offer(&hub, &summary, cx);
        Self::read_fork(&hub, &summary, cx);
        let questions = cx.new(|cx| QuestionCard::new(handle.clone(), cx));
        let asking = cx.observe(&questions, |_, _, cx| cx.notify());
        let pane = Self {
            hub,
            summary,
            mode: Mode::Chat,
            terminal: None,
            confirm_spawn: false,
            handle,
            questions,
            turns: ListState::new(0, ListAlignment::Bottom, px(600.)),
            exchanges: Arc::new(Vec::new()),
            folds: Arc::new(Folds::default()),
            fetched: Arc::new(HashMap::new()),
            previews: Arc::new(HashMap::new()),
            requested_previews: HashSet::new(),
            hovered_turn: None,
            width: None,
            composer,
            offer: None,
            switches: Switches::default(),
            picked: Switch::default(),
            picking: None,
            attachments: Vec::new(),
            uploading: 0,
            attach_problem: None,
            renaming: None,
            forking: None,
            carried: Vec::new(),
            forked_from: None,
            fork_source: None,
            running: Running::Working,
            awaiting_title: false,
            elsewhere: false,
            escaped: None,
            seeking: None,
            found: None,
            _revisions: watcher,
            _submitted: submitted,
            _pasted: pasted,
            _typed: typed,
            _asking: asking,
        };
        pane.watch_top(cx);
        pane
    }

    /// What the session's harness takes with a prompt: images, and a switch of model or effort
    /// when its node says the running session can change them.
    fn read_offer(hub: &Hub, summary: &SessionSummary, cx: &mut Context<Self>) {
        let hub = hub.clone();
        let summary = summary.clone();
        cx.spawn(async move |this, cx| {
            let offer = hub.offer(&summary).await;
            let described = hub.describe(&summary).await;
            this.update(cx, |this, cx| {
                if let Ok(Some(offer)) = offer {
                    this.offer = Some(offer);
                }
                if let Ok(described) = described {
                    this.switches = described.switches;
                    this.elsewhere = described.running_elsewhere;
                    this.take_listing(&described.summary, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A prompt waits for the images still uploading, so none is sent without what was attached.
    fn submit(&mut self, text: String, cx: &mut Context<Self>) {
        if self.uploading > 0 {
            self.composer
                .update(cx, |composer, cx| composer.set_text(&text, cx));
            return;
        }
        self.attach_problem = None;
        self.picking = None;
        let prompt = Prompt {
            text,
            attachments: std::mem::take(&mut self.attachments),
            switch: self
                .picked
                .against(self.summary.model.as_deref(), self.summary.effort.as_deref()),
        };
        if self.questions.read(cx).waiting() {
            self.handle.answer(prompt);
        } else {
            self.handle.send(prompt);
        }
        cx.notify();
    }

    fn takes_images(&self) -> bool {
        self.offer.as_ref().is_some_and(|offer| offer.takes_images)
    }

    fn models(&self) -> &[ModelChoice] {
        self.offer.as_ref().map_or(&[], |offer| offer.models.as_slice())
    }

    fn steers(&self) -> bool {
        self.offer.as_ref().is_some_and(|offer| offer.steers)
    }

    pub fn session_id(&self) -> &model::SessionId {
        &self.summary.id
    }

    pub fn summary(&self) -> &SessionSummary {
        &self.summary
    }

    /// The workspace's listing has a newer copy of this session, such as the title the harness gave
    /// it or the model and effort a turn switched it to.
    pub fn take_listing(&mut self, latest: &SessionSummary, cx: &mut Context<Self>) {
        if self.renaming.is_none() && latest.title != self.summary.title {
            self.summary.title.clone_from(&latest.title);
            cx.notify();
        }
        if latest.model.is_some() && latest.model != self.summary.model {
            self.summary.model.clone_from(&latest.model);
            cx.notify();
        }
        if latest.effort.is_some() && latest.effort != self.summary.effort {
            self.summary.effort.clone_from(&latest.effort);
            cx.notify();
        }
    }

    pub fn composer(&self) -> &Entity<TextInput> {
        &self.composer
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let (own, running) = {
            let view = self.handle.view();
            let view = view.read();
            let running = if view.lost() {
                Running::Lost
            } else if view.needs_you() {
                Running::Waiting
            } else {
                Running::Working
            };
            (
                exchanges(view.turns(), view.live(), view.busy() || view.driving().busy),
                running,
            )
        };
        let folded: Vec<_> = self.carried.iter().cloned().chain(own).collect();
        let previous = Arc::clone(&self.exchanges);
        let at = self.carried.len();
        let inserted = read_back(&folded, &previous, at);
        if inserted > 0 {
            self.turns.splice(at..at, inserted);
            self.turns.remeasure_items(at + inserted..at + inserted + 1);
            self.folds = Arc::new(self.folds.shifted(at, inserted));
            self.forking = self
                .forking
                .map(|index| if index < at { index } else { index + inserted });
            self.found = self
                .found
                .map(|index| if index < at { index } else { index + inserted });
            self.hovered_turn = None;
        }
        let before = |index: usize| match index {
            index if index < at => previous.get(index),
            index if index < at + inserted => None,
            index => previous.get(index - inserted),
        };
        let turn_ended = previous.last().map(|view| view.exchange.state) == Some(ExchangeState::Working)
            && folded
                .last()
                .is_some_and(|exchange| exchange.state != ExchangeState::Working);
        if turn_ended && self.summary.title.is_none() {
            self.await_title(cx);
        }
        let jump = self.hub.preferences().jump_to_newest
            && arrived(
                previous.len() + inserted,
                previous.last().map(|view| view.exchange.state),
                &folded,
            );
        let views: Vec<ExchangeView> = folded
            .into_iter()
            .enumerate()
            .map(|(index, exchange)| match before(index) {
                Some(old) if old.exchange == exchange => old.clone(),
                Some(old) => ExchangeView::advance(old, exchange),
                None => ExchangeView::new(exchange),
            })
            .collect();
        let count = views.len();
        let known = self.turns.item_count();
        let mut first_changed = views
            .iter()
            .enumerate()
            .filter(|(index, _)| !(at..at + inserted).contains(index))
            .find(|(index, new)| before(*index).is_none_or(|old| old != *new))
            .map_or(known.min(count), |(index, _)| index);
        // A running exchange shows whether its stream is down or it waits on you, so the newest
        // one draws again.
        if running != self.running {
            self.running = running;
            first_changed = first_changed.min(count.saturating_sub(1));
        }
        if count < known {
            self.turns.reset(count);
        } else {
            let stale = first_changed.min(known);
            self.turns.splice(stale..known, count - stale);
        }
        if jump {
            self.turns.scroll_to_end();
        }
        self.exchanges = Arc::new(views);
        self.fetch_previews(cx);
        self.seek_step(cx);
        cx.notify();
    }

    /// Harnesses title a session shortly after a turn ends, and nothing announces it, so an
    /// untitled session asks its node a few times.
    fn await_title(&mut self, cx: &mut Context<Self>) {
        if self.awaiting_title {
            return;
        }
        self.awaiting_title = true;
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        cx.spawn(async move |this, cx| {
            for wait in [2, 5, 10, 20] {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(wait))
                    .await;
                let Ok(latest) = hub.describe(&summary).await.map(|described| described.summary) else {
                    continue;
                };
                if latest.title.is_some() {
                    this.update(cx, |this, cx| {
                        this.take_listing(&latest, cx);
                        cx.emit(PaneEvent::Renamed(Box::new(this.summary.clone())));
                    })
                    .ok();
                    break;
                }
            }
            this.update(cx, |this, _| this.awaiting_title = false).ok();
        })
        .detach();
    }
}

/// How many exchanges a page read back put in front of the ones `previous` showed from `at` on;
/// none when the newest changed the way a turn does.
fn read_back(folded: &[model::chat::Exchange], previous: &[ExchangeView], at: usize) -> usize {
    let Some(first) = previous.get(at) else {
        return 0;
    };
    let grown = folded.len().saturating_sub(previous.len());
    (0..=grown)
        .find(|shift| folded.get(at + shift) == Some(&first.exchange))
        .unwrap_or(0)
}

/// The transcript column: 64% of the pane, kept between 680 and 920px, and never wider than the pane allows.
fn column_width(body: Pixels) -> Pixels {
    (body * 0.64)
        .clamp(px(680.), px(920.))
        .min(body - px(48.))
        .max(Pixels::ZERO)
}

impl gpui::EventEmitter<PaneEvent> for SessionPane {}

impl Render for SessionPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body_width = self.width.unwrap_or_else(|| window.viewport_size().width);
        let column = column_width(body_width);
        let pane = cx.entity().downgrade();
        let measure = gpui::canvas(
            move |bounds, _, cx| {
                pane.update(cx, |this, cx| {
                    if this.width != Some(bounds.size.width) {
                        this.width = Some(bounds.size.width);
                        cx.notify();
                    }
                })
                .ok();
            },
            |_, (), _, _| {},
        )
        .absolute()
        .size_full();
        let behind = self.turns.is_scrolled_to_end() == Some(false);
        let footer = {
            let shared = self.handle.view();
            let view = shared.read();
            div().w(column).pb_5().pt_2().flex().flex_col().gap_3().children(
                view.error()
                    .map(|message| div().text_color(Theme::warn()).child(message.to_owned())),
            )
        };
        let header = {
            let shared = self.handle.view();
            let view = shared.read();
            self.header(&view, cx)
        };
        let prompt = self.spawn_prompt(cx);
        let body = match (self.mode, self.live_terminal(cx)) {
            (Mode::Terminal, None) => div().flex_1().w_full().child(prompt),
            (Mode::Terminal, Some(terminal)) => div()
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .px_4()
                .child(div().flex_1().w_full().child(terminal))
                .child(footer),
            _ => self.chat_body(column, behind, footer, body_width > px(900.), cx),
        };
        div()
            .key_context("Session")
            .on_action(cx.listener(Self::toggle))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .child(measure)
            .child(header)
            .child(body)
    }
}
