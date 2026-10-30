use gpui::{FontWeight, Hsla, Pixels, SharedString, TextAlign, TextRun, Window, font, point, px};
use model::plugin::canvas::Weight;
use model::plugin::view::Align;

use crate::theme::Theme;

pub(super) struct Face {
    pub(super) size: Pixels,
    pub(super) weight: Weight,
    pub(super) monospace: bool,
    pub(super) color: Hsla,
}

/// One line of text, ending in an ellipsis past `max_width`.
pub(super) fn shape_text(
    text: &str,
    face: &Face,
    max_width: Option<Pixels>,
    window: &Window,
) -> Option<gpui::ShapedLine> {
    if face.size < px(4.) || text.is_empty() {
        return None;
    }
    let mut family = font(if face.monospace { Theme::MONO } else { Theme::SANS });
    family.weight = match face.weight {
        Weight::Regular => FontWeight::NORMAL,
        Weight::Medium => FontWeight::MEDIUM,
        Weight::Bold => FontWeight::BOLD,
    };
    let shape = |text: SharedString| {
        let run = TextRun {
            len: text.len(),
            font: family.clone(),
            color: face.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window.text_system().shape_line(text, face.size, &[run], None)
    };
    let first_line: SharedString = text.lines().next().unwrap_or_default().to_owned().into();
    let line = shape(first_line.clone());
    let Some(max) = max_width.filter(|max| line.width > *max) else {
        return Some(line);
    };
    let ends: Vec<usize> = first_line.char_indices().map(|(at, _)| at).collect();
    let (mut fits, mut over) = (0, ends.len());
    while fits + 1 < over {
        let middle = usize::midpoint(fits, over);
        let candidate = format!("{}…", first_line[..ends[middle]].trim_end());
        if shape(candidate.into()).width <= max {
            fits = middle;
        } else {
            over = middle;
        }
    }
    Some(shape(format!("{}…", first_line[..ends[fits]].trim_end()).into()))
}

pub(super) fn paint_text(
    text: &str,
    at: gpui::Point<Pixels>,
    face: &Face,
    align: Align,
    max_width: Option<Pixels>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let Some(line) = shape_text(text, face, max_width, window) else {
        return;
    };
    let x = match align {
        Align::Start => at.x,
        Align::Center => at.x - line.width / 2.,
        Align::End => at.x - line.width,
    };
    line.paint(point(x, at.y), face.size * 1.3, TextAlign::Left, None, window, cx)
        .ok();
}
