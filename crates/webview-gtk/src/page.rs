//! One page on the web thread: its offscreen window and web view, the frames it paints, and the
//! input, messages and clipboard text that cross between it and the app.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, glib};
use javascriptcore::ValueExt;
use webkit2gtk::{
    HardwareAccelerationPolicy, LoadEvent, NavigationPolicyDecision, NavigationPolicyDecisionExt,
    OptionMenuExt, PolicyDecisionExt, PolicyDecisionType, SettingsExt, URIRequestExt,
    UserContentInjectedFrames, UserContentManager, UserContentManagerExt, UserScript,
    UserScriptInjectionTime, UserStyleLevel, UserStyleSheet, WebProcessTerminationReason, WebView,
    WebViewExt,
};
use webview::protocol::{Command, Event};
use webview::{Cursor, Menu, Modifiers, Pointer, Rect, Size};

use crate::events;
use crate::ime::Ime;
use crate::scheme;

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

/// How many times a page's process may stop within `CRASH_WINDOW` and be started again.
const RESTARTS: usize = 2;
const CRASH_WINDOW: Duration = Duration::from_secs(60);

/// What the page's view and its signal handlers share.
struct State {
    panel: u64,
    size: Cell<Size>,
    capturing: Cell<bool>,
    buttons: Cell<u32>,
    menu: RefCell<Option<webkit2gtk::OptionMenu>>,
    /// The clipboard text the page saw last, so only its own copies go to the app.
    clipboard: RefCell<Option<String>>,
    /// When the page's process stopped lately.
    crashes: RefCell<Vec<Instant>>,
}

impl State {
    fn event(&self, event: Event) {
        crate::event(self.panel, event);
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

    fn rect(&self, x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x: self.logical(x),
            y: self.logical(y),
            width: self.logical(width),
            height: self.logical(height),
        }
    }

