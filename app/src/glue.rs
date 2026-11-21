//! Bundles the Kotlin glue (`android/glue/`, a Gradle library) into the Android app.
//!
//! The `manganis::ffi` block below leaves a marker in `libmain.so`. When `dx bundle` finds it, it
//! copies `android/glue/` into its generated Gradle project as `:plugins:lossglue` and adds it as a
//! dependency of the app, so Gradle compiles the Kotlin files and merges their manifest and
//! resources. The Kotlin classes are called by name through JNI (see `platform.rs`), not through
//! the generated `LossGlue` wrapper.

#![allow(dead_code, unused_imports)]

#[manganis::ffi("../android/glue")]
extern "Kotlin" {
    pub type LossGlue;
}
