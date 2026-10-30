mod editing;
mod element;
mod handler;

use std::ops::Range;

use gpui::{
    App, Bounds, Context, CursorStyle, EventEmitter, FocusHandle, Focusable, MouseButton, Pixels,
    SharedString, Window, actions, div, prelude::*, px,
};

use element::{LINE_HEIGHT, Layout, TextElement};

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

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        if self.purpose != Purpose::Message {
            cx.emit(Submitted(self.text().to_owned()));
        } else if let Some(text) = self.take(cx) {
            cx.emit(Submitted(text));
        }
    }
}
