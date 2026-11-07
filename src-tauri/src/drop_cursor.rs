//! Who decides the drag cursor above a webview.
//!
//! A button that cannot serve the files in flight is switched off for the length of the
//! gesture, and the cursor says it too: the copy badge AppKit paints beside the pointer
//! *is* the answer to "does this destination want the file", so the refusal belongs there
//! rather than in a mark drawn on top of the button.
//!
//! The page cannot put it there for a file from the file manager: that drag never reaches
//! the DOM, wry takes it, and `draggingUpdated:` reports `NSDragOperation::Copy` whatever
//! the page thinks (`wry/src/wkwebview/drag_drop.rs`). So the verdict is handed down here.
//! Both entry points keep calling wry — the events the page listens for and the drop it
//! accepts are wry's business; only the one word that says "welcome" is ours.
//!
//! Windows needs none of this: there the drag goes through OLE, whose report follows the
//! `dropEffect` the page set, so the cursor already answers `none` when asked.

#[cfg(target_os = "macos")]
use std::ffi::c_void;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(target_os = "macos")]
use std::sync::Once;
#[cfg(target_os = "macos")]
use std::{thread, time::Duration};

use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tauri::Emitter;

/// Whether the button under the pointer refuses the files in flight.
#[cfg(target_os = "macos")]
static REFUSE: AtomicBool = AtomicBool::new(false);
/// One release watcher at a time, and only while a refusal is on screen.
#[cfg(target_os = "macos")]
static WATCHING: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
static INSTALL: Once = Once::new();
/// wry's own implementations, kept so that only the report changes.
#[cfg(target_os = "macos")]
static WRY_ENTERED: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "macos")]
static WRY_UPDATED: AtomicUsize = AtomicUsize::new(0);

/// `NSDragOperationNone`. Anything else tells AppKit the drop is welcome, and the cursor
/// of the session follows that report rather than anything the page renders.
#[cfg(target_os = "macos")]
const OPERATION_NONE: usize = 0;

/// The shape of `draggingEntered:` and `draggingUpdated:`, which both report an
/// `NSDragOperation`: an option set the size of `NSInteger`.
#[cfg(target_os = "macos")]
type DragReport =
    unsafe extern "C-unwind" fn(*mut c_void, objc2::runtime::Sel, *mut c_void) -> usize;

