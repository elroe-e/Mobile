//! JNI plumbing between Rust and the Kotlin glue (`android/glue`, package `app.loss.glue`).
//!
//! * [`start`] registers `LossNative.onEvent(kind, json)` and `LossNative.back()` with
//!   `RegisterNatives`, then tells the glue it may call them (`LossBridge.nativeReady`), which also
//!   delivers anything that happened before Rust was ready. From then on every event arrives as a
//!   call, so nothing polls Kotlin.
//! * Classes are loaded once through the activity's class loader and kept as global references:
//!   `FindClass` can't see app classes from threads Rust created.
//! * The typed functions at the bottom are the calls Rust makes into `LossBridge`.

use crate::event::Event;
use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM, NativeMethod};
use std::ffi::c_void;
use std::sync::OnceLock;

pub const BRIDGE_CLASS: &str = "app.loss.glue.LossBridge";
pub const NATIVE_CLASS: &str = "app.loss.glue.LossNative";

static VM: OnceLock<JavaVM> = OnceLock::new();
static BRIDGE: OnceLock<GlobalRef> = OnceLock::new();
static NATIVE: OnceLock<GlobalRef> = OnceLock::new();
static HANDLERS: OnceLock<Handlers> = OnceLock::new();

/// What Rust does with calls from Kotlin. Both run on the calling Java thread (often the main
/// thread), so they should only hand the work on (e.g. into a channel).
pub struct Handlers {
    pub event: Box<dyn Fn(Event) + Send + Sync>,
    /// Back was pressed; return whether the app handled it.
    pub back: Box<dyn Fn() -> bool + Send + Sync>,
}

pub fn vm() -> Option<&'static JavaVM> {
    if let Some(vm) = VM.get() {
        return Some(vm);
    }
    let ctx = ndk_context::android_context();
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    Some(VM.get_or_init(|| vm))
}

/// Runs `f` with a JNI env and the activity. Local references are freed when it returns, so it is
/// safe to call in a loop on a long-lived thread. Java exceptions are logged and cleared.
pub fn with_env<T>(f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let vm = vm()?;
    let mut env = vm.attach_current_thread().ok()?;
    let ctx = ndk_context::android_context();
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let out = env.with_local_frame(32, |env| f(env, &activity));
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    // `activity` wraps the NDK's reference; JObject has no Drop, so nothing deletes it.
    out.map_err(|e| log::error!("jni: {e}")).ok()
}

fn load_class<'a>(
    env: &mut JNIEnv<'a>,
    act: &JObject,
    name: &str,
) -> jni::errors::Result<JClass<'a>> {
    let loader = env
        .call_method(act, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string(name)?;
    let cls = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok(JClass::from(cls))
}

fn cached<'a>(
    env: &mut JNIEnv<'a>,
    act: &JObject,
    cell: &OnceLock<GlobalRef>,
    name: &str,
) -> jni::errors::Result<JClass<'a>> {
    if let Some(global) = cell.get() {
        return Ok(JClass::from(env.new_local_ref(global)?));
    }
    let cls = load_class(env, act, name)?;
    let global = env.new_global_ref(&cls)?;
    let _ = cell.set(global);
    Ok(cls)
}

/// `app.loss.glue.LossBridge` (cached).
pub fn bridge_class<'a>(env: &mut JNIEnv<'a>, act: &JObject) -> jni::errors::Result<JClass<'a>> {
    cached(env, act, &BRIDGE, BRIDGE_CLASS)
}

/// Registers the native methods and tells the glue it can call them. Returns false if that
/// failed (the glue then keeps queueing events, and Back falls back to leaving the app).
/// Only the first call does anything.
pub fn start(handlers: Handlers) -> bool {
    if HANDLERS.set(handlers).is_err() {
        return true;
    }
    let ok = with_env(|env, act| {
        let native = cached(env, act, &NATIVE, NATIVE_CLASS)?;
        env.register_native_methods(
            &native,
            &[
                NativeMethod {
                    name: "onEvent".into(),
                    sig: "(Ljava/lang/String;Ljava/lang/String;)V".into(),
                    fn_ptr: on_event as *mut c_void,
                },
                NativeMethod {
                    name: "back".into(),
                    sig: "()Z".into(),
                    fn_ptr: on_back as *mut c_void,
                },
            ],
        )?;
        let bridge = bridge_class(env, act)?;
        env.call_static_method(
            &bridge,
            "nativeReady",
            "(Landroid/content/Context;)V",
            &[JValue::Object(act)],
        )?;
        Ok(())
    })
    .is_some();
    if ok {
        log::info!("platform bridge ready");
    } else {
        log::error!("platform bridge: registering natives failed");
    }
    ok
}

extern "system" fn on_event<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    kind: JString<'local>,
    json: JString<'local>,
) {
    let (Ok(kind), Ok(json)) = (env.get_string(&kind), env.get_string(&json)) else {
        return;
    };
    let (kind, json): (String, String) = (kind.into(), json.into());
    let Some(event) = Event::parse(&kind, &json) else {
        log::warn!("platform event not understood: {kind} {json}");
        return;
    };
    log::debug!("platform event: {event:?}");
    if let Some(h) = HANDLERS.get() {
        // A panic must not cross into Java.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (h.event)(event)));
    }
}

extern "system" fn on_back<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jni::sys::jboolean {
    let handled = HANDLERS
        .get()
        .and_then(|h| std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (h.back)())).ok())
        .unwrap_or(false);
    handled as jni::sys::jboolean
}

// ---- Calls into LossBridge ----

/// What the media notification and lock screen show.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// A cover image file, or empty.
    pub art_path: String,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Keep the audio-focus request while paused (waiting for a call to end).
    pub keep_focus: bool,
}

/// Starts the playback service if needed and refreshes the notification and media session.
pub fn update_playback(n: &NowPlaying) {
    with_env(|env, act| {
        let cls = bridge_class(env, act)?;
        let (t, a, al, art) = (
            env.new_string(&n.title)?,
            env.new_string(&n.artist)?,
            env.new_string(&n.album)?,
            env.new_string(&n.art_path)?,
        );
        env.call_static_method(
            &cls,
            "update",
            "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZJJZ)V",
            &[
                JValue::Object(act),
                JValue::Object(&t),
                JValue::Object(&a),
                JValue::Object(&al),
                JValue::Object(&art),
                JValue::Bool(n.playing as u8),
                JValue::Long(n.position_ms),
                JValue::Long(n.duration_ms),
                JValue::Bool(n.keep_focus as u8),
            ],
        )?;
        Ok(())
    });
}

/// Removes the media notification and stops the playback service.
pub fn stop_playback() {
    with_env(|env, act| {
        let cls = bridge_class(env, act)?;
        env.call_static_method(&cls, "stop", "()V", &[])?;
        Ok(())
    });
}

/// Download notification state: 1 running (foreground service), 2 paused, 0 nothing left.
pub fn update_downloads(state: i32, title: &str, text: &str, progress: i32) {
    with_env(|env, act| {
        let cls = bridge_class(env, act)?;
        let (t, x) = (env.new_string(title)?, env.new_string(text)?);
        env.call_static_method(
            &cls,
            "downloads",
            "(Landroid/content/Context;ILjava/lang/String;Ljava/lang/String;I)V",
            &[
                JValue::Object(act),
                JValue::Int(state),
                JValue::Object(&t),
                JValue::Object(&x),
                JValue::Int(progress),
            ],
        )?;
        Ok(())
    });
}
