//! The page's input method: `WebKit` asks it for the text being composed and tells it where the
//! caret is, while the composing itself happens in the app's input method (fcitx5, `IBus`), which
//! reaches the page through `Ime::compose` and `Ime::commit`. gtk-rs has no subclassing support
//! for `WebKitInputMethodContext`, so its class is filled in here.
#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::c_char;

use gtk::glib;
use gtk::glib::prelude::*;
use gtk::glib::subclass::prelude::*;
use gtk::glib::translate::ToGlibPtr;
use webkit2gtk::ffi;

pub(crate) mod imp {
    use super::{Cell, ObjectImpl, ObjectSubclass, RefCell, glib};

    pub(super) type Focus = Box<dyn Fn(bool)>;
    pub(super) type Caret = Box<dyn Fn(i32, i32, i32, i32)>;

    #[derive(Default)]
    #[expect(
        missing_debug_implementations,
        reason = "it holds callbacks, and glib prints the object"
    )]
    pub struct Ime {
        /// The text being composed, and its cursor in characters.
        pub(super) preedit: RefCell<(String, u32)>,
        pub(super) composing: Cell<bool>,
        pub(super) on_focus: RefCell<Option<Focus>>,
        pub(super) on_caret: RefCell<Option<Caret>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Ime {
        const NAME: &'static str = "RivetHubPanelIme";
        type Type = super::Ime;
        type ParentType = webkit2gtk::InputMethodContext;
    }

    impl ObjectImpl for Ime {}
}

glib::wrapper! {
    pub struct Ime(ObjectSubclass<imp::Ime>) @extends webkit2gtk::InputMethodContext;
}

impl Ime {
    /// `focus` hears when an editable part of the page gains or loses the focus, `caret` where
    /// the caret is in device pixels.
    pub(crate) fn new(focus: impl Fn(bool) + 'static, caret: impl Fn(i32, i32, i32, i32) + 'static) -> Self {
        let ime: Self = glib::Object::new();
        *ime.imp().on_focus.borrow_mut() = Some(Box::new(focus));
        *ime.imp().on_caret.borrow_mut() = Some(Box::new(caret));
        ime
    }

    pub(crate) fn compose(&self, text: String, cursor: u32) {
        let imp = self.imp();
        if text.is_empty() {
            self.end();
            return;
        }
        *imp.preedit.borrow_mut() = (text, cursor);
        if !imp.composing.replace(true) {
            self.emit_by_name::<()>("preedit-started", &[]);
        }
        self.emit_by_name::<()>("preedit-changed", &[]);
    }

    pub(crate) fn commit(&self, text: &str) {
        self.end();
        self.emit_by_name::<()>("committed", &[&text]);
    }

    fn end(&self) {
        let imp = self.imp();
        *imp.preedit.borrow_mut() = (String::new(), 0);
        if imp.composing.replace(false) {
            self.emit_by_name::<()>("preedit-changed", &[]);
            self.emit_by_name::<()>("preedit-finished", &[]);
        }
    }
}

/// The subclass behind a context `WebKit` hands a vfunc.
///
/// # Safety
/// `context` must be an `Ime`, which it is for every context whose class `class_init` filled.
unsafe fn this<'a>(context: *mut ffi::WebKitInputMethodContext) -> &'a imp::Ime {
    // SAFETY: per the contract above, `context` is the instance struct of an `Ime`.
    let instance = unsafe { &*context.cast::<<imp::Ime as ObjectSubclass>::Instance>() };
    instance.imp()
}

unsafe extern "C" fn get_preedit(
    context: *mut ffi::WebKitInputMethodContext,
    text: *mut *mut c_char,
    underlines: *mut *mut glib::ffi::GList,
    cursor: *mut u32,
) {
    // SAFETY: WebKit calls this only on contexts of this class.
    let ime = unsafe { this(context) };
    let (preedit, at) = ime.preedit.borrow().clone();
    // SAFETY: each out pointer is null or writable; the text is handed over owned, as the vfunc
    // contract asks, and an empty list of underlines is a null `GList`.
    unsafe {
        if !text.is_null() {
            *text = preedit.to_glib_full();
        }
        if !underlines.is_null() {
            *underlines = std::ptr::null_mut();
        }
        if !cursor.is_null() {
            *cursor = at;
        }
    }
}

/// Keys reach the page after the app's input method has had them, so none is held back here.
unsafe extern "C" fn filter_key_event(
    _: *mut ffi::WebKitInputMethodContext,
    _: *mut gtk::gdk::ffi::GdkEventKey,
) -> glib::ffi::gboolean {
    glib::ffi::GFALSE
}

unsafe extern "C" fn notify_focus_in(context: *mut ffi::WebKitInputMethodContext) {
    // SAFETY: WebKit calls this only on contexts of this class.
    if let Some(focus) = unsafe { this(context) }.on_focus.borrow().as_ref() {
        focus(true);
    }
}

unsafe extern "C" fn notify_focus_out(context: *mut ffi::WebKitInputMethodContext) {
    // SAFETY: WebKit calls this only on contexts of this class.
    if let Some(focus) = unsafe { this(context) }.on_focus.borrow().as_ref() {
        focus(false);
    }
}

unsafe extern "C" fn notify_cursor_area(
    context: *mut ffi::WebKitInputMethodContext,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    // SAFETY: WebKit calls this only on contexts of this class.
    if let Some(caret) = unsafe { this(context) }.on_caret.borrow().as_ref() {
        caret(x, y, width, height);
    }
}

unsafe extern "C" fn reset(context: *mut ffi::WebKitInputMethodContext) {
    // SAFETY: WebKit calls this only on contexts of this class.
    let ime = unsafe { this(context) };
    *ime.preedit.borrow_mut() = (String::new(), 0);
    ime.composing.set(false);
}

// SAFETY: `class_init` chains to the parent's and then fills only vfuncs of
// `WebKitInputMethodContextClass` with functions of the matching signatures.
unsafe impl IsSubclassable<imp::Ime> for webkit2gtk::InputMethodContext {
    fn class_init(class: &mut glib::Class<Self>) {
        Self::parent_class_init::<imp::Ime>(class);
        let class = class.as_mut();
        class.get_preedit = Some(get_preedit);
        class.filter_key_event = Some(filter_key_event);
        class.notify_focus_in = Some(notify_focus_in);
        class.notify_focus_out = Some(notify_focus_out);
        class.notify_cursor_area = Some(notify_cursor_area);
        class.reset = Some(reset);
    }
}

impl Ime {
    pub(crate) fn as_context(&self) -> &webkit2gtk::InputMethodContext {
        self.upcast_ref()
    }
}