/// Hand the cursor's answer to the native layer.
///
/// The page is the only thing that knows what the drag carries and which button the
/// pointer sits on; this is the one word it has about the cursor. Fire and forget - the
/// drag moves on while the call is in flight, and the newest report is the one that counts.
#[tauri::command]
pub fn set_drop_refusal(app: AppHandle, refused: bool) {
    #[cfg(target_os = "macos")]
    {
        install();
        let was_refusing = REFUSE.swap(refused, Ordering::AcqRel);
        if refused && !was_refusing && !WATCHING.swap(true, Ordering::AcqRel) {
            watch_for_release(app);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // OLE asks this process what the drop is worth on every move, and WebView2's own
        // drag controller forwards the effect the page set, so the cursor already answers
        // `none` where it was asked to. There is nothing to override.
        let _ = (app, refused);
    }
}

/// The end of a refused drag is never delivered to the page: a destination that reports
/// `None` gets no `performDragOperation:`, and no `draggingExited:` either, so its buttons
/// would stay switched off with nothing in the air. The release of the button that started
/// the drag is watched here instead, and only while a refusal is on screen.
#[cfg(target_os = "macos")]
fn watch_for_release(app: AppHandle) {
    thread::spawn(move || {
        // The end of the gesture, told to the page so it can stand its buttons down.
        let finish = || {
            WATCHING.store(false, Ordering::Release);
            // Only a gesture still being refused needs telling: one that moved onto a
            // button that can serve it is over, and its drop is on its way.
            if REFUSE.swap(false, Ordering::AcqRel) {
                let _ = app.emit_to("main", "drop://cancelled", ());
            }
        };
        // A drag presses, so a session where nothing is held is one the page watches end
        // by itself; clearing the refusal here would only cut a working gesture short.
        if !primary_button_held() {
            WATCHING.store(false, Ordering::Release);
            return;
        }
        // The ceiling is a held button for two minutes, which no real gesture reaches.
        for _ in 0..4800 {
            if !REFUSE.load(Ordering::Acquire) || !primary_button_held() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        finish();
    });
}

/// Whether the primary mouse button is held: a drag always is, so its release ends the
/// session.
#[cfg(target_os = "macos")]
fn primary_button_held() -> bool {
    /// `kCGEventSourceStateCombinedSessionState`
    const SESSION: i32 = -1;
    /// `kCGMouseButtonLeft`
    const PRIMARY: i32 = 0;
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceButtonState(state: i32, button: i32) -> bool;
    }
    unsafe { CGEventSourceButtonState(SESSION, PRIMARY) }
}

/// Replace the two report methods of wry's webview class.
///
/// Once, on first use: the class exists only after a window has been built, and changing
/// an implementation is safe from any thread.
#[cfg(target_os = "macos")]
fn install() {
    INSTALL.call_once(|| {
        let Some(class) = wry_web_view_class() else {
            eprintln!("Shift could not find the webview class; refusals stay on the buttons.");
            return;
        };
        let entered = swap(
            class,
            objc2::sel!(draggingEntered:),
            dragging_entered,
            &WRY_ENTERED,
        );
        let updated = swap(
            class,
            objc2::sel!(draggingUpdated:),
            dragging_updated,
            &WRY_UPDATED,
        );
        // The one way to notice that the swap never happened is that the cursor never
        // says no, which is a thing to find in the log rather than by hand.
        if entered && updated {
            eprintln!(
                "Shift answers for the drag cursor: {}",
                class.name().to_string_lossy()
            );
        }
    });
}

/// Replace one method and remember what was there before.
#[cfg(target_os = "macos")]
fn swap(
    class: &'static objc2::runtime::AnyClass,
    selector: objc2::runtime::Sel,
    replacement: DragReport,
    slot: &AtomicUsize,
) -> bool {
    let Some(method) = class.instance_method(selector) else {
        return false;
    };
    let implementation =
        unsafe { std::mem::transmute::<DragReport, objc2::runtime::Imp>(replacement) };
    let previous = unsafe { method.set_implementation(implementation) };
    slot.store(previous as *const c_void as usize, Ordering::Release);
    true
}

/// The class wry builds its webviews from.
///
/// Its name is generated from the module path plus the crate version, so it is looked for
/// rather than written down: an upgrade that only moves the version keeps working, and the
/// parent view of the same module cannot be mistaken for it.
#[cfg(target_os = "macos")]
fn wry_web_view_class() -> Option<&'static objc2::runtime::AnyClass> {
    const NAME: &[u8] = b"wry::wkwebview::class::wry_web_view::WryWebView";
    objc2::runtime::AnyClass::classes()
        .iter()
        .find(|class| class.name().to_bytes().starts_with(NAME))
        .copied()
}

/// A session always begins permissive: the page answers a moment later, and refusing
/// before the carried files are known would waste a drag that works. The verdict is read
/// again after wry has run, so an entry that arrives while the page is still deciding
/// welcomes the file like any other.
#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn dragging_entered(
    this: *mut c_void,
    selector: objc2::runtime::Sel,
    info: *mut c_void,
) -> usize {
    REFUSE.store(false, Ordering::Release);
    let welcome = report(WRY_ENTERED.load(Ordering::Acquire), this, selector, info);
    refuse_if_verdict_says_so(welcome, REFUSE.load(Ordering::Acquire))
}

#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn dragging_updated(
    this: *mut c_void,
    selector: objc2::runtime::Sel,
    info: *mut c_void,
) -> usize {
    let welcome = report(WRY_UPDATED.load(Ordering::Acquire), this, selector, info);
    refuse_if_verdict_says_so(welcome, REFUSE.load(Ordering::Acquire))
}

/// Say no where wry would have said yes, and never the other way round.
///
/// wry's implementation is always called first, and its answer is the one that gets
/// narrowed rather than replaced: that call is what carries `over` on to the page, and a
/// refusal that skipped it would also stop the page from hearing that the pointer had
/// moved. A destination that refuses once and then never hears again cannot change its
/// mind, and a gesture that started on a button which cannot serve the file would be lost
/// even after it moved onto one that can.
#[cfg(target_os = "macos")]
fn refuse_if_verdict_says_so(welcome: usize, refused: bool) -> usize {
    if refused && welcome != OPERATION_NONE {
        OPERATION_NONE
    } else {
        welcome
    }
}

/// Call wry's own implementation, so everything except the report is untouched.
#[cfg(target_os = "macos")]
fn report(
    previous: usize,
    this: *mut c_void,
    selector: objc2::runtime::Sel,
    info: *mut c_void,
) -> usize {
    if previous == 0 {
        return OPERATION_NONE;
    }
    let previous: DragReport = unsafe { std::mem::transmute(previous) };
    unsafe { previous(this, selector, info) }
}
