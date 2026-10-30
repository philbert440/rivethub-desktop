//! A plug-in's web panel (CONTRACT note 60): its own page, painted offscreen by `webview` and
//! drawn here frame by frame, with the app's pointer, keys, input method, clipboard and theme
//! handed to it. What the page asks for runs as the plug-in's buttons would.

mod input;

use std::sync::Arc;

use app::Hub;
use futures::StreamExt;
use gpui::{
    Bounds, ClipboardItem, Context, CursorStyle, EventEmitter, FocusHandle, IntoElement, ParentElement,
    Pixels, Render, RenderImage, Styled, Subscription, Task, Window, canvas, div, img, prelude::*, px,
};
use model::plugin::panel::{HostMessage, Message, Request, Theme as PanelTheme};
use model::plugin::view::{Action, WebPanelBlock};
use model::plugin::{Context as About, PluginRef};
use webview::{Fetch, Fetched, Menu, Panel, Size, Update};

use crate::theme::Theme;

/// A panel's height when neither its block nor its page says.
const HEIGHT: f32 = 320.;
/// The heights a page may ask for.
const HEIGHTS: (f32, f32) = (48., 2400.);

/// The page asked for these actions.
pub struct Asked(pub Vec<Action>);

pub struct WebPanelView {
    plugin: PluginRef,
    block: WebPanelBlock,
    context: About,
    panel: Option<Panel>,
    problem: Option<String>,
    frame: Option<(Arc<RenderImage>, gpui::Size<Pixels>)>,
    /// Frames replaced since the last draw, whose textures the window lets go of.
    retired: Vec<Arc<RenderImage>>,
    height: f32,
    size: Option<Size>,
    bounds: Option<Bounds<Pixels>>,
    cursor: CursorStyle,
    menu: Option<Menu>,
    focus: FocusHandle,
    /// The theme the page was given last, once it has loaded.
    theme: Option<PanelTheme>,
    /// Text the input method is composing.
    marked: Option<String>,
    caret_moved: bool,
    pressed: bool,
    hovered: bool,
    watch: Vec<Subscription>,
    _updates: Task<()>,
}

impl EventEmitter<Asked> for WebPanelView {}

impl WebPanelView {
    pub fn new(
        hub: &Hub,
        plugin: PluginRef,
        block: WebPanelBlock,
        context: About,
        cx: &mut Context<Self>,
    ) -> Self {
        let height = block.height.unwrap_or(HEIGHT);
        let options = webview::Options {
            origin: format!("{}/{}", plugin.node, plugin.id),
            entry: block.entry.clone(),
            style: gateway::panel::style(&Theme::web_panel()),
            background: Theme::rgb(Theme::surface()),
            size: Size {
                width: 640.,
                height,
                scale: 1.,
            },
            loader: loader(hub, &plugin),
        };
        let (panel, problem, updates) = match Panel::open(options) {
            Ok((panel, mut updates)) => {
                let follow = cx.spawn(async move |this, cx| {
                    while let Some(update) = updates.next().await {
                        if this.update(cx, |this, cx| this.apply(update, cx)).is_err() {
                            break;
                        }
                    }
                });
                (Some(panel), None, follow)
            }
            Err(err) => (None, Some(err.to_string()), Task::ready(())),
        };
        Self {
            plugin,
            block,
            context,
            panel,
            problem,
            frame: None,
            retired: Vec::new(),
            height,
            size: None,
            bounds: None,
            cursor: CursorStyle::Arrow,
            menu: None,
            focus: cx.focus_handle(),
            theme: None,
            marked: None,
            caret_moved: false,
            pressed: false,
            hovered: false,
            watch: Vec::new(),
            _updates: updates,
        }
    }

    /// Whether the page this shows is the one `block` asks for.
    pub fn shows(&self, block: &WebPanelBlock) -> bool {
        self.block.entry == block.entry && self.block.params == block.params
    }

    /// Takes a block fetched again, keeping the page; a height the block sets wins.
    pub fn renew(&mut self, block: WebPanelBlock, cx: &mut Context<Self>) {
        if let Some(height) = block.height {
            self.height = height;
        }
        self.block = block;
        cx.notify();
    }

    fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Frame => {
                if let Some(frame) = self.panel.as_ref().and_then(Panel::take_frame) {
                    let size = gpui::size(
                        px(frame_logical(frame.width, frame.scale)),
                        px(frame_logical(frame.height, frame.scale)),
                    );
                    if let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels)
                    {
                        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                        if let Some((old, _)) = self.frame.replace((image, size)) {
                            self.retired.push(old);
                        }
                    }
                }
            }
            Update::Loaded => self.greet(),
            Update::Message(json) => self.heard(&json, cx),
            Update::Cursor(cursor) => self.cursor = cursor_style(cursor),
            Update::Menu(menu) => self.menu = menu,
            Update::Copied(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Update::Caret => self.caret_moved = true,
            Update::Stopped => {
                self.problem = Some("The panel's page stopped.".into());
            }
        }
        cx.notify();
    }

    /// Tells a page that has loaded the theme, its params and what its view is about.
    fn greet(&mut self) {
        let theme = Theme::web_panel();
        let init = HostMessage::Init {
            theme: theme.clone(),
            params: self.block.params.clone(),
            context: self.context.clone(),
            plugin: self.plugin.clone(),
        };
        self.post(&init);
        self.theme = Some(theme);
    }

    fn post(&self, message: &HostMessage) {
        let Some(panel) = &self.panel else {
            return;
        };
        match gateway::panel::host_message(message) {
            Ok(json) => panel.post(json),
            Err(err) => tracing::warn!(%err, "a web panel message couldn't be written"),
        }
    }

    fn heard(&mut self, json: &str, cx: &mut Context<Self>) {
        match gateway::panel::page_message(json) {
            Ok(Message::Ready) => {}
            Ok(Message::Resize(height)) => {
                #[expect(clippy::cast_precision_loss, reason = "a height in pixels is far below 2^24")]
                let height = height as f32;
                self.height = height.clamp(HEIGHTS.0, HEIGHTS.1);
            }
            Ok(Message::Action(action)) => cx.emit(Asked(vec![action])),
            Err(err) => {
                tracing::warn!(plugin = %self.plugin.id, %err, "a web panel posted a message outside the contract");
            }
        }
    }

    /// Gives a loaded page the theme when it changed since it was given last.
    fn follow_theme(&mut self) {
        let Some(given) = &self.theme else {
            return;
        };
        let theme = Theme::web_panel();
        if *given == theme {
            return;
        }
        if let Some(panel) = &self.panel {
            panel.restyle(gateway::panel::style(&theme));
        }
        self.post(&HostMessage::Theme(theme.clone()));
        self.theme = Some(theme);
    }

    /// The panel's place on screen, from its layout: the page follows its size and scale.
    fn place(&mut self, bounds: Bounds<Pixels>, scale: f32) {
        self.bounds = Some(bounds);
        let size = Size {
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            scale,
        };
        if self.size != Some(size)
            && let Some(panel) = &self.panel
        {
            panel.resize(size);
            self.size = Some(size);
        }
    }

    fn menu_overlay(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let menu = self.menu.as_ref()?;
        let bounds = self.bounds?;
        let at = bounds.origin + gpui::point(px(menu.anchor.x), px(menu.anchor.y + menu.anchor.height));
        Some(gpui::deferred(
            gpui::anchored().position(at).child(
                div()
                    .occlude()
                    .min_w(px(menu.anchor.width.max(160.)))
                    .max_h(px(360.))
                    .p_1()
                    .rounded(px(12.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .shadow_md()
                    .text_sm()
                    .text_color(Theme::ink())
                    .flex()
                    .flex_col()
                    .on_mouse_down_out(cx.listener(|this, _: &gpui::MouseDownEvent, _, cx| {
                        this.choose(None, cx);
                    }))
                    .children(menu.items.iter().enumerate().map(|(index, label)| {
                        div()
                            .id(("web-panel-option", index))
                            .px_3()
                            .py_1p5()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .when(menu.selected == Some(index), |item| item.bg(Theme::selected()))
                            .hover(|item| item.bg(Theme::raised()))
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                this.choose(Some(index), cx);
                            }))
                            .child(label.clone())
                    })),
            ),
        ))
    }

    fn choose(&mut self, item: Option<usize>, cx: &mut Context<Self>) {
        if let Some(panel) = &self.panel {
            panel.choose(item);
        }
        self.menu = None;
        cx.notify();
    }
}

