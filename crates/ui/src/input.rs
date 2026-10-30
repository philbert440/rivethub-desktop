use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div, fill, point, prelude::*, px, relative,
    size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::Theme;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        Submit,
        Newline,
        Up,
        Down,
        SelectUp,
        SelectDown
    ]
);

pub struct Submitted(pub String);

/// What the input is for, which decides what Enter does with its text and how it is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    /// Enter sends the text and clears the input.
    Message,
    /// Enter hands the text on and keeps it.
    Field,
    /// A field whose text is masked and never copied out.
    Secret,
}

/// An image pasted into a multi-line input, for the view that owns it to attach.
pub struct PastedImage(pub gpui::Image);

pub struct TextInput {
    focus_handle: FocusHandle,
    /// Whether the text may span lines: Shift+Enter and pasted newlines stay, where a one-line input flattens them.
    multiline: bool,
    purpose: Purpose,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<Layout>,
    scroll: Pixels,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl TextInput {
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let start = self.content[..self.cursor_offset()]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let from = self.cursor_offset();
        let end = self.content[from..]
            .find('\n')
            .map_or(self.content.len(), |newline| from + newline);
        self.move_to(end, cx);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        }
    }

    fn vertical(&self, rows: f32) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        let at = layout.point_for(self.cursor_offset())?;
        Some(layout.index_for(point(
            at.x,
            at.y + layout.line_height * rows + layout.line_height / 2.,
        )))
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.vertical(-1.) {
            self.move_to(index, cx);
        }
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.vertical(1.) {
            self.move_to(index, cx);
        }
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.vertical(-1.) {
            self.select_to(index, cx);
        }
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.vertical(1.) {
            self.select_to(index, cx);
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                window.play_system_bell();
                return;
            }
            self.select_to(prev, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                window.play_system_bell();
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    #[expect(clippy::unused_self, reason = "GPUI action listeners receive the view")]
    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(text) = item.text() {
            let text = text.replace("\r\n", "\n");
            let text = if self.multiline {
                text
            } else {
                text.replace('\n', " ")
            };
            self.replace_text_in_range(None, &text, window, cx);
            return;
        }
        if !self.multiline {
            return;
        }
        for entry in item.entries() {
            if let gpui::ClipboardEntry::Image(image) = entry {
                cx.emit(PastedImage(image.clone()));
            }
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && self.purpose != Purpose::Secret {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && self.purpose != Purpose::Secret {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        // An empty input lays out its placeholder, whose offsets aren't the content's.
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(layout)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        layout.index_for(point(
            position.x - bounds.left(),
            position.y - bounds.top() + self.scroll,
        ))
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf8_offset(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn reset(&mut self) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.last_bounds = None;
        self.scroll = Pixels::ZERO;
        self.is_selecting = false;
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(range.start..range.start + new_text.len());
        }
        self.selected_range = composed_selection(&range, new_text, new_selected_range_utf16.as_ref());

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = layout.point_for(range.start)?;
        let end = layout.point_for(range.end)?;
        let top = bounds.top() - self.scroll;
        Some(Bounds::from_corners(
            point(bounds.left() + start.x, top + start.y),
            point(
                bounds.left() + end.x.max(start.x),
                top + end.y + layout.line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point_in_window: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let layout = self.last_layout.as_ref()?;
        let local = point(
            point_in_window.x - bounds.left(),
            point_in_window.y - bounds.top() + self.scroll,
        );
        Some(self.offset_to_utf16(layout.index_for(local)))
    }
}

const LINE_HEIGHT: f32 = 22.;
const MAX_HEIGHT: f32 = 336.;

pub struct Layout {
    lines: Vec<(usize, Pixels, WrappedLine)>,
    line_height: Pixels,
    height: Pixels,
}

impl Layout {
    fn shape(
        text: &SharedString,
        runs: &[TextRun],
        width: Pixels,
        font_size: Pixels,
        window: &Window,
    ) -> Self {
        let line_height = px(LINE_HEIGHT);
        let wrapped = window
            .text_system()
            .shape_text(text.clone(), font_size, runs, Some(width), None)
            .unwrap_or_default();
        let mut lines = Vec::with_capacity(wrapped.len());
        let mut start = 0;
        let mut top = Pixels::ZERO;
        for line in wrapped {
            let height = line.size(line_height).height.max(line_height);
            let length = line.len();
            lines.push((start, top, line));
            start += length + 1;
            top += height;
        }
        Self {
            lines,
            line_height,
            height: top.max(line_height),
        }
    }

    fn point_for(&self, offset: usize) -> Option<Point<Pixels>> {
        let (start, top, line) = self.lines.iter().rev().find(|(start, _, _)| *start <= offset)?;
        let local = line.position_for_index(offset - start, self.line_height)?;
        Some(point(local.x, *top + local.y))
    }

    fn index_for(&self, at: Point<Pixels>) -> usize {
        let Some((start, top, line)) = self
            .lines
            .iter()
            .rev()
            .find(|(_, top, _)| *top <= at.y)
            .or_else(|| self.lines.first())
        else {
            return 0;
        };
        let local = point(at.x, (at.y - *top).max(Pixels::ZERO));
        start
            + line
                .closest_index_for_position(local, self.line_height)
                .unwrap_or_else(|nearest| nearest)
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    layout: Option<Layout>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    scroll: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The byte offset in `text` of its UTF-16 offset `offset`, or its end.
fn utf8_offset(text: &str, offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in text.chars() {
        if utf16 >= offset {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

/// The selection once `new_text` is composed over `range`: `within`, which the platform gives in
/// UTF-16 inside the composed text, or the composition's end.
fn composed_selection(range: &Range<usize>, new_text: &str, within: Option<&Range<usize>>) -> Range<usize> {
    within.map_or(
        range.start + new_text.len()..range.start + new_text.len(),
        |within| {
            range.start + utf8_offset(new_text, within.start)..range.start + utf8_offset(new_text, within.end)
        },
    )
}

/// A secret shown as one mark a character, each as long in UTF-8 as the character it hides, so
/// every offset into the text still lands on the same place.
fn masked(text: &str) -> String {
    text.chars()
        .map(|ch| match ch.len_utf8() {
            1 => '*',
            2 => '\u{b7}',
            3 => '\u{2022}',
            _ => '\u{1f784}',
        })
        .collect()
}

fn runs_for(input: &TextInput, window: &Window) -> (SharedString, Vec<TextRun>) {
    let style = window.text_style();
    let (text, color) = if input.content.is_empty() {
        (input.placeholder.clone(), Theme::ink_dim().into())
    } else if input.purpose == Purpose::Secret {
        (masked(&input.content).into(), style.color)
    } else {
        (input.content.clone(), style.color)
    };
    let base = TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let runs = match input.marked_range.as_ref().filter(|_| !input.content.is_empty()) {
        Some(marked) => vec![
            TextRun {
                len: marked.start,
                ..base.clone()
            },
            TextRun {
                len: marked.end - marked.start,
                underline: Some(UnderlineStyle {
                    color: Some(base.color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                ..base.clone()
            },
            TextRun {
                len: text.len() - marked.end,
                ..base
            },
        ]
        .into_iter()
        .filter(|run| run.len > 0)
        .collect(),
        None => vec![base],
    };
    (text, runs)
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let (text, runs) = runs_for(self.input.read(cx), window);
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let id = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.unwrap_or(match available.width {
                gpui::AvailableSpace::Definite(width) => width,
                _ => px(600.),
            });
            let layout = Layout::shape(&text, &runs, width, font_size, window);
            size(width, layout.height.min(px(MAX_HEIGHT)))
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let (text, runs) = runs_for(input, window);
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());
        let layout = Layout::shape(&text, &runs, bounds.size.width, font_size, window);
        let line_height = layout.line_height;
        let cursor_at = if input.content.is_empty() {
            Some(Point::default())
        } else {
            layout.point_for(input.cursor_offset())
        };
        let overflow = (layout.height - bounds.size.height).max(Pixels::ZERO);
        let scroll = cursor_at.map_or(Pixels::ZERO, |at| {
            (at.y + line_height - bounds.size.height).clamp(Pixels::ZERO, overflow)
        });
        let origin = point(bounds.left(), bounds.top() - scroll);
        let selected = input.selected_range.clone();
        let mut selection = Vec::new();
        if !selected.is_empty()
            && !input.content.is_empty()
            && let (Some(start), Some(end)) =
                (layout.point_for(selected.start), layout.point_for(selected.end))
        {
            let mut row = start.y;
            while row <= end.y {
                let left = if row == start.y { start.x } else { Pixels::ZERO };
                let right = if row == end.y { end.x } else { bounds.size.width };
                selection.push(fill(
                    Bounds::from_corners(
                        point(origin.x + left, origin.y + row),
                        point(origin.x + right, origin.y + row + line_height),
                    ),
                    Theme::selected(),
                ));
                row += line_height;
            }
        }
        let cursor = selected.is_empty().then_some(cursor_at).flatten().map(|at| {
            fill(
                Bounds::new(point(origin.x + at.x, origin.y + at.y), size(px(2.), line_height)),
                Theme::accent(),
            )
        });
        PrepaintState {
            layout: Some(layout),
            cursor,
            selection,
            scroll,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let Some(layout) = prepaint.layout.take() else {
            return;
        };
        let scroll = prepaint.scroll;
        let focused = focus_handle.is_focused(window);
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if focused {
                for quad in prepaint.selection.drain(..) {
                    window.paint_quad(quad);
                }
            }
            for (_, top, line) in &layout.lines {
                let origin = point(bounds.left(), bounds.top() + *top - scroll);
                if let Err(err) = line.paint(
                    origin,
                    layout.line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                ) {
                    tracing::warn!(%err, "painting the input failed");
                }
            }
            if focused && let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = Some(layout);
            input.last_bounds = Some(bounds);
            input.scroll = scroll;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .w_full()
            .line_height(px(LINE_HEIGHT))
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<Submitted> for TextInput {}
impl EventEmitter<PastedImage> for TextInput {}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            multiline: false,
            purpose: Purpose::Message,
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            scroll: Pixels::ZERO,
            last_bounds: None,
            is_selecting: false,
        }
    }
    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    /// Lets the text span lines, as a message or instructions do.
    #[must_use]
    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// Keeps the text when Enter hands it on, as a form's field does.
    #[must_use]
    pub fn keeping(mut self) -> Self {
        self.purpose = Purpose::Field;
        self
    }

    /// Masks the text and keeps it off the clipboard.
    #[must_use]
    pub fn secret(mut self) -> Self {
        self.purpose = Purpose::Secret;
        self
    }

    /// Starts with `text` and the cursor at its end, as a form's field starts with its value.
    #[must_use]
    pub fn with_text(mut self, text: &str) -> Self {
        self.content = text.to_owned().into();
        self.selected_range = self.content.len()..self.content.len();
        self
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.reset();
        self.content = text.to_owned().into();
        self.selected_range = 0..self.content.len();
        cx.notify();
    }

    pub fn text(&self) -> &str {
        self.content.trim()
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }

    pub fn take(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let text = self.content.trim().to_owned();
        if text.is_empty() {
            return None;
        }
        self.reset();
        cx.notify();
        Some(text)
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        if self.purpose != Purpose::Message {
            cx.emit(Submitted(self.text().to_owned()));
        } else if let Some(text) = self.take(cx) {
            cx.emit(Submitted(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::composed_selection;

    #[test]
    fn a_composition_selects_inside_its_own_text() {
        assert_eq!(composed_selection(&(3..5), "ñab", Some(&(1..2))), 5..6);
        assert_eq!(composed_selection(&(3..5), "ñab", None), 7..7);
        assert_eq!(composed_selection(&(0..10), "", Some(&(0..0))), 0..0);
    }
}