    /// Whether a page whose process just stopped gets another: a few within a minute do.
    fn may_restart(&self) -> bool {
        let mut crashes = self.crashes.borrow_mut();
        crashes.retain(|at| at.elapsed() < CRASH_WINDOW);
        crashes.push(Instant::now());
        crashes.len() <= RESTARTS
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
    pub(crate) fn open(
        context: &webkit2gtk::WebContext,
        panel: u64,
        host: &str,
        entry: &str,
        style: &str,
        background: u32,
        size: Size,
    ) -> Self {
        let state = Rc::new(State {
            panel,
            size: Cell::new(size),
            capturing: Cell::new(false),
            buttons: Cell::new(0),
            menu: RefCell::new(None),
            clipboard: RefCell::new(None),
            crashes: RefCell::new(Vec::new()),
        });
        let manager = UserContentManager::new();
        manager.add_script(&UserScript::new(
            BRIDGE,
            UserContentInjectedFrames::TopFrame,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
        manager.add_style_sheet(&style_sheet(style));
        manager.register_script_message_handler("rivethub");
        let posted = Rc::clone(&state);
        manager.connect_script_message_received(Some("rivethub"), move |_, result| {
            let Some(json) = result.js_value().map(|value| value.to_str().to_string()) else {
                return;
            };
            if json.len() > MESSAGE_LIMIT {
                crate::warn(format!(
                    "a page posted a message of {} bytes, too long to take",
                    json.len()
                ));
                return;
            }
            posted.event(Event::Message(json));
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
        view.set_background_color(&background_color(background));
        let ime = Ime::new(
            {
                let state = Rc::clone(&state);
                move |editing| state.event(Event::Editing(editing))
            },
            {
                let state = Rc::clone(&state);
                move |x, y, width, height| state.event(Event::Caret(state.rect(x, y, width, height)))
            },
        );
        view.set_input_method_context(Some(ime.as_context()));
        let window = gtk::OffscreenWindow::new();
        window.add(&view);
        let separator = if entry.starts_with('/') { "" } else { "/" };
        let page = Self {
            entry: format!("{}://{host}{separator}{entry}", scheme::SCHEME),
            window,
            view,
            manager,
            ime,
            state,
        };
        page.resize(size);
        page.window.show_all();
        // Before it's realized GTK gives a widget the first monitor's scale, and WebKit keeps the
        // scale it read then, rendering a HiDPI monitor's multiple of the pixels drawn here. Now
        // the view has its window's scale; asking again makes WebKit read it.
        page.view.notify("scale-factor");
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
                    capture(&window, state.panel, state.size.get().scale);
                });
            }
            false
        });
        // The view gives up the keyboard through its window when Tab leaves the page's last
        // element (Shift+Tab its first): the app takes it on, and GTK doesn't wrap it around.
        let state = Rc::clone(&self.state);
        self.window.connect_focus(move |_, direction| {
            match direction {
                gtk::DirectionType::TabForward => state.event(Event::FocusLeft { forward: true }),
                gtk::DirectionType::TabBackward => state.event(Event::FocusLeft { forward: false }),
                _ => {}
            }
            glib::Propagation::Stop
        });
        let state = Rc::clone(&self.state);
        self.view.connect_load_changed(move |_, event| {
            if event == LoadEvent::Finished {
                state.event(Event::Loaded);
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
            let anchor = state.rect(anchor.x(), anchor.y(), anchor.width(), anchor.height());
            let closed = Rc::clone(&state);
            menu.connect_close(move |_| {
                if closed.menu.borrow_mut().take().is_some() {
                    closed.event(Event::Menu(None));
                }
            });
            *state.menu.borrow_mut() = Some(menu.clone());
            state.event(Event::Menu(Some(Menu {
                items,
                selected,
                anchor,
            })));
            true
        });
        let state = Rc::clone(&self.state);
        self.view.connect_web_process_terminated(move |view, reason| {
            if reason == WebProcessTerminationReason::TerminatedByApi {
                return;
            }
            if state.may_restart() {
                crate::warn(format!(
                    "a page's process stopped ({reason:?}); starting it again"
                ));
                let view = view.clone();
                glib::idle_add_local_once(move || view.reload());
            } else {
                crate::warn(format!(
                    "a page's process stopped ({reason:?}) again; leaving it stopped"
                ));
                state.event(Event::Stopped);
            }
        });
        if let Some(target) = self.view.window() {
            let cursors = Cursors::new();
            let state = Rc::clone(&self.state);
            target.connect_cursor_notify(move |target| {
                state.event(Event::Cursor(cursors.find(target.cursor().as_ref())));
            });
        }
    }

    fn resize(&self, size: Size) {
        self.state.size.set(size);
        let (width, height) = size.device();
        // The window is in device pixels and the page zoomed by the scale, so its CSS pixels are
        // the app's logical ones and its `devicePixelRatio` the scale, fractional or not.
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
            Command::Enter => {
                if let Some(target) = self.window.window() {
                    events::focus(&target, true);
                }
                self.view.child_focus(gtk::DirectionType::TabForward);
            }
            Command::Post(json) => {
                let script =
                    format!("window.dispatchEvent(new MessageEvent('message', {{ data: {json} }}));");
                self.view
                    .evaluate_javascript(&script, None, None, gtk::gio::Cancellable::NONE, |done| {
                        if let Err(err) = done {
                            crate::warn(format!("posting to a page failed: {err}"));
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
            Command::Reload => {
                self.state.crashes.borrow_mut().clear();
                self.view.reload();
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
                        state.event(Event::Copied(text.to_owned()));
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

fn background_color(rgb: u32) -> gdk::RGBA {
    let channel = |shift: u32| f64::from((rgb >> shift) & 0xff) / 255.;
    gdk::RGBA::new(channel(16), channel(8), channel(0), 1.)
}

/// Hands the app the page's pixels, when its window has painted any.
fn capture(window: &gtk::OffscreenWindow, panel: u64, scale: f32) {
    let Some(surface) = window
        .surface()
        .and_then(|surface| gtk::cairo::ImageSurface::try_from(surface).ok())
    else {
        return;
    };
    let (Ok(width), Ok(height), Ok(stride)) = (
        u32::try_from(surface.width()),
        u32::try_from(surface.height()),
        usize::try_from(surface.stride()),
    ) else {
        return;
    };
    let (Ok(columns), Ok(rows)) = (usize::try_from(width), usize::try_from(height)) else {
        return;
    };
    if columns == 0 || rows == 0 {
        return;
    }
    let mut pixels = Vec::with_capacity(columns * rows * 4);
    let copied = surface.with_data(|data| {
        for row in data.chunks(stride).take(rows) {
            pixels.extend_from_slice(&row[..columns * 4]);
        }
    });
    if copied.is_ok() {
        crate::frame(panel, width, height, scale, &pixels);
    }
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
