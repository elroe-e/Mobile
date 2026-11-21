//! The little bit of platform code Loss needs: where to keep data, and
//! asking Android for permission to read the user's music.

use std::path::PathBuf;

pub struct Paths {
    /// Private app data (settings, library index, download queue).
    pub data: PathBuf,
    /// Default folder for downloads.
    pub downloads: PathBuf,
    /// Suggested music folders to scan.
    pub music_suggestions: Vec<PathBuf>,
}

#[cfg(target_os = "android")]
pub mod android {
    use jni::objects::{JObject, JString, JValue};
    use jni::JavaVM;

    /// Runs `f` with a JNI env and the activity. Local references are freed
    /// when it returns, so it is safe to call in a loop on a long-lived thread.
    pub fn with_env<T>(f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
        let out = env.with_local_frame(32, |env| f(env, &activity));
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        // The activity reference is owned by the NDK; don't delete it.
        std::mem::forget(activity);
        out.map_err(|e| log::error!("jni: {e}")).ok()
    }

    fn file_path(env: &mut jni::JNIEnv, file: JObject) -> jni::errors::Result<String> {
        let s = env
            .call_method(&file, "getAbsolutePath", "()Ljava/lang/String;", &[])?
            .l()?;
        Ok(env.get_string(&JString::from(s))?.into())
    }

    pub fn files_dir() -> Option<String> {
        with_env(|env, act| {
            let f = env.call_method(act, "getFilesDir", "()Ljava/io/File;", &[])?.l()?;
            file_path(env, f)
        })
    }

    pub fn external_files_dir() -> Option<String> {
        with_env(|env, act| {
            let f = env
                .call_method(
                    act,
                    "getExternalFilesDir",
                    "(Ljava/lang/String;)Ljava/io/File;",
                    &[JValue::Object(&JObject::null())],
                )?
                .l()?;
            if f.is_null() {
                return Err(jni::errors::Error::NullPtr("external storage unavailable"));
            }
            file_path(env, f)
        })
    }

    pub fn sdk_int() -> i32 {
        with_env(|env, _| env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()).unwrap_or(0)
    }

    fn audio_permission() -> &'static str {
        if sdk_int() >= 33 {
            "android.permission.READ_MEDIA_AUDIO"
        } else {
            "android.permission.READ_EXTERNAL_STORAGE"
        }
    }

    pub fn has_audio_permission() -> bool {
        let perm = audio_permission();
        with_env(|env, act| {
            let p = env.new_string(perm)?;
            env.call_method(act, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&p)])?
                .i()
        })
        .map(|r| r == 0)
        .unwrap_or(false)
    }

    const STREAM_MUSIC: i32 = 3;

    fn audio_manager<'a>(env: &mut jni::JNIEnv<'a>, act: &JObject) -> jni::errors::Result<JObject<'a>> {
        let name = env.new_string("audio")?;
        env.call_method(act, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&name)])?
            .l()
    }

    /// Android media volume as (current step, max step).
    pub fn media_volume() -> Option<(i32, i32)> {
        with_env(|env, act| {
            let am = audio_manager(env, act)?;
            let cur = env
                .call_method(&am, "getStreamVolume", "(I)I", &[JValue::Int(STREAM_MUSIC)])?
                .i()?;
            let max = env
                .call_method(&am, "getStreamMaxVolume", "(I)I", &[JValue::Int(STREAM_MUSIC)])?
                .i()?;
            Ok((cur, max))
        })
    }

    pub fn set_media_volume(step: i32) {
        with_env(|env, act| {
            let am = audio_manager(env, act)?;
            env.call_method(
                &am,
                "setStreamVolume",
                "(III)V",
                &[JValue::Int(STREAM_MUSIC), JValue::Int(step), JValue::Int(0)],
            )?;
            Ok(())
        });
    }

    /// Opens Android's settings page for Loss (to grant access manually).
    pub fn open_app_settings() {
        with_env(|env, act| {
            let action = env.new_string("android.settings.APPLICATION_DETAILS_SETTINGS")?;
            let intent = env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&action)])?;
            let pkg = env.call_method(act, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
            let scheme = env.new_string("package")?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "fromParts",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
                    &[JValue::Object(&scheme), JValue::Object(&pkg), JValue::Object(&JObject::null())],
                )?
                .l()?;
            env.call_method(&intent, "setData", "(Landroid/net/Uri;)Landroid/content/Intent;", &[JValue::Object(&uri)])?;
            env.call_method(&intent, "addFlags", "(I)Landroid/content/Intent;", &[JValue::Int(0x10000000)])?;
            env.call_method(act, "startActivity", "(Landroid/content/Intent;)V", &[JValue::Object(&intent)])?;
            Ok(())
        });
    }

    pub fn request_audio_permission() {
        let perm = audio_permission();
        with_env(|env, act| {
            let arr = env.new_object_array(1, "java/lang/String", JObject::null())?;
            let p = env.new_string(perm)?;
            env.set_object_array_element(&arr, 0, p)?;
            env.call_method(
                act,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&arr), JValue::Int(7)],
            )?;
            Ok(())
        });
    }
}

#[cfg(target_os = "android")]
pub fn paths() -> Paths {
    let data = android::files_dir()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/data/app.loss.player/files"));
    let downloads = android::external_files_dir()
        .map(|p| PathBuf::from(p).join("Music"))
        .unwrap_or_else(|| data.join("Music"));
    Paths {
        data,
        downloads,
        music_suggestions: vec![
            PathBuf::from("/storage/emulated/0/Music"),
            PathBuf::from("/storage/emulated/0/Download"),
        ],
    }
}

#[cfg(not(target_os = "android"))]
pub fn paths() -> Paths {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let data = std::env::var_os("LOSS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share/loss"));
    Paths {
        downloads: home.join("Music/Loss"),
        data,
        music_suggestions: vec![home.join("Music")],
    }
}

pub fn has_audio_permission() -> bool {
    #[cfg(target_os = "android")]
    {
        android::has_audio_permission()
    }
    #[cfg(not(target_os = "android"))]
    {
        true
    }
}

pub fn request_audio_permission() {
    #[cfg(target_os = "android")]
    android::request_audio_permission();
}

/// The phone's media volume (0..1), or None where the app controls its own
/// volume (desktop).
pub fn system_volume() -> Option<f32> {
    #[cfg(target_os = "android")]
    {
        android::media_volume()
            .filter(|(_, max)| *max > 0)
            .map(|(cur, max)| cur as f32 / max as f32)
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

/// Sets the phone's media volume (0..1), rounded to Android's volume steps.
pub fn set_system_volume(v: f32) {
    #[cfg(target_os = "android")]
    if let Some((_, max)) = android::media_volume() {
        android::set_media_volume((v.clamp(0.0, 1.0) * max as f32).round() as i32);
    }
    #[cfg(not(target_os = "android"))]
    let _ = v;
}

pub fn open_app_settings() {
    #[cfg(target_os = "android")]
    android::open_app_settings();
}

pub fn init_logging() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("Loss"),
    );
}
