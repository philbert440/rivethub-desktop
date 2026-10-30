//! One page on the web thread: its offscreen window and web view, the frames it paints, and the
//! input, messages and clipboard text that cross between it and the app.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use gtk::prelude::*;
use gtk::{gdk, glib};
use javascriptcore::ValueExt;
use webkit2gtk::{
    HardwareAccelerationPolicy, LoadEvent, NavigationPolicyDecision, PolicyDecisionType,
    UserContentInjectedFrames, UserContentManager, UserScript, UserScriptInjectionTime, UserStyleLevel,
    UserStyleSheet, WebView,
};
use webkit2gtk::{
    NavigationPolicyDecisionExt, OptionMenuExt, PolicyDecisionExt, SettingsExt, URIRequestExt,
    UserContentManagerExt, WebViewExt,
};

use crate::events;
use crate::ime::Ime;
use crate::keys::Modifiers;
use crate::{Cursor, Frame, Menu, Options, Pointer, Rect, Shared, Size, Update, scheme};

/// What the page sees of its host: `parent.postMessage(message)`, as an iframe would, reaching
/// the app as JSON on the `rivethub` handler.
const BRIDGE: &str = r"(() => {
  const handler = window.webkit.messageHandlers.rivethub;
  const host = Object.freeze({
    postMessage(message) { handler.postMessage(JSON.stringify(message)); },
  });
  Object.defineProperty(window, 'parent', { get: () => host, configurable: false });
})();";

/// The longest message a page may post; a longer one is dropped.
const MESSAGE_LIMIT: usize = 256 * 1024;

pub(crate) struct Opening {
    pub(crate) id: u64,
    pub(crate) options: Options,
    pub(crate) shared: Arc<Shared>,
    pub(crate) updates: UnboundedSender<Update>,
}

pub(crate) enum Command {
    Resize(Size),
    Pointer(Pointer, Modifiers),
    Key {
        press: bool,
        keyval: u32,
        modifiers: Modifiers,
    },
    Focus(bool),
    Post(String),
    Restyle(String),
    Choose(Option<usize>),
    Compose {
        text: String,
        cursor: usize,
    },
    Commit(String),
    Paste(String),
}

/// What the page's view and its signal handlers share.
struct State {
    shared: Arc<Shared>,
    updates: UnboundedSender<Update>,
    size: Cell<Size>,
    capturing: Cell<bool>,
    buttons: Cell<u32>,
    menu: RefCell<Option<webkit2gtk::OptionMenu>>,
    /// The clipboard text the page saw last, so only its own copies go to the app.
    clipboard: RefCell<Option<String>>,
}

impl State {
    fn update(&self, update: Update) {
        self.updates.unbounded_send(update).ok();
    }

    /// Device pixels to the page's logical ones.
    fn logical(&self, pixels: i32) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a panel is far smaller than 2^24 pixels"
        )]
        let pixels = pixels as f32;
        pixels / self.size.get().scale
    }

    fn device(&self, logical: f32) -> f64 {
        f64::from(logical * self.size.get().scale)
    }
}

pub(crate) struct Page {
    /// The page's address, `rhpanel://<host>/<entry>`.
    entry: String,
    window: gtk::OffscreenWindow,
    view: WebView,
    manager: UserContentManager,
    ime: Ime,
    state: Rc<State>,
}

