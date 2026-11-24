//! Loss' platform bridge, the Rust half.
//!
//! * [`event`]: the events Android sends (`LossNative.onEvent(kind, json)` in the Kotlin glue),
//!   parsed into [`Event`].
//! * [`policy`]: what Loss does about audio focus and unplugged headphones. Kotlin only reports;
//!   every decision is made here, where it is tested.
//! * `android` (Android only): JNI plumbing. Cached classes loaded through the activity's class
//!   loader, natives registered with `RegisterNatives`, typed calls into `app.loss.glue.LossBridge`.

pub mod event;
pub mod policy;
pub mod reliability;
pub mod route;

#[cfg(target_os = "android")]
pub mod android;

pub use event::{Event, FocusChange, MediaCommand};
pub use policy::{FocusPolicy, Reaction};
