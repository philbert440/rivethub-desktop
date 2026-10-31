use std::cell::Cell;
use std::rc::Rc;

use app::{TerminalHandle, TerminalState};
use gpui::{
    App, Bounds, ClickEvent, ClipboardItem, Context, Element, ElementId, EventEmitter, FocusHandle,
    Focusable, FontStyle, FontWeight, GlobalElementId, InspectorElementId, IntoElement, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollWheelEvent, ShapedLine, SharedString, Style, Subscription, Task, TextAlign, TextRun,
    UnderlineStyle, Window, anchored, deferred, div, fill, font, point, prelude::*, px, relative, rgb, size,
};
use term::{GridSize, Hit, Link, Modifiers, SelectBy, Snapshot, encode_key};

use crate::theme::Theme;

const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 18.;

/// Where the grid was last painted, so pointer positions map to cells.
#[derive(Clone, Copy)]
struct Geometry {
    origin: Point<Pixels>,
    cell: Pixels,
}

impl Geometry {
    fn hit(self, position: Point<Pixels>) -> Hit {
        let x = f32::from(position.x - self.origin.x).max(0.) / f32::from(self.cell).max(1.);
        let y = f32::from(position.y - self.origin.y).max(0.) / LINE_HEIGHT;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "non-negative and bounded by the grid"
        )]
        let (row, column) = (y as usize, x as usize);
        Hit {
            row,
            column,
            right_half: x.fract() >= 0.5,
        }
    }
}

/// The terminal rang its bell while the window was in the background.
pub struct Bell;

/// A link under the pointer, on the given visible row.
#[derive(Clone)]
struct Hovered {
    row: usize,
    link: Link,
}

pub struct TerminalView {
    handle: TerminalHandle,
    focus: FocusHandle,
    geometry: Rc<Cell<Option<Geometry>>>,
    selecting: bool,
    menu: Option<Point<Pixels>>,
    pointer: Option<Hit>,
    hovered: Option<Hovered>,
    /// A bell already raised a notice; more wait until the window is active again.
    bell_raised: bool,
    _revisions: Task<()>,
    _activation: Subscription,
}

impl EventEmitter<Bell> for TerminalView {}