impl Page {
    pub(crate) fn open(context: &webkit2gtk::WebContext, opening: Opening, host: &str) -> Self {
        let Opening {
            options,
            shared,
            updates,
            ..
        } = opening;
        let state = Rc::new(State {
            shared,
            updates,
            size: Cell::new(options.size),
            capturing: Cell::new(false),
            buttons: Cell::new(0),
            menu: RefCell::new(None),
            clipboard: RefCell::new(None),
        });
        let manager = UserContentManager::new();
        manager.add_script(&UserScript::new(
            BRIDGE,
            UserContentInjectedFrames::TopFrame,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
        manager.add_style_sheet(&style_sheet(&options.style));
        manager.register_script_message_handler("rivethub");
        let posted = Rc::clone(&state);
        manager.connect_script_message_received(Some("rivethub"), move |_, result| {
            let Some(json) = result.js_value().map(|value| value.to_str().to_string()) else {
                return;
            };
            if json.len() > MESSAGE_LIMIT {
                tracing::warn!(
                    bytes = json.len(),
                    "a web panel posted a message too long to take"
                );
                return;
            }
            posted.update(Update::Message(json));
        });
        let view: WebView = glib::Object::builder()
            .property("web-context", context)
            .property("user-content-manager", &manager)
            .build();
        if let Some(settings) = WebViewExt::settings(&view) {
            // An offscreen window has no GL surface, so the page paints in software.
            settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
            settings.set_enable_developer_extras(false);
            settings.set_javascript_can_open_windows_automatically(false);
        }
        view.set_background_color(&background(options.background));
        let ime = Ime::new(
            {
                let state = Rc::clone(&state);
                move |editing| {
                    state.shared.editing.store(editing, Ordering::Relaxed);
                    state.update(Update::Caret);
                }
            },
            {
                let state = Rc::clone(&state);
                move |x, y, width, height| {
                    let caret = Rect {
                        x: state.logical(x),
                        y: state.logical(y),
                        width: state.logical(width),
                        height: state.logical(height),
                    };
                    *state.shared.caret.lock().unwrap_or_else(PoisonError::into_inner) = Some(caret);
                    state.update(Update::Caret);
                }
            },
        );
        view.set_input_method_context(Some(ime.as_context()));
        let window = gtk::OffscreenWindow::new();
        window.add(&view);
        let path = &options.entry;
        let separator = if path.starts_with('/') { "" } else { "/" };
        let page = Self {
            entry: format!("{}://{host}{separator}{path}", scheme::SCHEME),
            window,
            view,
            manager,
            ime,
            state,
        };
        page.resize(options.size);
        page.window.show_all();
        page.follow();
        page.view.load_uri(&page.entry);
        page
    }

    /// Connects what the page does to the app.
    fn follow(&self) {
        let state = Rc::clone(&self.state);
        let window = self.window.clone();
        self.window.connect_damage_event(move |_, _| {
            if !state.capturing.replace(true) {
                let state = Rc::clone(&state);
                let window = window.clone();
                glib::idle_add_local_once(move || {
                    state.capturing.set(false);
                    if let Some(frame) = capture(&window, state.size.get())
                        && state.shared.put_frame(frame)
                    {
                        state.update(Update::Frame);
                    }
                });
            }
            false
        });
        let state = Rc::clone(&self.state);
        self.view.connect_load_changed(move |_, event| {
            if event == LoadEvent::Finished {
                state.update(Update::Loaded);
            }
        });
        let entry = self.entry.clone();
        self.view.connect_decide_policy(move |_, decision, kind| {
            // The page stays the document it was opened as: no navigating away, no new windows,
            // and nothing it can't show, such as a download.
            let allowed = match kind {
                PolicyDecisionType::NavigationAction => decision
                    .downcast_ref::<NavigationPolicyDecision>()
                    .and_then(NavigationPolicyDecisionExt::navigation_action)
                    .and_then(|action| action.request())
                    .and_then(|request| request.uri())
                    .is_some_and(|uri| document(&uri) == document(&entry)),
                PolicyDecisionType::Response => decision
                    .downcast_ref::<webkit2gtk::ResponsePolicyDecision>()
                    .is_some_and(webkit2gtk::ResponsePolicyDecisionExt::is_mime_type_supported),
                _ => false,
            };
            if !allowed {
                decision.ignore();
            }
            !allowed
        });
        self.view.connect_context_menu(|_, _, _, _| true);
        let state = Rc::clone(&self.state);
        self.view.connect_show_option_menu(move |_, menu, _, anchor| {
            let items = (0..menu.n_items())
                .filter_map(|index| menu.item(index))
                .map(|mut item| item.label().map(|label| label.to_string()).unwrap_or_default())
                .collect();
            let selected = (0..menu.n_items())
                .find(|&index| menu.item(index).is_some_and(|mut item| item.is_selected()))
                .and_then(|index| usize::try_from(index).ok());
            let anchor = Rect {
                x: state.logical(anchor.x()),
                y: state.logical(anchor.y()),
                width: state.logical(anchor.width()),
                height: state.logical(anchor.height()),
            };
            let closed = Rc::clone(&state);
            menu.connect_close(move |_| {
                if closed.menu.borrow_mut().take().is_some() {
                    closed.update(Update::Menu(None));
                }
            });
            *state.menu.borrow_mut() = Some(menu.clone());
            state.update(Update::Menu(Some(Menu {
                items,
                selected,
                anchor,
            })));
            true
        });
        let state = Rc::clone(&self.state);
        self.view.connect_web_process_terminated(move |_, reason| {
            tracing::warn!(?reason, "a web panel's process stopped");
            state.update(Update::Stopped);
        });
        if let Some(target) = self.view.window() {
            let cursors = Cursors::new();
            let state = Rc::clone(&self.state);
            target.connect_cursor_notify(move |target| {
                state.update(Update::Cursor(cursors.find(target.cursor().as_ref())));
            });
        }
    }

    fn resize(&self, size: Size) {
        self.state.size.set(size);
        let (width, height) = size.device();
        // The window is in device pixels and the page zoomed by the scale, so its CSS pixels are
        // the app's logical ones at any fractional scale. WebKit also multiplies the page's
        // `devicePixelRatio` by the screen scale it finds itself, which only HiDPI screens have.
        self.view.set_zoom_level(f64::from(size.scale));
        self.view.set_size_request(width, height);
        self.window.set_default_size(width, height);
        self.window.resize(width, height);
    }

    pub(crate) fn handle(&self, command: Command) {
        match command {
            Command::Resize(size) => {
                if size != self.state.size.get() {
                    self.resize(size);
                }
            }
            Command::Pointer(pointer, modifiers) => self.pointer(pointer, modifiers),
            Command::Key {
                press,
                keyval,
                modifiers,
            } => {
                if let Some(target) = self.window.window() {
                    events::key(&target, press, keyval, modifiers.state());
                }
                if press && (modifiers.control || modifiers.super_key) {
                    self.watch_clipboard();
                }
            }
            Command::Focus(focused) => {
                if let Some(target) = self.window.window() {
                    events::focus(&target, focused);
                }
                if focused {
                    self.view.grab_focus();
                }
            }
            Command::Post(json) => {
                let script =
                    format!("window.dispatchEvent(new MessageEvent('message', {{ data: {json} }}));");
                self.view
                    .evaluate_javascript(&script, None, None, gtk::gio::Cancellable::NONE, |done| {
                        if let Err(err) = done {
                            tracing::warn!(%err, "posting to a web panel failed");
                        }
                    });
            }
            Command::Restyle(style) => {
                self.manager.remove_all_style_sheets();
                self.manager.add_style_sheet(&style_sheet(&style));
            }
            Command::Choose(item) => {
                // Taken before closing: closing runs the menu's close handler, which looks too.
                let menu = self.state.menu.borrow_mut().take();
                if let Some(menu) = menu {
                    if let Some(index) = item.and_then(|index| u32::try_from(index).ok()) {
                        menu.activate_item(index);
                    }
                    menu.close();
                }
            }
            Command::Compose { text, cursor } => {
                self.ime.compose(text, u32::try_from(cursor).unwrap_or(u32::MAX));
            }
            Command::Commit(text) => self.ime.commit(&text),
            Command::Paste(text) => {
                // The page reads the clipboard through GTK, which only this process sees; it gets
                // the app's text there first.
                gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).set_text(&text);
                self.state.clipboard.replace(Some(text));
                self.view.execute_editing_command("Paste");
            }
        }
    }

    fn pointer(&self, pointer: Pointer, modifiers: Modifiers) {
        let Some(target) = self.view.window() else {
            return;
        };
        let state = &self.state;
        let at = |x: f32, y: f32| (state.device(x), state.device(y));
        let held = modifiers.state() | state.buttons.get();
        match pointer {
            Pointer::Move { x, y } => events::motion(&target, at(x, y), held),
            Pointer::Press { x, y, button } => {
                events::button(&target, true, at(x, y), button, held);
                state
                    .buttons
                    .set(state.buttons.get() | events::button_mask(button));
            }
            Pointer::Release { x, y, button } => {
                events::button(&target, false, at(x, y), button, held);
                state
                    .buttons
                    .set(state.buttons.get() & !events::button_mask(button));
                self.watch_clipboard();
            }
            Pointer::Scroll { x, y, dx, dy } => {
                events::scroll(&target, at(x, y), (f64::from(dx), f64::from(dy)), held);
            }
            Pointer::Leave => events::leave(&target),
        }
    }

    /// After a click or a shortcut that may have copied, looks at the clipboard GTK keeps for the
    /// page, which no other app sees, and hands new text to the app's.
    fn watch_clipboard(&self) {
        for wait in [60, 400] {
            let state = Rc::clone(&self.state);
            glib::timeout_add_local_once(Duration::from_millis(wait), move || {
                gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).request_text(move |_, text| {
                    let Some(text) = text.filter(|text| !text.is_empty()) else {
                        return;
                    };
                    if state.clipboard.borrow().as_deref() != Some(text) {
                        state.clipboard.replace(Some(text.to_owned()));
                        state.update(Update::Copied(text.to_owned()));
                    }
                });
            });
        }
    }