/// A frame's side in logical pixels.
fn frame_logical(pixels: u32, scale: f32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame is far below 2^24 pixels a side"
    )]
    let pixels = pixels as f32;
    pixels / scale
}

/// Answers a page's requests through the plug-in's node, as the contract's proxy routes do.
fn loader(hub: &Hub, plugin: &PluginRef) -> webview::Loader {
    let (hub, plugin) = (hub.clone(), plugin.clone());
    Arc::new(move |fetch: Fetch| {
        let (hub, plugin) = (hub.clone(), plugin.clone());
        Box::pin(async move {
            let request = Request {
                method: fetch.method,
                path: fetch.path,
                headers: fetch.headers,
                body: fetch.body,
            };
            match hub.plugin_page(plugin, request).await {
                Ok(response) => Fetched {
                    status: response.status,
                    media_type: response.media_type,
                    body: response.body,
                },
                Err(err) => Fetched {
                    status: 502,
                    media_type: Some("text/plain; charset=utf-8".into()),
                    body: err.to_string().into_bytes(),
                },
            }
        })
    })
}

fn cursor_style(cursor: webview::Cursor) -> CursorStyle {
    use webview::Cursor;
    match cursor {
        Cursor::Default => CursorStyle::Arrow,
        Cursor::Pointer => CursorStyle::PointingHand,
        Cursor::Text => CursorStyle::IBeam,
        Cursor::VerticalText => CursorStyle::IBeamCursorForVerticalLayout,
        Cursor::Crosshair => CursorStyle::Crosshair,
        Cursor::Grab => CursorStyle::OpenHand,
        Cursor::Move | Cursor::Grabbing => CursorStyle::ClosedHand,
        Cursor::NotAllowed => CursorStyle::OperationNotAllowed,
        Cursor::ResizeEastWest => CursorStyle::ResizeLeftRight,
        Cursor::ResizeNorthSouth => CursorStyle::ResizeUpDown,
        Cursor::ResizeNorthEastSouthWest => CursorStyle::ResizeUpRightDownLeft,
        Cursor::ResizeNorthWestSouthEast => CursorStyle::ResizeUpLeftDownRight,
        Cursor::ResizeColumn => CursorStyle::ResizeColumn,
        Cursor::ResizeRow => CursorStyle::ResizeRow,
        Cursor::ResizeNorth => CursorStyle::ResizeUp,
        Cursor::ResizeSouth => CursorStyle::ResizeDown,
        Cursor::ResizeEast => CursorStyle::ResizeRight,
        Cursor::ResizeWest => CursorStyle::ResizeLeft,
        Cursor::ContextMenu => CursorStyle::ContextualMenu,
        Cursor::Copy => CursorStyle::DragCopy,
        Cursor::Alias => CursorStyle::DragLink,
    }
}

impl Render for WebPanelView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.retired.drain(..) {
            window.drop_image(image).ok();
        }
        if self.watch.is_empty() {
            self.watch_focus(window, cx);
        }
        if std::mem::take(&mut self.caret_moved) {
            window.invalidate_character_coordinates();
        }
        self.follow_theme();
        let view = cx.entity();
        let cursor = self.cursor;
        let surface = canvas(
            {
                let view = view.clone();
                move |bounds, window, cx| {
                    let scale = window.scale_factor();
                    view.update(cx, |this, _| this.place(bounds, scale));
                    window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
                }
            },
            move |bounds, hitbox, window, cx| {
                window.set_cursor_style(cursor, &hitbox);
                input::listen(&view, &hitbox, bounds, window, cx);
            },
        )
        .absolute()
        .size_full();
        div()
            .relative()
            .w_full()
            .h(px(self.height))
            .flex_none()
            .rounded(px(16.))
            .overflow_hidden()
            .bg(Theme::surface())
            .key_context("WebPanel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .children(self.frame.clone().map(|(image, size)| {
                img(image)
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(size.width)
                    .h(size.height)
            }))
            .child(surface)
            .children(self.problem.clone().map(|problem| {
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(12.))
                    .max_w(px(420.))
                    .px_3()
                    .py_1()
                    .rounded(px(12.))
                    .bg(Theme::danger_container())
                    .text_xs()
                    .text_color(Theme::danger())
                    .child(problem)
            }))
            .children(self.menu_overlay(cx))
    }
}
