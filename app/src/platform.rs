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
mod android {
    use jni::objects::{JObject, JString, JValue};
    use jni::JavaVM;

    fn with_env<T>(f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
        let out = f(&mut env, &activity);
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
