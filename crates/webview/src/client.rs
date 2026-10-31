//! The app's side of the engine library: finding and loading it, sending it commands, and routing
//! what it says back to the pages it belongs to. The app's fonts are answered here too, so they
//! never cross into the library.
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::ffi::CStr;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use futures::channel::mpsc::UnboundedSender;

use crate::protocol::{self, Event, FromEngine, ToEngine};
use crate::{Error, Face, Fetched, Frame, Request, Shared, Update};

/// Where a page's news goes.
pub(crate) struct Route {
    pub(crate) updates: UnboundedSender<Update>,
    pub(crate) shared: Arc<Shared>,
    pub(crate) faces: Vec<Face>,
}

struct Engine {
    send: protocol::SendFn,
    respond: protocol::RespondFn,
    /// Kept loaded for as long as the app runs, as the functions above live in it.
    _library: libloading::Library,
}

static ENGINE: OnceLock<Result<Engine, Error>> = OnceLock::new();
static ROUTES: OnceLock<Mutex<HashMap<u64, Route>>> = OnceLock::new();

/// Where the app fonts are served at a page's origin.
const FONTS: &str = "/.rivethub/fonts/";

fn routes() -> std::sync::MutexGuard<'static, HashMap<u64, Route>> {
    ROUTES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The library beside the app's binary, as cargo builds it, or in `lib/rivethub-desktop/` beside
/// its `bin/`, as it's installed.
fn library_path() -> Result<PathBuf, Error> {
    let exe = std::env::current_exe().map_err(|err| Error(format!("the app's own path: {err}")))?;
    let dir = exe.parent().map(PathBuf::from).unwrap_or_default();
    let candidates = [
        dir.join(protocol::LIBRARY),
        dir.join("../lib/rivethub-desktop").join(protocol::LIBRARY),
    ];
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            Error(format!(
                "Web panels need {}, which isn't installed beside the app.",
                protocol::LIBRARY
            ))
        })
}

fn load() -> Result<Engine, Error> {
    let path = library_path()?;
    // SAFETY: the library is this app's own engine, built from this workspace; its initialisers
    // only register GTK and WebKit types. Loading fails cleanly when WebKitGTK is missing.
    let library = unsafe { libloading::Library::new(&path) }.map_err(|err| {
        Error(format!(
            "Web panels need WebKitGTK 4.1 (libwebkit2gtk-4.1), which couldn't be loaded: {err}"
        ))
    })?;
    let symbol_error =
        |err: libloading::Error| Error(format!("{} is from another build: {err}", path.display()));
    // SAFETY: each symbol is looked up by the name the library exports it under, with the type
    // `protocol` gives it; the copies stay valid while `_library` keeps the library loaded.
    let (start, send, respond) = unsafe {
        (
            *library
                .get::<protocol::StartFn>(protocol::START)
                .map_err(symbol_error)?,
            *library
                .get::<protocol::SendFn>(protocol::SEND)
                .map_err(symbol_error)?,
            *library
                .get::<protocol::RespondFn>(protocol::RESPOND)
                .map_err(symbol_error)?,
        )
    };
    // SAFETY: `start` takes the protocol version and two functions that live as long as the app,
    // and returns null or a NUL-terminated string the library keeps.
    let refused = unsafe { start(protocol::VERSION, on_message, on_frame) };
    if !refused.is_null() {
        // SAFETY: a non-null answer is a NUL-terminated string that the library keeps alive.
        let why = unsafe { CStr::from_ptr(refused) }.to_string_lossy().into_owned();
        return Err(Error(format!("The web panel engine couldn't start: {why}")));
    }
    Ok(Engine {
        send,
        respond,
        _library: library,
    })
}

fn engine() -> Result<&'static Engine, Error> {
    ENGINE.get_or_init(load).as_ref().map_err(Clone::clone)
}

pub(crate) fn open(panel: u64, route: Route, opening: &ToEngine) -> Result<(), Error> {
    engine()?;
    routes().insert(panel, route);
    send(opening);
    Ok(())
}

pub(crate) fn close(panel: u64) {
    if routes().remove(&panel).is_some() {
        send(&ToEngine::Close { panel });
    }
}

pub(crate) fn send(message: &ToEngine) {
    let (Ok(engine), Ok(json)) = (engine(), serde_json::to_vec(message)) else {
        return;
    };
    // SAFETY: `send` reads `json.len()` bytes from a live buffer during the call.
    unsafe { (engine.send)(json.as_ptr(), json.len()) };
}

