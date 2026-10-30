//! The person's pointer, keys and input method, handed to a web panel's page.

use std::ops::Range;

use gpui::{
    App, Bounds, Context, DispatchPhase, ElementInputHandler, Entity, EntityInputHandler, Hitbox,
    KeyDownEvent, KeyUpEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    ScrollDelta, ScrollWheelEvent, UTF16Selection, Window, point, px, size,
};
use webview::{Button, Modifiers, Panel, Pointer};

use super::WebPanelView;

/// How far one step of GDK's smooth scrolling moves a page, in pixels, as `WebKit` counts it.
const SCROLL_STEP: f32 = 40.;

fn modifiers(held: gpui::Modifiers) -> Modifiers {
    Modifiers {
        shift: held.shift,
        control: held.control,
        alt: held.alt,
        super_key: held.platform,
    }
}

fn button(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Middle => Some(Button::Middle),
        MouseButton::Right => Some(Button::Right),
        MouseButton::Navigate(_) => None,
    }
}

/// Ctrl+V, Ctrl+Shift+V and Shift+Insert: the page gets the app's clipboard, which it can't read.
fn pastes(keystroke: &Keystroke) -> bool {
    let held = &keystroke.modifiers;
    (keystroke.key == "v" && held.control && !held.alt)
        || (keystroke.key == "insert" && held.shift && !held.control)
}

/// Registers the panel's pointer handlers and input method for this frame.
pub(super) fn listen(
    view: &Entity<WebPanelView>,
    hitbox: &Hitbox,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let local = move |at: gpui::Point<Pixels>| {
        (
            f32::from(at.x - bounds.origin.x),
            f32::from(at.y - bounds.origin.y),
        )
    };
    window.on_mouse_event({
        let (view, hitbox) = (view.clone(), hitbox.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            let Some(pressed) = button(event.button) else {
                return;
            };
            view.update(cx, |this, cx| {
                window.focus(&this.focus, cx);
                this.pressed = true;
                let (x, y) = local(event.position);
                this.pointer(
                    Pointer::Press {
                        x,
                        y,
                        button: pressed,
                    },
                    event.modifiers,
                );
            });
            cx.stop_propagation();
        }
    });
    window.on_mouse_event({
        let (view, hitbox) = (view.clone(), hitbox.clone());
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let over = hitbox.is_hovered(window);
            view.update(cx, |this, _| {
                if over || this.pressed {
                    let (x, y) = local(event.position);
                    this.pointer(Pointer::Move { x, y }, event.modifiers);
                } else if std::mem::take(&mut this.hovered) {
                    this.pointer(Pointer::Leave, event.modifiers);
                }
                this.hovered |= over;
            });
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let Some(released) = button(event.button) else {
                return;
            };
            view.update(cx, |this, _| {
                if std::mem::take(&mut this.pressed) {
                    let (x, y) = local(event.position);
                    this.pointer(
                        Pointer::Release {
                            x,
                            y,
                            button: released,
                        },
                        event.modifiers,
                    );
                }
            });
        }
    });
    scrolls(view, hitbox, local, window);
    let focus = view.read(cx).focus.clone();
    window.handle_input(&focus, ElementInputHandler::new(bounds, view.clone()), cx);
}

/// Wheel and touchpad scrolling over the panel, as GDK's smooth scroll steps.
fn scrolls(
    view: &Entity<WebPanelView>,
    hitbox: &Hitbox,
    local: impl Fn(gpui::Point<Pixels>) -> (f32, f32) + 'static,
    window: &mut Window,
) {
    window.on_mouse_event({
        let (view, hitbox) = (view.clone(), hitbox.clone());
        move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                return;
            }
            let (dx, dy) = match event.delta {
                ScrollDelta::Pixels(delta) => {
                    (f32::from(delta.x) / SCROLL_STEP, f32::from(delta.y) / SCROLL_STEP)
                }
                ScrollDelta::Lines(delta) => (delta.x, delta.y),
            };
            let (x, y) = local(event.position);
            view.update(cx, |this, _| {
                this.pointer(
                    Pointer::Scroll {
                        x,
                        y,
                        dx: -dx,
                        dy: -dy,
                    },
                    event.modifiers,
                );
            });
            cx.stop_propagation();
        }
    });
}

impl WebPanelView {
    fn pointer(&self, pointer: Pointer, held: gpui::Modifiers) {
        if let Some(panel) = &self.panel {
            panel.pointer(pointer, modifiers(held));
        }
    }

    pub(super) fn watch_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.watch = vec![
            cx.on_focus(&self.focus, window, |this, _, _| {
                if let Some(panel) = &this.panel {
                    panel.focus(true);
                }
            }),
            cx.on_blur(&self.focus, window, |this, _, _| {
                if let Some(panel) = &this.panel {
                    panel.focus(false);
                }
            }),
        ];
    }

    pub(super) fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.panel else {
            return;
        };
        let keystroke = &event.keystroke;
        if pastes(keystroke) {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                panel.paste(text);
            }
            cx.stop_propagation();
            return;
        }
        if panel.key(
            true,
            &keystroke.key,
            keystroke.key_char.as_deref(),
            modifiers(keystroke.modifiers),
        ) {
            cx.stop_propagation();
        }
    }

    pub(super) fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.panel else {
            return;
        };
        let keystroke = &event.keystroke;
        if pastes(keystroke)
            || panel.key(
                false,
                &keystroke.key,
                keystroke.key_char.as_deref(),
                modifiers(keystroke.modifiers),
            )
        {
            cx.stop_propagation();
        }
    }

    fn marked_len(&self) -> usize {
        self.marked
            .as_deref()
            .map_or(0, |text| text.encode_utf16().count())
    }
}

/// The characters before UTF-16 offset `offset` of `text`, as `WebKit` counts a preedit cursor.
fn characters(text: &str, offset: usize) -> usize {
    let mut units = 0;
    text.chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= offset
        })
        .count()
}

/// What the app's input method (fcitx5, `IBus`) composes and settles on goes to the page as its
/// own preedit and text; keys it doesn't take arrive as key events.
impl EntityInputHandler for WebPanelView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.marked_len();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|_| 0..self.marked_len())
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        if let (Some(text), Some(panel)) = (self.marked.take(), &self.panel) {
            panel.commit(text);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        let was_marked = self.marked.take().is_some();
        let Some(panel) = &self.panel else {
            return;
        };
        if !text.is_empty() {
            panel.commit(text.to_owned());
        } else if was_marked && range.is_some() {
            panel.compose(String::new(), 0);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
        let cursor = selected.map_or_else(|| text.chars().count(), |selected| characters(text, selected.end));
        self.marked = (!text.is_empty()).then(|| text.to_owned());
        if let Some(panel) = &self.panel {
            panel.compose(text.to_owned(), cursor);
        }
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let caret = self.panel.as_ref().and_then(Panel::caret)?;
        Some(Bounds::new(
            element_bounds.origin + point(px(caret.x), px(caret.y)),
            size(px(caret.width.max(1.)), px(caret.height)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.panel.as_ref().is_some_and(Panel::editing)
    }
}
