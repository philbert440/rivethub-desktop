//! Input for a page in an offscreen window, as GDK events made by hand: gtk-rs can't set an
//! event's fields, so they're filled through gdk-sys and dispatched with `gtk_main_do_event`.
#![allow(unsafe_code)]

use std::sync::OnceLock;
use std::time::Instant;

use gtk::gdk;
use gtk::gdk::ffi as gdk_ffi;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;

use webview::Button;

/// Milliseconds on one clock for every event, which `WebKit` uses to count double clicks.
fn now() -> u32 {
    static START: OnceLock<Instant> = OnceLock::new();
    u32::try_from(START.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u32::MAX)
}

fn seat() -> Option<gdk::Seat> {
    gdk::Display::default()?.default_seat()
}

pub(crate) fn button_number(button: Button) -> u32 {
    match button {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 3,
    }
}

/// The state bit GDK sets while `button` is held.
pub(crate) fn button_mask(button: Button) -> u32 {
    1 << (7 + button_number(button))
}

/// A new reference to `window`, which the event owns and `gdk_event_free` drops.
fn owned(window: &gdk::Window) -> *mut gdk_ffi::GdkWindow {
    let raw: *mut gdk_ffi::GdkWindow = window.to_glib_none().0;
    // SAFETY: `raw` is a live GdkWindow borrowed from `window`; the added reference keeps it
    // alive for as long as the event holds it.
    unsafe { gtk::glib::gobject_ffi::g_object_ref(raw.cast()).cast() }
}

/// Fills a new event of `kind` through `fill` and dispatches it.
fn dispatch(
    kind: gdk_ffi::GdkEventType,
    device: Option<gdk::Device>,
    fill: impl FnOnce(*mut gdk_ffi::GdkEvent),
) {
    // SAFETY: `gdk_event_new` returns an owned, zeroed event of `kind`; `fill` writes only the
    // fields of that kind's struct; `gtk_main_do_event` borrows it and `gdk_event_free` frees it
    // with the window reference `owned` gave it. All of it runs on the GTK thread.
    unsafe {
        let event = gdk_ffi::gdk_event_new(kind);
        fill(event);
        if let Some(device) = device {
            gdk_ffi::gdk_event_set_device(event, device.to_glib_none().0);
        }
        gtk::ffi::gtk_main_do_event(event);
        gdk_ffi::gdk_event_free(event);
    }
}

pub(crate) fn button(target: &gdk::Window, press: bool, (x, y): (f64, f64), button: Button, state: u32) {
    let kind = if press {
        gdk_ffi::GDK_BUTTON_PRESS
    } else {
        gdk_ffi::GDK_BUTTON_RELEASE
    };
    dispatch(kind, seat().and_then(|seat| seat.pointer()), |event| {
        // SAFETY: a button event's struct, per `kind`.
        let event = unsafe { &mut *event.cast::<gdk_ffi::GdkEventButton>() };
        event.window = owned(target);
        event.send_event = 1;
        event.time = now();
        (event.x, event.y, event.x_root, event.y_root) = (x, y, x, y);
        event.button = button_number(button);
        event.state = state;
    });
}

pub(crate) fn motion(target: &gdk::Window, (x, y): (f64, f64), state: u32) {
    dispatch(
        gdk_ffi::GDK_MOTION_NOTIFY,
        seat().and_then(|seat| seat.pointer()),
        |event| {
            // SAFETY: a motion event's struct, per its kind.
            let event = unsafe { &mut *event.cast::<gdk_ffi::GdkEventMotion>() };
            event.window = owned(target);
            event.send_event = 1;
            event.time = now();
            (event.x, event.y, event.x_root, event.y_root) = (x, y, x, y);
            event.state = state;
        },
    );
}

/// The pointer leaving the page, so it drops its hover.
pub(crate) fn leave(target: &gdk::Window) {
    dispatch(
        gdk_ffi::GDK_LEAVE_NOTIFY,
        seat().and_then(|seat| seat.pointer()),
        |event| {
            // SAFETY: a crossing event's struct, per its kind.
            let event = unsafe { &mut *event.cast::<gdk_ffi::GdkEventCrossing>() };
            event.window = owned(target);
            event.send_event = 1;
            event.time = now();
            (event.x, event.y) = (-1., -1.);
            event.mode = gdk_ffi::GDK_CROSSING_NORMAL;
            event.detail = gdk_ffi::GDK_NOTIFY_ANCESTOR;
        },
    );
}

pub(crate) fn scroll(target: &gdk::Window, (x, y): (f64, f64), (dx, dy): (f64, f64), state: u32) {
    dispatch(
        gdk_ffi::GDK_SCROLL,
        seat().and_then(|seat| seat.pointer()),
        |event| {
            // SAFETY: a scroll event's struct, per its kind.
            let event = unsafe { &mut *event.cast::<gdk_ffi::GdkEventScroll>() };
            event.window = owned(target);
            event.send_event = 1;
            event.time = now();
            (event.x, event.y, event.x_root, event.y_root) = (x, y, x, y);
            event.state = state;
            event.direction = gdk_ffi::GDK_SCROLL_SMOOTH;
            (event.delta_x, event.delta_y) = (dx, dy);
        },
    );
}

pub(crate) fn focus(target: &gdk::Window, focused: bool) {
    dispatch(
        gdk_ffi::GDK_FOCUS_CHANGE,
        seat().and_then(|seat| seat.keyboard()),
        |event| {
            // SAFETY: a focus event's struct, per its kind.
            let event = unsafe { &mut *event.cast::<gdk_ffi::GdkEventFocus>() };
            event.window = owned(target);
            event.send_event = 1;
            event.in_ = i16::from(focused);
        },
    );
}

pub(crate) fn key(target: &gdk::Window, press: bool, keyval: u32, state: u32) {
    let keycode = gdk::Display::default()
        .and_then(|display| gdk::Keymap::for_display(&display))
        .and_then(|keymap| {
            keymap
                .entries_for_keyval(keyval)
                .first()
                .map(gdk::KeymapKey::keycode)
        })
        .and_then(|code| u16::try_from(code).ok())
        .unwrap_or(0);
    let kind = if press {
        gdk_ffi::GDK_KEY_PRESS
    } else {
        gdk_ffi::GDK_KEY_RELEASE
    };
    dispatch(kind, seat().and_then(|seat| seat.keyboard()), |event| {
        // SAFETY: a key event's struct, per `kind`.
        let event = unsafe { &mut *event.cast::<gdk_ffi::GdkEventKey>() };
        event.window = owned(target);
        event.send_event = 1;
        event.time = now();
        event.state = state;
        event.keyval = keyval;
        event.hardware_keycode = keycode;
    });
}
