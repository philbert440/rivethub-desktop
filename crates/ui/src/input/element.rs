use gpui::{
    App, Bounds, ElementId, ElementInputHandler, Entity, GlobalElementId, LayoutId, PaintQuad, Pixels, Point,
    SharedString, Style, TextRun, UnderlineStyle, Window, WrappedLine, fill, point, prelude::*, px, relative,
    size,
};

use super::{Purpose, TextInput};
use crate::theme::Theme;

pub(super) const LINE_HEIGHT: f32 = 22.;
const MAX_HEIGHT: f32 = 336.;

pub(super) struct Layout {
    lines: Vec<(usize, Pixels, WrappedLine)>,
    pub(super) line_height: Pixels,
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

    pub(super) fn point_for(&self, offset: usize) -> Option<Point<Pixels>> {
        let (start, top, line) = self.lines.iter().rev().find(|(start, _, _)| *start <= offset)?;
        let local = line.position_for_index(offset - start, self.line_height)?;
        Some(point(local.x, *top + local.y))
    }

    pub(super) fn index_for(&self, at: Point<Pixels>) -> usize {
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

pub(super) struct TextElement {
    pub(super) input: Entity<TextInput>,
}

pub(super) struct PrepaintState {
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
