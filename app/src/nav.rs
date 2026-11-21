//! Back-button plumbing between the platform and the UI.
//!
//! The UI tells this module whether there is anything to go back to
//! (`set_can_go_back`, every render). The platform asks `request_back` when
//! the user presses Back: if the UI can go back, the request is queued for
//! the UI task and `true` is returned; otherwise the platform moves the app to
//! the background.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

static CAN_GO_BACK: AtomicBool = AtomicBool::new(false);

struct Channel {
    tx: UnboundedSender<()>,
    rx: Mutex<Option<UnboundedReceiver<()>>>,
}

fn channel() -> &'static Channel {
    static CH: OnceLock<Channel> = OnceLock::new();
    CH.get_or_init(|| {
        let (tx, rx) = unbounded_channel();
        Channel {
            tx,
            rx: Mutex::new(Some(rx)),
        }
    })
}

/// Called by the UI on every render.
pub fn set_can_go_back(v: bool) {
    CAN_GO_BACK.store(v, Ordering::Relaxed);
}

/// Called by the platform when Back is pressed. Returns whether the UI will
/// handle it.
pub fn request_back() -> bool {
    if !CAN_GO_BACK.load(Ordering::Relaxed) {
        return false;
    }
    channel().tx.send(()).is_ok()
}

/// The UI takes the receiving end once, at startup.
pub fn take_back_requests() -> Option<UnboundedReceiver<()>> {
    channel().rx.lock().ok()?.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_is_only_queued_when_the_ui_can_go_back() {
        let mut rx = take_back_requests().expect("receiver");
        set_can_go_back(false);
        assert!(!request_back());
        assert!(rx.try_recv().is_err());
        set_can_go_back(true);
        assert!(request_back());
        assert!(rx.try_recv().is_ok());
    }
}
