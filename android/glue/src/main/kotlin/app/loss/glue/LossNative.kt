// Native methods implemented in Rust (crates/loss-platform/src/android.rs). Rust registers them
// with RegisterNatives at startup and then calls LossBridge.nativeReady; until then calling them
// throws UnsatisfiedLinkError, so use LossBridge.emit, which queues.
// Kotlin only forwards to Rust; decisions live in Rust.

package app.loss.glue

object LossNative {
    /**
     * Something happened that Rust should know about. Kinds: media {cmd}, focus {change},
     * noisy {}, outputs_added / outputs_removed {types}, volume {level, max}.
     */
    @JvmStatic external fun onEvent(kind: String, json: String)

    /** Back was pressed. Returns false when the app has nothing to go back to. */
    @JvmStatic external fun back(): Boolean
}
