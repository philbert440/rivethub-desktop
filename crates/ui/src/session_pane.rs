use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use app::{Hub, SessionHandle};
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, ExternalPaths, Focusable, ImageFormat, IntoElement,
    ListAlignment, ListState, ParentElement, PathPromptOptions, Pixels, Render, Styled, Subscription, Task,
    Window, div, list, prelude::*, px, relative,
};
use model::chat::{ExchangeState, arrived, exchanges, totals};
use model::harness::ModelChoice;
use model::prompt::{Attachment, Prompt, Switch, image_media_type};
use model::session::{Decision, SessionSummary, SessionView, Switches};
use model::usage::tokens;
use term::GridSize;

use crate::input::{PastedImage, Submitted, TextInput};
use crate::question_card::QuestionCard;
use crate::terminal_view::{Bell, TerminalView};
use crate::theme::Theme;
use crate::transcript::{self, ExchangeView, Fold, Folds};

pub(crate) mod plan;
mod switch;

use switch::Picker;

gpui::actions!(session, [ToggleTerminal]);

const IMAGES_ONLY: &str = "Only PNG, JPEG, GIF and WebP images can be sent.";

const FIRST_TERMINAL: GridSize = GridSize {
    columns: 120,
    rows: 40,
};

#[derive(Debug, Clone)]
pub enum PaneEvent {
    Renamed(Box<SessionSummary>),
    /// The session's terminal rang its bell while the window was in the background.
    Bell,
}

