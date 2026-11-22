// Background reliability (M0-13): reports the phone's settings that decide whether Loss may keep
// running with the screen off, and opens the screens that change them. Rust decides what to show
// (crates/loss-platform/src/reliability.rs). Kotlin only forwards to Rust; decisions live in Rust.

package app.loss.glue

import android.app.ActivityManager
import android.app.NotificationManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import org.json.JSONObject

object LossReliability {
    // Xiaomi's Autostart screen has moved between MIUI and HyperOS versions.
    private val autostartScreens = listOf(
        ComponentName("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"),
        ComponentName("com.miui.securitycenter", "com.miui.appmanager.ApplicationsDetailsActivity"),
    )

    private fun autostartIntent(ctx: Context): Intent? = autostartScreens
        .map { Intent().setComponent(it) }
        .firstOrNull { ctx.packageManager.resolveActivity(it, 0) != null }

    fun status(ctx: Context): String {
        val pm = ctx.getSystemService(Context.POWER_SERVICE) as PowerManager
        val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        val am = ctx.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        return JSONObject()
            .put("manufacturer", Build.MANUFACTURER)
            .put("sdk", Build.VERSION.SDK_INT)
            .put("battery_unrestricted", pm.isIgnoringBatteryOptimizations(ctx.packageName))
            .put("notifications", nm.areNotificationsEnabled())
            .put("background_restricted", Build.VERSION.SDK_INT >= 28 && am.isBackgroundRestricted)
            .put("autostart_screen", autostartIntent(ctx) != null)
            .toString()
    }

    /** Opens the settings screen for `what`: battery, autostart, notifications or app. */
    fun open(ctx: Context, what: String) {
        val pkg = ctx.packageName
        val details = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$pkg"))
        val intent = when (what) {
            "battery" -> Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:$pkg"))
            "autostart" -> autostartIntent(ctx) ?: details
            "notifications" -> Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                .putExtra(Settings.EXTRA_APP_PACKAGE, pkg)
            else -> details
        }
        for (i in listOf(intent, details)) {
            try {
                ctx.startActivity(i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                return
            } catch (e: Exception) {
                android.util.Log.w("Loss", "cannot open $what settings: $e")
            }
        }
    }
}
