//! The host owns shutdown. The view can request it, never bypass app vetoes.
use octosense_updater::macos::Prepared;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

static PREPARED: Mutex<Option<Prepared>> = Mutex::new(None);
static REQUEST: AtomicBool = AtomicBool::new(false);

pub(crate) fn request(prepared: Prepared) {
    *PREPARED.lock().unwrap_or_else(|e| e.into_inner()) = Some(prepared);
    REQUEST.store(true, Ordering::Release);
}
pub fn take_request() -> bool {
    REQUEST.swap(false, Ordering::AcqRel)
}
pub fn pending() -> bool {
    PREPARED.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}
pub fn cancel() {
    REQUEST.store(false, Ordering::Release);
    PREPARED.lock().unwrap_or_else(|e| e.into_inner()).take();
}
pub fn commit() -> Result<(), String> {
    PREPARED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| "The update was cancelled before shutdown.".to_string())?
        .commit()
}
pub fn acknowledge_startup() {
    octosense_updater::macos::acknowledge_startup();
}

/// The helper sets this fixed signal only when it relaunches the previous app.
/// A failed update should be visible without making the person inspect logs.
pub fn take_rollback_notice() -> bool {
    static SEEN: AtomicBool = AtomicBool::new(false);
    !SEEN.swap(true, Ordering::AcqRel)
        && std::env::var("OCTOSENSE_UPDATE_ROLLED_BACK").as_deref() == Ok("1")
}
