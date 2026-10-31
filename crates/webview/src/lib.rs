//! A web page drawn offscreen by `WebKitGTK`, which the app shows as pictures and hands its
//! input to. `WebKit`'s own windows can't be children of a Wayland surface, so the page never
//! gets one: it renders into a `GtkOffscreenWindow` on a thread of its own that runs GTK's main
//! loop, and its frames, cursor, caret and messages cross to the app.
//!
//! `WebKitGTK` lives in a library of its own, `librivethub_webview.so`, loaded the first time a
//! page opens, so the app starts and runs without it and a page says why it can't show instead.
//!
//! Each page lives at an origin of its own, `rhpanel://<host>/`, and the app answers its every
//! request (`Update::Fetch`); nothing else is reachable from it.

mod client;
mod keys;
#[doc(hidden)]
pub mod protocol;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use futures::channel::mpsc::UnboundedReceiver;
use serde::{Deserialize, Serialize};

pub use keys::Modifiers;

/// A request the page made of its origin, for the app to answer with `Panel::respond`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub method: String,
    /// The path at the origin, with its query.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The app's answer to a `Request`. Only its media type of all its headers reaches the page; the
/// page's own security policy goes with every answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub status: u16,
    pub media_type: Option<String>,
    pub body: Vec<u8>,
}

/// A size in logical pixels and the scale it is shown at.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

impl Size {
    /// The size in device pixels, at least one each way.
    pub fn device(self) -> (i32, i32) {
        let device = |logical: f32| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a panel is far smaller than i32::MAX"
            )]
            let pixels = (logical * self.scale).round() as i32;
            pixels.max(1)
        };
        (device(self.width), device(self.height))
    }
}

/// A rectangle in the page's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// One of the app's fonts, which the page can use by its family name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Face {
    pub family: &'static str,
    pub weight: u16,
    pub italic: bool,
    /// A TrueType file.
    pub data: &'static [u8],
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Names the page's origin; pages opened with the same one share it.
    pub origin: String,
    /// The page's path at its origin, with any query.
    pub entry: String,
    /// CSS the document gets before its own.
    pub style: String,
    /// The fonts the page may use, served at its origin under `/.rivethub/fonts/`.
    pub faces: Vec<Face>,
    /// The colour shown until the page paints its own, as `0xRRGGBB`.
    pub background: u32,
    pub size: Size,
}

/// One picture of the page: BGRA with premultiplied alpha, `width * 4` bytes a row.
#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Device pixels per logical pixel.
    pub scale: f32,
    pub pixels: Vec<u8>,
}

/// The pointer shapes a page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Cursor {
    #[default]
    Default,
    Pointer,
    Text,
    VerticalText,
    Crosshair,
    Move,
    Grab,
    Grabbing,
    NotAllowed,
    ResizeEastWest,
    ResizeNorthSouth,
    ResizeNorthEastSouthWest,
    ResizeNorthWestSouthEast,
    ResizeColumn,
    ResizeRow,
    ResizeNorth,
    ResizeSouth,
    ResizeEast,
    ResizeWest,
    ContextMenu,
    Copy,
    Alias,
}