    pub(crate) fn close(self) {
        self.view.terminate_web_process();
        self.window.close();
    }
}

/// The address without its fragment, which a navigation within the page changes.
fn document(uri: &str) -> &str {
    uri.split('#').next().unwrap_or(uri)
}

fn style_sheet(css: &str) -> UserStyleSheet {
    UserStyleSheet::new(
        css,
        UserContentInjectedFrames::TopFrame,
        UserStyleLevel::User,
        &[],
        &[],
    )
}

fn background(rgb: u32) -> gdk::RGBA {
    let channel = |shift: u32| f64::from((rgb >> shift) & 0xff) / 255.;
    gdk::RGBA::new(channel(16), channel(8), channel(0), 1.)
}

/// The page's pixels, when its window has painted any.
fn capture(window: &gtk::OffscreenWindow, size: Size) -> Option<Frame> {
    let surface = gtk::cairo::ImageSurface::try_from(window.surface()?).ok()?;
    let (width, height) = (surface.width(), surface.height());
    let stride = usize::try_from(surface.stride()).ok()?;
    let (columns, rows) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
    if columns == 0 || rows == 0 {
        return None;
    }
    let mut pixels = Vec::with_capacity(columns * rows * 4);
    surface
        .with_data(|data| {
            for row in data.chunks(stride).take(rows) {
                pixels.extend_from_slice(&row[..columns * 4]);
            }
        })
        .ok()?;
    Some(Frame {
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
        scale: size.scale,
        pixels,
    })
}

