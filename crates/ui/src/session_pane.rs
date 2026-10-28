use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use app::{Hub, SessionHandle};
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, ExternalPaths, Focusable, ImageFormat, IntoElement,
    ListAlignment, ListState, ParentElement, PathPromptOptions, Pixels, Render, Styled, Subscription, Task,
    Window, div, list, prelude::*, px, relative,
};
use model::chat::exchanges;
use model::prompt::{Attachment, Prompt, image_media_type};
use model::session::{Decision, SessionSummary, SessionView};
use term::GridSize;

use crate::input::{PastedImage, Submitted, TextInput};
use crate::terminal_view::TerminalView;
use crate::theme::Theme;
use crate::transcript::{self, ExchangeView, Fold, Folds};

gpui::actions!(session, [ToggleTerminal]);

const IMAGES_ONLY: &str = "Only PNG, JPEG, GIF and WebP images can be sent.";

const FIRST_TERMINAL: GridSize = GridSize {
    columns: 120,
    rows: 40,
};

#[derive(Debug, Clone)]
pub enum PaneEvent {
    Renamed(SessionSummary),
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
    attachments: Vec<Attachment>,
    uploading: usize,
    attach_problem: Option<String>,
    renaming: Option<Renaming>,
    _revisions: Task<()>,
    _submitted: Subscription,
    _pasted: Subscription,
    _typed: Subscription,
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
        Self {
            hub,
            summary,
            mode: Mode::Chat,
            terminal: None,
            confirm_spawn: false,
            handle,
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
            attachments: Vec::new(),
            uploading: 0,
            attach_problem: None,
            renaming: None,
            _revisions: watcher,
            _submitted: submitted,
            _pasted: pasted,
            _typed: typed,
        }
    }

    fn read_offer(hub: &Hub, summary: &SessionSummary, cx: &mut Context<Self>) {
        let hub = hub.clone();
        let summary = summary.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Some(offer)) = hub.offer(&summary).await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.takes_images = offer.takes_images;
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
        self.handle.send(Prompt {
            text,
            attachments: std::mem::take(&mut self.attachments),
        });
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

    /// The workspace's listing has a newer copy of this session, such as the title the harness gave it.
    pub fn take_listing(&mut self, latest: &SessionSummary, cx: &mut Context<Self>) {
        if self.renaming.is_none() && latest.title != self.summary.title {
            self.summary.title.clone_from(&latest.title);
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
        let submitted = cx.subscribe(&input, |this, _, Submitted(title), cx| {
            this.commit_rename(title.clone(), cx);
        });
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            input,
            problem: None,
            _submitted: submitted,
        });
        cx.notify();
    }

    fn commit_rename(&mut self, title: String, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let summary = self.summary.clone();
        cx.spawn(async move |this, cx| {
            let renamed = hub.rename(&summary, title).await;
            this.update(cx, |this, cx| {
                match renamed {
                    Ok(renamed) => {
                        this.summary.title = renamed.title;
                        this.renaming = None;
                        cx.emit(PaneEvent::Renamed(this.summary.clone()));
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
            cx.new(|cx| TerminalView::new(handle, cx))
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
        let meta = [self.summary.model.as_deref(), self.summary.cwd.as_deref()]
            .into_iter()
            .flatten()
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
            exchanges(view.turns(), view.live())
        };
        let previous = Arc::clone(&self.exchanges);
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
        self.exchanges = Arc::new(views);
        self.fetch_previews(cx);
        cx.notify();
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
                footer.children(self.queue_dock()).child(
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
        let index = match fold {
            Fold::Exchange(index)
            | Fold::Group(index, _)
            | Fold::Thought(index, _)
            | Fold::Tool(index, _, _) => index,
        };
        if index < self.turns.item_count() {
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

    fn queue_dock(&self) -> Option<AnyElement> {
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
                    .child(div().flex_1())
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
