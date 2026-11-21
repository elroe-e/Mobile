// Loss' Android activity. Everything else in the app is Rust; the rest of the Kotlin glue
// (media service, download service, bridge helpers) lives in android/glue/, a Gradle library
// that dx bundles into the APK (see app/src/glue.rs).

package dev.dioxus.main

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.KeyEvent
import app.loss.glue.LossBridge
import app.loss.glue.LossNative

typealias BuildConfig = app.loss.player.BuildConfig

class MainActivity : WryActivity() {
    // Back leaves the app running (music keeps playing) instead of closing it.
    override val handleBackNavigation: Boolean = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        LossBridge.appContext = applicationContext
        val wanted = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= 33) {
            wanted += Manifest.permission.READ_MEDIA_AUDIO
            wanted += Manifest.permission.POST_NOTIFICATIONS
        } else {
            wanted += Manifest.permission.READ_EXTERNAL_STORAGE
        }
        val missing = wanted.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
        if (missing.isNotEmpty()) requestPermissions(missing.toTypedArray(), 7)
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (keyCode == KeyEvent.KEYCODE_BACK) {
            // Rust closes the player, a sub-page or returns to Home. Only when
            // there's nothing to go back to does the app move to the background
            // (music keeps playing).
            val handled = try { LossNative.back() } catch (e: UnsatisfiedLinkError) { false }
            if (!handled) moveTaskToBack(true)
            return true
        }
        return super.onKeyDown(keyCode, event)
    }
}
