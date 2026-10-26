use app::{TerminalHandle, TerminalState};
use gpui::{
    App, Bounds, Context, Element, ElementId, FocusHandle, Focusable, FontStyle, FontWeight, GlobalElementId,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, Pixels, Render, ScrollWheelEvent, ShapedLine,
    SharedString, Style, Task, TextAlign, TextRun, UnderlineStyle, Window, div, fill, font, point,
    prelude::*, px, relative, rgb, size,
};
use term::{GridSize, Modifiers, Snapshot, encode_key};

use crate::theme::Theme;

const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 18.;

pub struct TerminalView {
    handle: TerminalHandle,
    focus: FocusHandle,
    _revisions: Task<()>,
}

impl TerminalView {
    pub fn new(handle: TerminalHandle, cx: &mut Context<Self>) -> Self {
        let mut revisions = handle.revisions();
        let watcher = cx.spawn(async move |this, cx| {
            while revisions.changed().await.is_ok() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            handle,
            focus: cx.focus_handle(),
            _revisions: watcher,
        }
    }

    pub fn end(&self) {
        self.handle.end();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let mods = Modifiers {
            ctrl: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
        };
        let app_cursor = self.handle.screen().read().app_cursor();
        if let Some(bytes) = encode_key(&keystroke.key, keystroke.key_char.as_deref(), mods, app_cursor) {
            self.handle.write(bytes);
            cx.stop_propagation();
        }
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
            TerminalState::Connecting => Some("Connecting to the terminal…"),
            TerminalState::Exited => Some("The terminal process exited."),
            TerminalState::Live => None,
        };
        div()
            .key_context("Terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_scroll_wheel(cx.listener(Self::scroll))
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
            })
    }
}

struct TerminalElement {
    handle: TerminalHandle,
    focused: FocusHandle,
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
                        font: mono(span.bold, span.italic),
                        color: rgb(span.fg.to_u32()).into(),
                        background_color: None,
                        underline: span.underline.then(|| UnderlineStyle {
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
                if span.bg != background {
                    let origin = point(bounds.left() + cell * row_f32(column), top);
                    window.paint_quad(fill(
                        Bounds::new(origin, size(cell * row_f32(width), line_height)),
                        rgb(span.bg.to_u32()),
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