struct Renaming {
    input: Entity<TextInput>,
    problem: Option<String>,
    _submitted: Subscription,
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
    takes_images: bool,
    switches: Switches,
    models: Vec<ModelChoice>,
    picked: Switch,
    picking: Option<Picker>,
    attachments: Vec<Attachment>,
    uploading: usize,
    attach_problem: Option<String>,
    renaming: Option<Renaming>,
    awaiting_title: bool,
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
        let questions = cx.new(|cx| QuestionCard::new(handle.clone(), cx));
        let asking = cx.observe(&questions, |_, _, cx| cx.notify());
        Self {
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
            takes_images: false,
            switches: Switches::default(),
            models: Vec::new(),
            picked: Switch::default(),
            picking: None,
            attachments: Vec::new(),
            uploading: 0,
            attach_problem: None,
            renaming: None,
            awaiting_title: false,
            _revisions: watcher,
            _submitted: submitted,
            _pasted: pasted,
            _typed: typed,
            _asking: asking,
        }
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
                    this.takes_images = offer.takes_images;
                    this.models = offer.models;
                }
                if let Ok(described) = described {
                    this.switches = described.switches;
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

    fn attach(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach images".into()),
        });
        self.attach_problem = None;
        cx.spawn(async move |this, cx| {
            let paths = match picked.await {
                Ok(Ok(Some(paths))) => paths,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    this.update(cx, |this, cx| {
                        this.attach_problem = Some(format!("couldn't open the file picker: {err}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            this.update(cx, |this, cx| this.attach_paths(paths, cx)).ok();
        })
        .detach();
    }

    /// Clears the last attachment problem, or says why nothing can be attached here.
    fn accepts_attachments(&mut self, cx: &mut Context<Self>) -> bool {
        self.attach_problem = (!self.takes_images).then(|| {
            format!(
                "{} doesn't take attachments in this session",
                self.summary.id.harness().display_name()
            )
        });
        cx.notify();
        self.takes_images
    }

    /// Files dropped on the chat attach when rh takes them, which today means images only.
    fn attach_dropped(&mut self, dropped: &ExternalPaths, cx: &mut Context<Self>) {
        if !self.accepts_attachments(cx) {
            return;
        }
        let (images, refused): (Vec<PathBuf>, Vec<PathBuf>) =
            dropped.paths().iter().cloned().partition(|path| {
                path.file_name()
                    .and_then(|name| image_media_type(&name.to_string_lossy()))
                    .is_some()
            });
        if !refused.is_empty() {
            let names = refused
                .iter()
                .map(|path| path.file_name().unwrap_or(path.as_os_str()).to_string_lossy())
                .collect::<Vec<_>>()
                .join(", ");
            self.attach_problem = Some(format!("Not attached: {names}. {IMAGES_ONLY}"));
        }
        self.attach_paths(images, cx);
        cx.notify();
    }

    fn attach_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let hub = self.hub.clone();
            let summary = self.summary.clone();
            self.track_upload(async move { hub.upload(&summary, path).await }, cx);
        }
    }

    fn attach_pasted(&mut self, image: &gpui::Image, cx: &mut Context<Self>) {
        if !self.accepts_attachments(cx) {
            return;
        }
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        let name = format!("pasted-image.{}", image.format.extension());
        if image_media_type(&name).is_none() {
            self.attach_problem = Some(IMAGES_ONLY.to_owned());
            cx.notify();
            return;
        }
        let bytes = image.bytes.clone();
        self.track_upload(async move { hub.upload_bytes(&summary, name, bytes).await }, cx);
    }

    /// Counts an upload in flight, so the prompt waits for it, and adds what it uploaded to the next prompt.
    fn track_upload<E: std::fmt::Display>(
        &mut self,
        upload: impl Future<Output = Result<Attachment, E>> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.uploading += 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let uploaded = upload.await;
            this.update(cx, |this, cx| {
                this.uploading = this.uploading.saturating_sub(1);
                match uploaded {
                    Ok(attachment) => this.attachments.push(attachment),
                    Err(err) => this.attach_problem = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Fetches the images sent earlier in the session that are still held on the node, once each.
    fn fetch_previews(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<(String, ImageFormat)> = self
            .exchanges
            .iter()
            .flat_map(|view| &view.exchange.files)
            .filter_map(|file| {
                let upload = file
                    .viewable_upload()
                    .filter(|upload| !self.requested_previews.contains(*upload))?;
                Some((upload.to_owned(), ImageFormat::from_mime_type(&file.media_type)?))
            })
            .collect();
        for (upload, format) in wanted {
            if !self.requested_previews.insert(upload.clone()) {
                continue;
            }
            let hub = self.hub.clone();
            let summary = self.summary.clone();
            cx.spawn(async move |this, cx| {
                let fetched = hub.download(&summary, upload.clone()).await;
                let bytes = match fetched {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        tracing::debug!(%err, upload, "a sent image is no longer available");
                        return;
                    }
                };
                this.update(cx, |this, cx| {
                    let image = Arc::new(gpui::Image::from_bytes(format, bytes));
                    Arc::make_mut(&mut this.previews).insert(upload.clone(), image);
                    let showing = this.exchanges.iter().enumerate().filter(|(_, view)| {
                        view.exchange
                            .files
                            .iter()
                            .any(|file| file.viewable_upload() == Some(upload.as_str()))
                    });
                    let items = this.turns.item_count();
                    for (index, _) in showing.filter(|(index, _)| *index < items) {
                        this.turns.splice(index..index + 1, 1);
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    pub fn session_id(&self) -> &model::SessionId {
        &self.summary.id
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

    fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.summary.title.clone().unwrap_or_default();
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Session title", cx);
            input.set_text(current.trim(), cx);
            input
        });
        let submitted = cx.subscribe_in(&input, window, |this, _, Submitted(title), window, cx| {
            this.commit_rename(title.clone(), window, cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            input,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    /// A finished rename hands the keyboard back to the composer, as cancelling does.
    fn commit_rename(&mut self, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let renamed = hub.rename(&summary, title).await;
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| {
                    match renamed {
                        Ok(renamed) => {
                            this.summary.title = renamed.title;
                            this.renaming = None;
                            window.focus(&this.composer.focus_handle(cx), cx);
                            cx.emit(PaneEvent::Renamed(Box::new(this.summary.clone())));
                        }
                        Err(err) => {
                            if let Some(renaming) = this.renaming.as_mut() {
                                renaming.problem = Some(format!("rename failed: {err}"));
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .ok();
        })
        .detach();
    }

    fn title(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(renaming) = &self.renaming else {
            return div()
                .id("session-title")
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .text_color(Theme::ink())
                .cursor_text()
                .hover(|title| title.text_color(Theme::accent()))
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.begin_rename(window, cx)))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .truncate()
                        .child(crate::text::one_line(self.summary.title.as_deref())),
                )
                .child(crate::brand::icon("icons/rename.svg", 14., Theme::ink_dim()))
                .into_any_element();
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(px(360.))
                    .px_3()
                    .py_1()
                    .rounded(px(10.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::accent())
                    .child(renaming.input.clone()),
            )
            .child(
                div()
                    .id("cancel-rename")
                    .cursor_pointer()
                    .text_color(Theme::ink_dim())
                    .hover(|cancel| cancel.text_color(Theme::ink()))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.renaming = None;
                        window.focus(&this.composer.focus_handle(cx), cx);
                        cx.notify();
                    }))
                    .child("×"),
            )
            .children(
                renaming
                    .problem
                    .clone()
                    .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
            )
            .into_any_element()
    }

    pub fn composer(&self) -> &Entity<TextInput> {
        &self.composer
    }

    fn show(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        match (mode, &self.terminal) {
            (Mode::Chat, _) => window.focus(&self.composer.focus_handle(cx), cx),
            (Mode::Terminal, Some(terminal)) => window.focus(&terminal.focus_handle(cx), cx),
            (Mode::Terminal, None) => self.check_terminal(window, cx),
        }
        cx.notify();
    }

    fn check_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let running = hub.has_terminal(&summary).await.unwrap_or(false);
            cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |this, cx| {
                    if this.mode != Mode::Terminal {
                        return;
                    }
                    if running {
                        this.attach_terminal(window, cx);
                    } else {
                        this.confirm_spawn = true;
                        cx.notify();
                    }
                })
                .ok();
            })
            .ok();
        })
        .detach();
    }

    fn attach_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_spawn = false;
        let terminal = self.terminal.get_or_insert_with(|| {
            let handle = self.hub.open_terminal(&self.summary, FIRST_TERMINAL);
            let terminal = cx.new(|cx| TerminalView::new(handle, window, cx));
            cx.subscribe(&terminal, |_, _, Bell, cx| cx.emit(PaneEvent::Bell))
                .detach();
            terminal
        });
        window.focus(&terminal.focus_handle(cx), cx);
        cx.notify();
    }

    fn spawn_prompt(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .when(self.confirm_spawn, |prompt| {
                prompt
                    .child(div().text_color(Theme::ink()).child("No terminal is running for this session."))
                    .child(div().max_w(px(520.)).text_color(Theme::ink_dim()).child(
                        "Opening one starts the harness for this session in a terminal. If the session is \
                         running anywhere else, including this chat, both copies write to the same transcript.",
                    ))
                    .child(
                        div()
                            .id("open-terminal")
                            .h(px(34.))
                            .px_4()
                            .rounded(px(17.))
                            .bg(Theme::accent())
                            .text_color(Theme::on_accent())
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.attach_terminal(window, cx)))
                            .child("Open terminal"),
                    )
            })
            .into_any_element()
    }

    fn end_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(terminal) = self.terminal.take() {
            terminal.read(cx).end();
        }
        self.show(Mode::Chat, window, cx);
    }

    fn toggle(&mut self, _: &ToggleTerminal, window: &mut Window, cx: &mut Context<Self>) {
        let next = if self.mode == Mode::Chat {
            Mode::Terminal
        } else {
            Mode::Chat
        };
        self.show(next, window, cx);
    }

    fn tab(&self, mode: Mode, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.mode == mode;
        div()
            .id(label)
            .h(px(30.))
            .px_4()
            .rounded(px(15.))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(active, |tab| {
                tab.bg(Theme::selected()).text_color(Theme::on_selected())
            })
            .when(!active, |tab| tab.text_color(Theme::ink_dim()))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.show(mode, window, cx)))
            .child(label)
    }

    fn header(&self, view: &SessionView, cx: &mut Context<Self>) -> gpui::Div {
        let meta = self
            .summary
            .model
            .as_deref()
            .map(|model| self.model_label(model))
            .into_iter()
            .chain(self.summary.cwd.clone())
            .collect::<Vec<_>>()
            .join(" · ");
        div()
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .pt_4()
            .pb_2()
            .child(self.title(cx))
            .child(
                div()
                    .text_xs()
                    .text_color(Theme::ink_dim())
                    .truncate()
                    .child(meta),
            )
            .child(div().flex_1())
            .children(Self::context_meter(view))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .p(px(3.))
                    .rounded(px(18.))
                    .bg(Theme::card())
                    .child(self.tab(Mode::Chat, "Chat", cx))
                    .child(self.tab(Mode::Terminal, "Terminal", cx)),
            )
            .when(self.mode == Mode::Terminal && self.terminal.is_some(), |header| {
                header.child(
                    div()
                        .id("end-terminal")
                        .h(px(30.))
                        .px_3()
                        .rounded(px(15.))
                        .border_1()
                        .border_color(Theme::line())
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_color(Theme::ink_dim())
                        .hover(|button| button.text_color(Theme::ink()))
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, window, cx| this.end_terminal(window, cx)),
                        )
                        .child("End"),
                )
            })
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let folded = {
            let view = self.handle.view();
            let view = view.read();
            exchanges(view.turns(), view.live(), view.busy())
        };
        let previous = Arc::clone(&self.exchanges);
        let turn_ended = previous.last().map(|view| view.exchange.state) == Some(ExchangeState::Working)
            && folded
                .last()
                .is_some_and(|exchange| exchange.state != ExchangeState::Working);
        if turn_ended && self.summary.title.is_none() {
            self.await_title(cx);
        }
        let jump = self.hub.preferences().jump_to_newest
            && arrived(
                previous.len(),
                previous.last().map(|view| view.exchange.state),
                &folded,
            );
        let views: Vec<ExchangeView> = folded
            .into_iter()
            .enumerate()
            .map(|(index, exchange)| match previous.get(index) {
                Some(old) if old.exchange == exchange => old.clone(),
                Some(old) => ExchangeView::advance(old, exchange),
                None => ExchangeView::new(exchange),
            })
            .collect();
        let count = views.len();
        let known = self.turns.item_count();
        let first_changed = views
            .iter()
            .zip(previous.iter())
            .position(|(new, old)| new != old)
            .unwrap_or(known.min(count));
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

    pub fn hover_turn(&mut self, turn: Option<usize>, cx: &mut Context<Self>) {
        if self.hovered_turn != turn {
            self.hovered_turn = turn;
            cx.notify();
        }
    }

    pub fn reveal_turn(&mut self, index: usize, cx: &mut Context<Self>) {
        self.turns.scroll_to_reveal_item(index);
        cx.notify();
    }

    fn active_turn(&self) -> usize {
        let last = self.exchanges.len().saturating_sub(1);
        if self.turns.is_scrolled_to_end() == Some(false) {
            self.turns.logical_scroll_top().item_ix.min(last)
        } else {
            last
        }
    }

    fn chat_body(
        &mut self,
        column: Pixels,
        behind: bool,
        footer: gpui::Div,
        wide: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let shared = self.handle.view();
        let view = shared.read();
        let exchanges = Arc::clone(&self.exchanges);
        let folds = Arc::clone(&self.folds);
        let fetched = Arc::clone(&self.fetched);
        let previews = Arc::clone(&self.previews);
        let pane = cx.entity().downgrade();
        let cwd = self.summary.cwd.clone();
        let home = std::env::var("HOME").ok();
        let transcript = list(
            self.turns.clone(),
            move |index, window: &mut Window, _: &mut App| {
                exchanges.get(index).map_or_else(
                    || div().into_any_element(),
                    |exchange| {
                        let context = transcript::Context {
                            index,
                            folds: &folds,
                            column,
                            pane: pane.clone(),
                            cwd: cwd.as_deref(),
                            home: home.as_deref(),
                            fetched: &fetched,
                            previews: &previews,
                        };
                        transcript::exchange(exchange, &context, window)
                    },
                )
            },
        )
        .size_full();
        let back = behind.then(|| {
            div().w(column).flex().justify_end().child(
                div()
                    .id("back-to-bottom")
                    .size(px(34.))
                    .rounded(px(17.))
                    .bg(Theme::raised())
                    .text_color(Theme::ink())
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.turns.scroll_to_end();
                        cx.notify();
                    }))
                    .child("↓"),
            )
        });
        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .border_2()
            .border_color(gpui::transparent_black())
            .drag_over::<ExternalPaths>(|style, _, _, _| style.border_color(Theme::accent()))
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| this.attach_dropped(dropped, cx)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .pt_2()
                    .child(transcript)
                    .children(
                        wide.then(|| {
                            let rail = crate::turn_rail::Rail {
                                exchanges: &self.exchanges,
                                active: self.active_turn(),
                                hovered: self.hovered_turn,
                            };
                            crate::turn_rail::rail(&rail, cx)
                        })
                        .flatten(),
                    )
                    .child(Self::fade()),
            )
            .children(back)
            .child(
                footer
                    .children(self.questions.read(cx).waiting().then(|| self.questions.clone()))
                    .children(self.plan_panel(&view, cx))
                    .children(self.queue_dock(cx))
                    .child(
                        self.approval_card(&view)
                            .unwrap_or_else(|| self.composer_card(&view, cx)),
                    ),
            )
    }

    fn fade() -> impl IntoElement {
        let mut clear: gpui::Hsla = Theme::surface().into();
        clear.a = 0.;
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(36.))
            .bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(clear, 0.),
                gpui::linear_color_stop(Theme::surface(), 1.),
            ))
    }

    fn tool_at(&self, exchange: usize, step: usize, tool: usize) -> Option<&model::session::ToolCall> {
        match self.exchanges.get(exchange)?.exchange.steps.get(step)? {
            model::chat::Step::Tools(tools) => tools.get(tool),
            _ => None,
        }
    }

    fn fetch_output(&mut self, exchange: usize, step: usize, tool: usize, cx: &mut Context<Self>) {
        let Some(call) = self
            .tool_at(exchange, step, tool)
            .filter(|call| crate::tool_view::fetchable(call))
        else {
            return;
        };
        let (Some(id), name) = (call.id.clone(), call.name.clone()) else {
            return;
        };
        if self.fetched.contains_key(&id) {
            return;
        }
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        cx.spawn(async move |this, cx| {
            let fetched = hub.tool_output(&summary, id.clone(), name).await;
            this.update(cx, |this, cx| {
                if let Ok(Some(output)) = fetched {
                    Arc::make_mut(&mut this.fetched).insert(id, output);
                    if exchange < this.turns.item_count() {
                        this.turns.splice(exchange..exchange + 1, 1);
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn toggle_fold(&mut self, fold: Fold, cx: &mut Context<Self>) {
        Arc::make_mut(&mut self.folds).toggle(fold);
        if let Fold::Tool(exchange, step, tool) = fold {
            self.fetch_output(exchange, step, tool, cx);
        }
        let row = match fold {
            Fold::Exchange(index)
            | Fold::Group(index, _)
            | Fold::Thought(index, _)
            | Fold::Tool(index, _, _) => Some(index),
            Fold::Plan => None,
        };
        if let Some(index) = row.filter(|index| *index < self.turns.item_count()) {
            self.turns.splice(index..index + 1, 1);
        }
        cx.notify();
    }

    fn context_meter(view: &SessionView) -> Option<gpui::Div> {
        let usage = view.context()?;
        let fill = if usage.nearly_full() {
            Theme::warn()
        } else {
            Theme::accent()
        };
        Some(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(
                    div()
                        .w(px(64.))
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(Theme::card())
                        .child(
                            div()
                                .h_full()
                                .w(relative(usage.fraction()))
                                .rounded(px(3.))
                                .bg(fill),
                        ),
                )
                .child(format!("{}k / {}k", usage.used / 1000, usage.window / 1000)),
        )
    }

    fn decision_button(
        &self,
        label: &'static str,
        decision: Decision,
        primary: bool,
        request_id: &str,
    ) -> impl IntoElement {
        let handle = self.handle.clone();
        let request_id = request_id.to_owned();
        div()
            .id(label)
            .h(px(34.))
            .px_4()
            .rounded(px(17.))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(primary, |button| {
                button.bg(Theme::accent()).text_color(Theme::on_accent())
            })
            .when(!primary, |button| {
                button
                    .border_1()
                    .border_color(Theme::line())
                    .text_color(Theme::ink())
            })
            .on_click(move |_, _, _| handle.decide(request_id.clone(), decision))
            .child(label)
    }

    fn approval_card(&self, view: &SessionView) -> Option<AnyElement> {
        let approval = view.approvals().first()?;
        let request_id = approval.request_id.clone();
        Some(
            div()
                .w_full()
                .p_5()
                .rounded(px(28.))
                .bg(Theme::warn_container())
                .border_1()
                .border_color(Theme::warn_line())
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_xs()
                        .text_color(Theme::warn())
                        .child("Waiting for approval"),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .text_color(Theme::ink())
                        .child(approval.tool.clone()),
                )
                .children(approval.reason.clone().map(|reason| {
                    div()
                        .font_family(Theme::MONO)
                        .text_size(px(12.))
                        .text_color(Theme::ink_dim())
                        .child(reason)
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(self.decision_button("Reject", Decision::Deny, false, &request_id))
                        .child(self.decision_button(
                            "Allow for session",
                            Decision::AllowForSession,
                            false,
                            &request_id,
                        ))
                        .child(self.decision_button("Allow once", Decision::Allow, true, &request_id)),
                )
                .into_any_element(),
        )
    }

    /// Puts a queued prompt back in the composer, after any draft, with its images.
    fn edit_queued(&mut self, queued: &Prompt, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.handle.take_queued(queued) else {
            return;
        };
        self.composer.update(cx, |composer, cx| {
            let draft = composer.text().trim_end().to_owned();
            let text = if draft.is_empty() {
                prompt.text.clone()
            } else {
                format!("{draft}\n{}", prompt.text)
            };
            composer.set_text(&text, cx);
        });
        self.attachments.extend(prompt.attachments);
        window.focus(&self.composer.focus_handle(cx), cx);
        cx.notify();
    }

    fn queue_dock(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let queued = self.handle.queued();
        if queued.is_empty() {
            return None;
        }
        Some(
            div()
                .w_full()
                .px_4()
                .py_2()
                .rounded(px(18.))
                .bg(Theme::panel())
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(Theme::ink_dim()).child(format!(
                    "{} queued {}",
                    queued.len(),
                    if queued.len() == 1 { "message" } else { "messages" }
                )))
                .children(queued.into_iter().enumerate().map(|(index, prompt)| {
                    let handle = self.handle.clone();
                    let queued = prompt.clone();
                    let images = match prompt.attachments.len() {
                        0 => None,
                        1 => Some("1 image".to_owned()),
                        count => Some(format!("{count} images")),
                    };
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_color(Theme::ink())
                                .child(crate::text::one_line(Some(&prompt.text))),
                        )
                        .children(
                            images.map(|images| div().text_xs().text_color(Theme::ink_dim()).child(images)),
                        )
                        .child(
                            div()
                                .id(("edit-queued", index))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    this.edit_queued(&queued, window, cx);
                                }))
                                .child(crate::brand::icon("icons/rename.svg", 13., Theme::ink_dim())),
                        )
                        .child(
                            div()
                                .id(("unqueue", index))
                                .cursor_pointer()
                                .text_color(Theme::ink_dim())
                                .on_click(move |_, _, _| handle.unqueue(index))
                                .child("×"),
                        )
                }))
                .into_any_element(),
        )
    }

    /// The session so far, beside Send: how fast the replies streamed here came, what the session
    /// wrote, and how much of what it read came from cache.
    fn session_stats(&self, view: &SessionView) -> Option<impl IntoElement> {
        let usage = totals(self.exchanges.iter().map(|view| &view.exchange));
        let parts: Vec<String> = view
            .pace()
            .tokens_per_second()
            .map(|rate| format!("{rate} tok/s"))
            .into_iter()
            .chain((usage.output > 0).then(|| format!("{} out", tokens(usage.output))))
            .chain(
                usage
                    .cached_percent()
                    .filter(|share| *share > 0)
                    .map(|share| format!("{share}% cached")),
            )
            .collect();
        (!parts.is_empty()).then(|| {
            div()
                .flex_none()
                .px_3()
                .text_xs()
                .text_color(Theme::ink_dim())
                .child(parts.join(" · "))
        })
    }

    fn send_button(&self, busy: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = !self.composer.read(cx).is_empty();
        let stop = busy && !draft;
        let enabled = stop || (draft && self.uploading == 0);
        let button = div()
            .id("send")
            .h(px(34.))
            .min_w(px(34.))
            .rounded(px(17.))
            .flex()
            .items_center()
            .justify_center()
            .bg(Theme::accent())
            .text_color(Theme::on_accent())
            .when(!enabled, |button| button.opacity(0.4))
            .when(enabled, gpui::Styled::cursor_pointer)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if stop {
                    this.handle.stop();
                } else if let Some(text) = this.composer.update(cx, TextInput::take) {
                    this.submit(text, cx);
                }
            }));
        match (stop, busy) {
            (true, _) => button.child(crate::brand::icon("icons/square.svg", 14., Theme::on_accent())),
            (false, true) => button.px_3().child("Queue"),
            (false, false) => button.child(crate::brand::icon("icons/arrow-up.svg", 18., Theme::on_accent())),
        }
    }

    fn attach_button(cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("attach")
            .size(px(34.))
            .rounded(px(17.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(|button| button.bg(Theme::raised()))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.attach(cx)))
            .child(crate::brand::icon("icons/plus.svg", 18., Theme::ink_dim()))
    }

    fn attachment_row(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.attachments.is_empty() && self.uploading == 0 && self.attach_problem.is_none() {
            return None;
        }
        let chip = || {
            div()
                .h(px(28.))
                .px_3()
                .rounded(px(14.))
                .bg(Theme::raised())
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(Theme::ink())
        };
        Some(
            div()
                .px_4()
                .pt_3()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .children(self.attachments.iter().enumerate().map(|(index, attachment)| {
                    chip()
                        .child(div().max_w(px(220.)).truncate().child(attachment.name.clone()))
                        .child(
                            div()
                                .id(("detach", index))
                                .cursor_pointer()
                                .text_color(Theme::ink_dim())
                                .hover(|remove| remove.text_color(Theme::ink()))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if index < this.attachments.len() {
                                        this.attachments.remove(index);
                                    }
                                    cx.notify();
                                }))
                                .child("×"),
                        )
                }))
                .when(self.uploading > 0, |row| {
                    row.child(
                        chip()
                            .text_color(Theme::ink_dim())
                            .child(format!("Uploading {}…", self.uploading)),
                    )
                })
                .children(
                    self.attach_problem
                        .clone()
                        .map(|problem| div().text_xs().text_color(Theme::warn()).child(problem)),
                ),
        )
    }

    fn composer_card(&self, view: &SessionView, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .rounded(px(28.))
            .bg(Theme::card())
            .flex()
            .flex_col()
            .children(self.attachment_row(cx))
            .children(self.picker_row(cx))
            .child(
                div()
                    .min_h(px(44.))
                    .pl_5()
                    .pr_4()
                    .pt_3()
                    .child(self.composer.clone()),
            )
            .child(
                div()
                    .h(px(46.))
                    .px_2()
                    .pb_2()
                    .flex()
                    .items_center()
                    .when(self.takes_images, |row| row.child(Self::attach_button(cx)))
                    .children(self.switch_chips(cx))
                    .child(div().flex_1())
                    .children(self.session_stats(view))
                    .child(self.send_button(view.busy(), cx)),
            )
            .into_any_element()
    }
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
        let body = match (self.mode, &self.terminal) {
            (Mode::Terminal, None) => div().flex_1().w_full().child(prompt),
            (Mode::Terminal, Some(terminal)) => div()
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .px_4()
                .child(div().flex_1().w_full().child(terminal.clone()))
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