pub(crate) fn respond(request: u64, fetched: &Fetched) {
    let Ok(engine) = engine() else {
        return;
    };
    let media_type = fetched.media_type.as_deref().unwrap_or_default();
    // SAFETY: both buffers are live for the call, which copies what it keeps.
    unsafe {
        (engine.respond)(
            request,
            fetched.status,
            media_type.as_ptr(),
            media_type.len(),
            fetched.body.as_ptr(),
            fetched.body.len(),
        );
    }
}

/// `@font-face` rules naming the app's fonts, served at the page's own origin.
pub(crate) fn faces_css(faces: &[Face]) -> String {
    faces.iter().enumerate().fold(String::new(), |mut css, (index, face)| {
        write!(
            css,
            "@font-face{{font-family:\"{}\";font-weight:{};font-style:{};src:url(\"{FONTS}{index}\") format(\"truetype\")}}",
            face.family,
            face.weight,
            if face.italic { "italic" } else { "normal" },
        )
        .ok();
        css
    })
}

/// Answers a request for one of the app's fonts, if it is one.
fn font(route: &Route, request: u64, path: &str) -> bool {
    let Some(index) = path.strip_prefix(FONTS) else {
        return false;
    };
    let fetched = match index
        .parse::<usize>()
        .ok()
        .and_then(|index| route.faces.get(index))
    {
        Some(face) => Fetched {
            status: 200,
            media_type: Some("font/ttf".into()),
            body: face.data.to_vec(),
        },
        None => Fetched {
            status: 404,
            media_type: None,
            body: Vec::new(),
        },
    };
    respond(request, &fetched);
    true
}

fn deliver(message: FromEngine) {
    let routes = routes();
    match message {
        FromEngine::Warning(warning) => tracing::warn!("web panel: {warning}"),
        FromEngine::Fetch {
            panel,
            request,
            method,
            path,
            headers,
            body,
        } => {
            let Some(route) = routes.get(&panel) else {
                return;
            };
            if !font(route, request, &path) {
                route
                    .updates
                    .unbounded_send(Update::Fetch(Request {
                        id: request,
                        method,
                        path,
                        headers,
                        body,
                    }))
                    .ok();
            }
        }
        FromEngine::Event { panel, event } => {
            let Some(route) = routes.get(&panel) else {
                return;
            };
            let update = match event {
                Event::Loaded => Update::Loaded,
                Event::Message(json) => Update::Message(json),
                Event::Cursor(cursor) => Update::Cursor(cursor),
                Event::Menu(menu) => Update::Menu(menu),
                Event::Copied(text) => Update::Copied(text),
                Event::Editing(editing) => {
                    route
                        .shared
                        .editing
                        .store(editing, std::sync::atomic::Ordering::Relaxed);
                    Update::Caret
                }
                Event::Caret(caret) => {
                    *route.shared.caret.lock().unwrap_or_else(PoisonError::into_inner) = Some(caret);
                    Update::Caret
                }
                Event::FocusLeft { forward } => Update::FocusLeft { forward },
                Event::Stopped => Update::Stopped,
            };
            route.updates.unbounded_send(update).ok();
        }
    }
}

/// What the engine says, on its thread.
unsafe extern "C" fn on_message(json: *const u8, len: usize) {
    // SAFETY: the engine passes a buffer of `len` bytes that lives for the call.
    let bytes = unsafe { std::slice::from_raw_parts(json, len) };
    match serde_json::from_slice::<FromEngine>(bytes) {
        Ok(message) => deliver(message),
        Err(err) => tracing::warn!(%err, "the web panel engine said something this app can't read"),
    }
}

/// A frame the engine painted, on its thread: kept as the page's latest, its page told when it
/// had none waiting, so a slow app skips frames rather than queueing them.
unsafe extern "C" fn on_frame(
    panel: u64,
    width: u32,
    height: u32,
    scale: f32,
    pixels: *const u8,
    len: usize,
) {
    // SAFETY: the engine passes `len` bytes of pixels that live for the call.
    let pixels = unsafe { std::slice::from_raw_parts(pixels, len) }.to_vec();
    let routes = routes();
    let Some(route) = routes.get(&panel) else {
        return;
    };
    let frame = Frame {
        width,
        height,
        scale,
        pixels,
    };
    let waiting = route
        .shared
        .frame
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(frame)
        .is_some();
    if !waiting {
        route.updates.unbounded_send(Update::Frame).ok();
    }
}
