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
    pub fn with_env<T>(
        f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> jni::errors::Result<T>,
    ) -> Option<T> {
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
            let f = env
                .call_method(act, "getFilesDir", "()Ljava/io/File;", &[])?
                .l()?;
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

    // ---- Calls from Kotlin (LossNative) ----

    /// `LossNative.command(cmd)`: playback commands from the notification,
    /// lock screen, headset buttons, audio focus and unplug events.
    #[no_mangle]
    pub extern "system" fn Java_app_loss_glue_LossNative_command<'local>(
        mut env: jni::JNIEnv<'local>,
        _class: jni::objects::JClass<'local>,
        cmd: JString<'local>,
    ) {
        let cmd: String = match env.get_string(&cmd) {
            Ok(s) => s.into(),
            Err(_) => return,
        };
        crate::media::apply_command(crate::services::get(), &cmd);
    }

    /// `LossNative.back()`: returns whether the app handled Back itself.
    #[no_mangle]
    pub extern "system" fn Java_app_loss_glue_LossNative_back<'local>(
        _env: jni::JNIEnv<'local>,
        _class: jni::objects::JClass<'local>,
    ) -> jni::sys::jboolean {
        crate::nav::request_back() as jni::sys::jboolean
    }

    /// `app.loss.glue.LossBridge`, loaded through the activity's class loader
    /// (works from threads Rust created, where `FindClass` can't see app classes).
    pub fn bridge_class<'a>(
        env: &mut jni::JNIEnv<'a>,
        act: &JObject,
    ) -> jni::errors::Result<jni::objects::JClass<'a>> {
        let loader = env
            .call_method(act, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
            .l()?;
        let name = env.new_string("app.loss.glue.LossBridge")?;
        let cls = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        Ok(jni::objects::JClass::from(cls))
    }

    pub fn copy_text(label: &str, text: &str) {
        with_env(|env, act| {
            let cls = bridge_class(env, act)?;
            let (l, t) = (env.new_string(label)?, env.new_string(text)?);
            env.call_static_method(
                &cls,
                "copyText",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(act), JValue::Object(&l), JValue::Object(&t)],
            )?;
            Ok(())
        });
    }

    pub fn share_text(subject: &str, text: &str) {
        with_env(|env, act| {
            let cls = bridge_class(env, act)?;
            let (s, t) = (env.new_string(subject)?, env.new_string(text)?);
            env.call_static_method(
                &cls,
                "shareText",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(act), JValue::Object(&s), JValue::Object(&t)],
            )?;
            Ok(())
        });
    }

    fn call_bytes(method: &str, input: &[u8]) -> Option<Vec<u8>> {
        with_env(|env, act| {
            let cls = bridge_class(env, act)?;
            let arr = env.byte_array_from_slice(input)?;
            let out = env
                .call_static_method(&cls, method, "([B)[B", &[JValue::Object(&arr)])?
                .l()?;
            if out.is_null() {
                return Ok(None);
            }
            Ok(Some(
                env.convert_byte_array(jni::objects::JByteArray::from(out))?,
            ))
        })
        .flatten()
    }

    pub fn wrap_key(raw: &[u8]) -> Option<Vec<u8>> {
        call_bytes("wrapKey", raw)
    }

    pub fn unwrap_key(wrapped: &[u8]) -> Option<Vec<u8>> {
        call_bytes("unwrapKey", wrapped)
    }

    pub fn diagnostics() -> Option<String> {
        with_env(|env, act| {
            let cls = bridge_class(env, act)?;
            let r = env
                .call_static_method(
                    &cls,
                    "diagnostics",
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[JValue::Object(act)],
                )?
                .l()?;
            Ok(env.get_string(&JString::from(r))?.into())
        })
    }

    pub fn sdk_int() -> i32 {
        with_env(|env, _| {
            env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
                .i()
        })
        .unwrap_or(0)
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
            env.call_method(
                act,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&p)],
            )?
            .i()
        })
        .map(|r| r == 0)
        .unwrap_or(false)
    }

    const STREAM_MUSIC: i32 = 3;

    fn audio_manager<'a>(
        env: &mut jni::JNIEnv<'a>,
        act: &JObject,
    ) -> jni::errors::Result<JObject<'a>> {
        let name = env.new_string("audio")?;
        env.call_method(
            act,
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[JValue::Object(&name)],
        )?
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
                .call_method(
                    &am,
                    "getStreamMaxVolume",
                    "(I)I",
                    &[JValue::Int(STREAM_MUSIC)],
                )?
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
            let intent = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&action)],
            )?;
            let pkg = env
                .call_method(act, "getPackageName", "()Ljava/lang/String;", &[])?
                .l()?;
            let scheme = env.new_string("package")?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "fromParts",
                    "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
                    &[
                        JValue::Object(&scheme),
                        JValue::Object(&pkg),
                        JValue::Object(&JObject::null()),
                    ],
                )?
                .l()?;
            env.call_method(
                &intent,
                "setData",
                "(Landroid/net/Uri;)Landroid/content/Intent;",
                &[JValue::Object(&uri)],
            )?;
            env.call_method(
                &intent,
                "addFlags",
                "(I)Landroid/content/Intent;",
                &[JValue::Int(0x10000000)],
            )?;
            env.call_method(
                act,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&intent)],
            )?;
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
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
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

/// Whether the platform can put text on the clipboard itself (Android). Elsewhere
/// the UI uses the web view's clipboard.
pub fn copy_text(label: &str, text: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        android::copy_text(label, text);
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (label, text);
        false
    }
}

/// Opens the system share sheet. Returns false where there is none.
pub fn share_text(subject: &str, text: &str) -> bool {
    #[cfg(target_os = "android")]
    {
        android::share_text(subject, text);
        true
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (subject, text);
        false
    }
}

/// The vault's key. On Android it is stored encrypted by an Android Keystore
/// key (`vault.key` holds only the wrapped form); on desktop development
/// builds it is a private file. None if no key can be made or recovered.
pub fn vault_key(data_dir: &std::path::Path) -> Option<[u8; 32]> {
    use loss_core::vault::{new_key, KEY_LEN};
    let path = data_dir.join("vault.key");
    let to_key = |v: Vec<u8>| -> Option<[u8; 32]> { v.try_into().ok() };
    #[cfg(target_os = "android")]
    {
        if let Ok(wrapped) = std::fs::read(&path) {
            if let Some(key) = android::unwrap_key(&wrapped).and_then(to_key) {
                return Some(key);
            }
            log::warn!("vault key can't be unlocked; making a new one");
        }
        let key = new_key().ok()?;
        let wrapped = android::wrap_key(&key)?;
        std::fs::write(&path, wrapped).ok()?;
        let _ = KEY_LEN;
        Some(key)
    }
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(raw) = std::fs::read(&path) {
            if raw.len() == KEY_LEN {
                return to_key(raw);
            }
        }
        let key = new_key().ok()?;
        std::fs::write(&path, key).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Some(key)
    }
}

/// Device and audio-output details (outputs, sample rates, formats, USB DACs).
pub fn diagnostics() -> String {
    #[cfg(target_os = "android")]
    {
        android::diagnostics().unwrap_or_else(|| "Device details unavailable".into())
    }
    #[cfg(not(target_os = "android"))]
    {
        format!(
            "Desktop build: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }
}