impl TerminalView {
    pub fn new(handle: TerminalHandle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut revisions = handle.revisions();
        let watcher = cx.spawn_in(window, async move |this, cx| {
            while revisions.changed().await.is_ok() {
                if this.update_in(cx, Self::changed).is_err() {
                    break;
                }
            }
        });
        let activation = cx.observe_window_activation(window, |this, window, _| {
            if window.is_window_active() {
                this.bell_raised = false;
            }
        });
        Self {
            handle,
            focus: cx.focus_handle(),
            geometry: Rc::default(),
            selecting: false,
            menu: None,
            pointer: None,
            hovered: None,
            bell_raised: false,
            _revisions: watcher,
            _activation: activation,
        }
    }

    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.handle.screen().read().take_bell() && !self.bell_raised && !window.is_window_active() {
            self.bell_raised = true;
            cx.emit(Bell);
        }
        self.hover(self.pointer);
        cx.notify();
    }

    /// Points at `hit`, or at nothing, and says whether the link under the pointer changed.
    fn hover(&mut self, hit: Option<Hit>) -> bool {
        self.pointer = hit;
        let hovered = hit.and_then(|hit| {
            let link = self.handle.screen().read().link_at(hit)?;
            Some(Hovered { row: hit.row, link })
        });
        let changed = hovered.as_ref().map(|now| (now.row, &now.link))
            != self.hovered.as_ref().map(|was| (was.row, &was.link));
        self.hovered = hovered;
        changed
    }

    pub fn end(&self) {
        self.handle.end();
    }

    pub fn exited(&self) -> bool {
        self.handle.state() == TerminalState::Exited
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let mods = Modifiers {
            ctrl: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
        };
        if keystroke.key == "insert" && !mods.alt && mods.ctrl != mods.shift {
            if mods.ctrl {
                self.copy(cx);
            } else {
                self.paste_clipboard(cx);
            }
            cx.stop_propagation();
            return;
        }
        let app_cursor = self.handle.screen().read().app_cursor();
        if let Some(bytes) = encode_key(&keystroke.key, keystroke.key_char.as_deref(), mods, app_cursor) {
            self.handle.screen().write().clear_selection();
            self.handle.write(bytes);
            cx.stop_propagation();
        }
    }

    fn copy(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.handle.screen().read().selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste_clipboard(&self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.paste(&text, cx);
        }
    }

    fn paste(&self, text: &str, cx: &mut Context<Self>) {
        let screen = self.handle.screen();
        let mut screen = screen.write();
        screen.clear_selection();
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let bytes = if screen.bracketed_paste() {
            format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
        } else {
            text.into_bytes()
        };
        drop(screen);
        self.handle.write(bytes);
        cx.notify();
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(geometry) = self.geometry.get() else {
            return;
        };
        let hit = geometry.hit(event.position);
        if event.modifiers.control
            && event.click_count == 1
            && let Some(link) = self.handle.screen().read().link_at(hit)
        {
            cx.open_url(&link.url);
            return;
        }
        let screen = self.handle.screen();
        let mut screen = screen.write();
        if event.modifiers.shift && event.click_count == 1 && screen.selected_text().is_some() {
            screen.extend_selection(hit);
        } else {
            let by = match event.click_count {
                1 => SelectBy::Character,
                2 => SelectBy::Word,
                _ => SelectBy::Line,
            };
            screen.select(hit, by);
        }
        self.selecting = true;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.geometry.get().map(|geometry| geometry.hit(event.position)) else {
            return;
        };
        if self.selecting && event.pressed_button == Some(MouseButton::Left) {
            self.handle.screen().write().extend_selection(hit);
            cx.notify();
        } else if self
            .pointer
            .is_none_or(|was| (was.row, was.column) != (hit.row, hit.column))
            && self.hover(Some(hit))
        {
            cx.notify();
        }
    }

    /// A finished selection goes to the primary selection, so other apps can paste it; the
    /// clipboard only changes on an explicit copy.
    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.selecting) {
            return;
        }
        if let Some(text) = self.handle.screen().read().selected_text() {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
    }

    fn open_menu(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.menu = Some(event.position);
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    fn context_menu(&self, at: Point<Pixels>, cx: &mut Context<Self>) -> impl IntoElement {
        let can_copy = self.handle.screen().read().selected_text().is_some();
        let item = |id: &'static str, label: &'static str, enabled: bool| {
            div()
                .id(id)
                .px_3()
                .py(px(6.))
                .rounded(px(8.))
                .text_color(if enabled { Theme::ink() } else { Theme::ink_dim() })
                .when(enabled, |item| {
                    item.cursor_pointer().hover(|item| item.bg(Theme::raised()))
                })
                .child(label)
        };
        deferred(
            anchored().position(at).child(
                div()
                    .occlude()
                    .min_w(px(160.))
                    .p_1()
                    .rounded(px(12.))
                    .bg(Theme::card())
                    .border_1()
                    .border_color(Theme::line())
                    .shadow_md()
                    .text_sm()
                    .flex()
                    .flex_col()
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_menu(cx)))
                    .child(item("terminal-copy", "Copy", can_copy).when(can_copy, |item| {
                        item.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.copy(cx);
                            this.close_menu(cx);
                        }))
                    }))
                    .child(item("terminal-paste", "Paste", true).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.paste_clipboard(cx);
                            this.close_menu(cx);
                        },
                    ))),
            ),
        )
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height()).y;
        let lines = (f32::from(delta) / LINE_HEIGHT).round();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a wheel step scrolls a handful of lines"
        )]
        let lines = lines as i32;
        if lines != 0 {
            self.handle.screen().write().scroll(lines);
            self.hover(self.pointer);
            cx.notify();
        }
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match self.handle.state() {
            TerminalState::Connecting => Some(self.handle.problem().map_or_else(
                || "Connecting to the terminal…".to_owned(),
                |problem| format!("Couldn't open the terminal: {problem}. Trying again…"),
            )),
            TerminalState::Exited => Some("The terminal process exited.".to_owned()),
            TerminalState::Live => None,
        };
        div()
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::open_menu))
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                if !inside && this.hover(None) {
                    cx.notify();
                }
            }))
            .size_full()
            .p_3()
            .rounded(px(16.))
            .bg(rgb(self.handle.screen().read().background().to_u32()))
            .flex()
            .flex_col()
            .children(status.map(|line| div().text_xs().text_color(Theme::ink_dim()).child(line)))
            .child(TerminalElement {
                handle: self.handle.clone(),
                focused: self.focus.clone(),
                geometry: self.geometry.clone(),
                link: self.hovered.clone(),
            })
            .children(self.menu.map(|at| self.context_menu(at, cx)))
    }
}