/// A `<select>` list the page opened, for the app to draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Menu {
    pub items: Vec<String>,
    pub selected: Option<usize>,
    /// The `<select>` it belongs to.
    pub anchor: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// A new frame waits in `Panel::take_frame`.
    Frame,
    /// The page finished loading, the first time or again.
    Loaded,
    /// The page posted this, as JSON.
    Message(String),
    Cursor(Cursor),
    /// A `<select>` opened its list, or `None` when it closed.
    Menu(Option<Menu>),
    /// The page put this text on its clipboard.
    Copied(String),
    /// The caret moved, or editing started or stopped.
    Caret,
    /// The page asks its origin for this.
    Fetch(Request),
    /// Tab moved the focus past the page's last element, or Shift+Tab past its first.
    FocusLeft {
        forward: bool,
    },
    /// The page's process kept stopping, so it isn't started again.
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Button {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Pointer {
    Move { x: f32, y: f32 },
    Press { x: f32, y: f32, button: Button },
    Release { x: f32, y: f32, button: Button },
    Scroll { x: f32, y: f32, dx: f32, dy: f32 },
    Leave,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Error(String);

/// What the engine reports and the app reads.
#[derive(Debug, Default)]
struct Shared {
    frame: Mutex<Option<Frame>>,
    /// Whether an editable part of the page has the focus, so typing is text.
    editing: AtomicBool,
    /// Where the caret is, in logical pixels.
    caret: Mutex<Option<Rect>>,
}

/// A web page on screen, held by the app. Dropping it closes the page.
#[derive(Debug)]
pub struct Panel {
    id: u64,
    shared: Arc<Shared>,
    faces: Vec<Face>,
}

impl Panel {
    /// Opens a page; what it does arrives on the receiver. Fails when the engine or `WebKitGTK`
    /// can't be loaded, saying which.
    pub fn open(options: Options) -> Result<(Self, UnboundedReceiver<Update>), Error> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(Shared::default());
        let (updates, received) = futures::channel::mpsc::unbounded();
        let style = client::faces_css(&options.faces) + &options.style;
        client::open(
            id,
            client::Route {
                updates,
                shared: Arc::clone(&shared),
                faces: options.faces.clone(),
            },
            &protocol::ToEngine::Open {
                panel: id,
                origin: options.origin,
                entry: options.entry,
                style,
                background: options.background,
                size: options.size,
            },
        )?;
        Ok((
            Self {
                id,
                shared,
                faces: options.faces,
            },
            received,
        ))
    }

    fn send(&self, command: protocol::Command) {
        client::send(&protocol::ToEngine::To {
            panel: self.id,
            command,
        });
    }

    pub fn resize(&self, size: Size) {
        self.send(protocol::Command::Resize(size));
    }

    pub fn pointer(&self, pointer: Pointer, modifiers: Modifiers) {
        self.send(protocol::Command::Pointer(pointer, modifiers));
    }

    /// Hands the page a key by the name the app's toolkit gives it, and the text it types.
    /// Returns `false` for a key the page has no name for, which the app keeps.
    pub fn key(&self, press: bool, key: &str, text: Option<&str>, modifiers: Modifiers) -> bool {
        let Some(keyval) = keys::keyval(key, text, modifiers) else {
            return false;
        };
        self.send(protocol::Command::Key {
            press,
            keyval,
            modifiers,
        });
        true
    }

    pub fn focus(&self, focused: bool) {
        self.send(protocol::Command::Focus(focused));
    }

    /// Gives the keyboard to the page's first element, as Tab coming into it does, or to its last
    /// for Shift+Tab.
    pub fn enter(&self, forward: bool) {
        self.send(protocol::Command::Enter { forward });
    }

    /// Delivers `json` to the page as a `message` event's data.
    pub fn post(&self, json: String) {
        self.send(protocol::Command::Post(json));
    }

    /// Replaces the CSS given at open.
    pub fn restyle(&self, style: &str) {
        self.send(protocol::Command::Restyle(client::faces_css(&self.faces) + style));
    }

    /// Picks an item of the open `<select>` list, or closes it with `None`.
    pub fn choose(&self, item: Option<usize>) {
        self.send(protocol::Command::Choose(item));
    }

    /// Shows text an input method is composing at the caret; empty text ends it.
    pub fn compose(&self, text: String, cursor: usize) {
        self.send(protocol::Command::Compose { text, cursor });
    }

    /// Types text an input method settled on.
    pub fn commit(&self, text: String) {
        self.send(protocol::Command::Commit(text));
    }

    /// Pastes `text` where the page's caret is, as a paste from the clipboard.
    pub fn paste(&self, text: String) {
        self.send(protocol::Command::Paste(text));
    }

    /// Loads the page again, in a new process if its last one stopped.
    pub fn reload(&self) {
        self.send(protocol::Command::Reload);
    }

    /// Answers one of the page's requests.
    pub fn respond(&self, request: u64, fetched: &Fetched) {
        client::respond(request, fetched);
    }

    pub fn take_frame(&self) -> Option<Frame> {
        self.shared
            .frame
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    pub fn editing(&self) -> bool {
        self.shared.editing.load(Ordering::Relaxed)
    }

    pub fn caret(&self) -> Option<Rect> {
        *self.shared.caret.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        client::close(self.id);
    }
}
