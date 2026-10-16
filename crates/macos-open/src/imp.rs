use std::sync::{Mutex, OnceLock, PoisonError};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, ns_string};

/// `FourCharCode`s from the Apple Event headers (`AEDataModel.h`, `AppleEvents.h`).
const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}
const K_CORE_EVENT_CLASS: u32 = four_cc(b"aevt");
const K_AE_OPEN_DOCUMENTS: u32 = four_cc(b"odoc");
const KEY_DIRECT_OBJECT: u32 = four_cc(b"----");

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
static WAKER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DesignCraftOpenDocumentsHandler"]
    struct Handler;

    impl Handler {
        /// `NSApplicationWillFinishLaunchingNotification`. AppKit has installed its default
        /// Apple Event handlers by now, and the launch-time open-documents event has not been
        /// dispatched yet, so this is where Apple says to replace the handler.
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn application_will_finish_launching(&self, _note: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            // SAFETY: `self` lives for the whole process (leaked in `install`) and implements the
            // selector with the signature the Apple Event manager calls; the class and event ID
            // are `AEEventClass`/`AEEventID` (`FourCharCode`, a `u32`).
            unsafe {
                let _: () = msg_send![
                    &manager,
                    setEventHandler: self,
                    andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                    forEventClass: K_CORE_EVENT_CLASS,
                    andEventID: K_AE_OPEN_DOCUMENTS
                ];
            }
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let paths = document_paths(event);
            if paths.is_empty() {
                return;
            }
            PENDING.lock().unwrap_or_else(PoisonError::into_inner).extend(paths);
            if let Some(wake) = WAKER.get() {
                wake();
            }
        }
    }

    unsafe impl NSObjectProtocol for Handler {}
);

/// The file paths in an open-documents event: its direct object is a list of file references
/// (or, from some senders, a single one).
fn document_paths(event: &NSAppleEventDescriptor) -> Vec<String> {
    // SAFETY: `paramDescriptorForKeyword:` takes an `AEKeyword` (`FourCharCode`, a `u32`) and
    // returns a possibly-nil autoreleased descriptor.
    let direct: Option<Retained<NSAppleEventDescriptor>> = unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
    let Some(direct) = direct else { return Vec::new() };
    let count = direct.numberOfItems();
    let items: Vec<Retained<NSAppleEventDescriptor>> = if count > 0 {
        // Apple Event lists are 1-based.
        (1..=count).filter_map(|i| direct.descriptorAtIndex(i)).collect()
    } else {
        vec![direct]
    };
    // `fileURLValue` coerces aliases, bookmarks and file references to a file URL.
    items.iter().filter_map(|d| d.fileURLValue()).filter_map(|url| url.path()).map(|p| p.to_string()).collect()
}

pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        log_not_main_thread();
        return;
    };
    // SAFETY: plain `init` of an `NSObject` subclass with no instance variables.
    let handler: Retained<Handler> = unsafe { msg_send![Handler::alloc(mtm), init] };
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: `handler` implements `applicationWillFinishLaunching:` taking one `NSNotification`,
    // and outlives the observation (it is leaked below).
    unsafe {
        center.addObserver_selector_name_object(
            &handler,
            sel!(applicationWillFinishLaunching:),
            Some(ns_string!("NSApplicationWillFinishLaunchingNotification")),
            None,
        );
    }
    // Neither the notification centre nor the Apple Event manager retains the handler.
    std::mem::forget(handler);
}

fn log_not_main_thread() {
    eprintln!("designcraft: Finder open-documents handler not installed (not on the main thread)");
}

pub fn set_waker(waker: Box<dyn Fn() + Send + Sync>) {
    let _ = WAKER.set(waker);
}

pub fn take_pending() -> Vec<String> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner))
}