struct TerminalElement {
    handle: TerminalHandle,
    focused: FocusHandle,
    geometry: Rc<Cell<Option<Geometry>>>,
    link: Option<Hovered>,
}

struct Prepainted {
    snapshot: Snapshot,
    lines: Vec<ShapedLine>,
    cell: Pixels,
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

fn mono(bold: bool, italic: bool) -> gpui::Font {
    let mut face = font(Theme::MONO);
    if bold {
        face.weight = FontWeight::BOLD;
    }
    if italic {
        face.style = FontStyle::Italic;
    }
    face
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        style.flex_grow = 1.;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
        let probe = TextRun {
            len: 1,
            font: mono(false, false),
            color: Theme::ink().into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let cell = window
            .text_system()
            .shape_line("M".into(), px(FONT_SIZE), &[probe], None)
            .width;
        let fits = |available: Pixels, step: f32| {
            let count = (f32::from(available) / step)
                .floor()
                .clamp(1., f32::from(u16::MAX));
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to u16"
            )]
            let count = count as u16;
            count
        };
        let grid = GridSize {
            columns: fits(bounds.size.width, f32::from(cell).max(1.)),
            rows: fits(bounds.size.height, LINE_HEIGHT),
        };
        self.handle.resize(grid);
        self.geometry.set(Some(Geometry {
            origin: bounds.origin,
            cell,
        }));
        let snapshot = self.handle.screen().read().snapshot();
        let lines = snapshot
            .lines
            .iter()
            .map(|spans| {
                let text: String = spans.iter().map(|span| span.text.as_str()).collect();
                let runs: Vec<TextRun> = spans
                    .iter()
                    .map(|span| TextRun {
                        len: span.text.len(),
                        font: mono(span.look.bold, span.look.italic),
                        color: rgb(span.fg.to_u32()).into(),
                        background_color: None,
                        underline: span.look.underline.then(|| UnderlineStyle {
                            thickness: px(1.),
                            color: None,
                            wavy: false,
                        }),
                        strikethrough: None,
                    })
                    .collect();
                window
                    .text_system()
                    .shape_line(SharedString::from(text), px(FONT_SIZE), &runs, None)
            })
            .collect();
        Prepainted {
            snapshot,
            lines,
            cell,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        prepainted: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let cell = prepainted.cell;
        let line_height = px(LINE_HEIGHT);
        let background = prepainted.snapshot.background;
        for (row, spans) in prepainted.snapshot.lines.iter().enumerate() {
            let top = bounds.top() + line_height * row_f32(row);
            let mut column = 0usize;
            for span in spans {
                let width = span.text.chars().count();
                let fill_color = if span.selected {
                    Some(Theme::selected())
                } else {
                    (span.bg != background).then(|| rgb(span.bg.to_u32()))
                };
                if let Some(color) = fill_color {
                    let origin = point(bounds.left() + cell * row_f32(column), top);
                    window.paint_quad(fill(
                        Bounds::new(origin, size(cell * row_f32(width), line_height)),
                        color,
                    ));
                }
                column += width;
            }
        }
        for (row, line) in prepainted.lines.iter().enumerate() {
            let origin = point(bounds.left(), bounds.top() + line_height * row_f32(row));
            if let Err(err) = line.paint(origin, line_height, TextAlign::Left, None, window, cx) {
                tracing::warn!(%err, "painting a terminal line failed");
            }
        }
        if let Some((row, column)) = prepainted.snapshot.cursor {
            let origin = point(
                bounds.left() + cell * row_f32(column),
                bounds.top() + line_height * row_f32(row),
            );
            let shape = if self.focused.is_focused(window) {
                size(cell, line_height)
            } else {
                size(px(2.), line_height)
            };
            window.paint_quad(fill(Bounds::new(origin, shape), Theme::accent()));
        }
        if let Some(Hovered { row, link }) = &self.link {
            let origin = point(
                bounds.left() + cell * row_f32(link.columns.start),
                bounds.top() + line_height * row_f32(*row + 1) - px(2.),
            );
            let width = cell * row_f32(link.columns.len());
            window.paint_quad(fill(Bounds::new(origin, size(width, px(1.))), Theme::accent()));
        }
    }
}

fn row_f32(index: usize) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "terminal grids stay far below f32's exact range"
    )]
    let value = index as f32;
    value
}