/// The named cursors `WebKit` sets, found again by identity: GDK hands out one cursor per name.
struct Cursors(Vec<(gdk::Cursor, Cursor)>);

impl Cursors {
    fn new() -> Self {
        const NAMES: [(&str, Cursor); 22] = [
            ("default", Cursor::Default),
            ("pointer", Cursor::Pointer),
            ("text", Cursor::Text),
            ("vertical-text", Cursor::VerticalText),
            ("crosshair", Cursor::Crosshair),
            ("move", Cursor::Move),
            ("grab", Cursor::Grab),
            ("grabbing", Cursor::Grabbing),
            ("not-allowed", Cursor::NotAllowed),
            ("ew-resize", Cursor::ResizeEastWest),
            ("ns-resize", Cursor::ResizeNorthSouth),
            ("nesw-resize", Cursor::ResizeNorthEastSouthWest),
            ("nwse-resize", Cursor::ResizeNorthWestSouthEast),
            ("col-resize", Cursor::ResizeColumn),
            ("row-resize", Cursor::ResizeRow),
            ("n-resize", Cursor::ResizeNorth),
            ("s-resize", Cursor::ResizeSouth),
            ("e-resize", Cursor::ResizeEast),
            ("w-resize", Cursor::ResizeWest),
            ("context-menu", Cursor::ContextMenu),
            ("copy", Cursor::Copy),
            ("alias", Cursor::Alias),
        ];
        let Some(display) = gdk::Display::default() else {
            return Self(Vec::new());
        };
        Self(
            NAMES
                .into_iter()
                .filter_map(|(name, cursor)| Some((gdk::Cursor::from_name(&display, name)?, cursor)))
                .collect(),
        )
    }

    fn find(&self, set: Option<&gdk::Cursor>) -> Cursor {
        set.and_then(|set| self.0.iter().find(|(known, _)| known == set))
            .map_or(Cursor::Default, |(_, cursor)| *cursor)
    }
}
