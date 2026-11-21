// Native methods implemented in Rust (app/src/platform.rs, exported from libmain.so).
// Kotlin only forwards to Rust; decisions live in Rust.

package app.loss.glue



/** Calls into Rust (libmain.so, loaded by WryActivity). */
object LossNative {
    /** A playback command: play, pause, toggle, next, prev, "seek <ms>". */
    @JvmStatic external fun command(cmd: String)

    /** Back was pressed. Returns false when the app has nothing to go back to. */
    @JvmStatic external fun back(): Boolean
}
