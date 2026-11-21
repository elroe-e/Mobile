// Foreground service that keeps downloads running with the screen off.
// Kotlin only forwards to Rust; decisions live in Rust.

package app.loss.glue

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.drawable.Icon
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager

/** Keeps downloads running with the screen off or the app in the background. */
class LossDownloadService : Service() {
    companion object {
        @Volatile var instance: LossDownloadService? = null
        private const val CHANNEL = "downloads"
        private const val NOTIFICATION_ID = 2

        private fun channel(ctx: Context) {
            if (Build.VERSION.SDK_INT >= 26) {
                val ch = NotificationChannel(CHANNEL, "Downloads", NotificationManager.IMPORTANCE_LOW)
                ch.setShowBadge(false)
                ctx.getSystemService(NotificationManager::class.java).createNotificationChannel(ch)
            }
        }

        private fun open(ctx: Context): PendingIntent? =
            ctx.packageManager.getLaunchIntentForPackage(ctx.packageName)?.let {
                PendingIntent.getActivity(ctx, 10, it, PendingIntent.FLAG_IMMUTABLE)
            }

        private fun action(ctx: Context, label: String, cmd: String, code: Int): Notification.Action {
            val i = Intent(ctx, LossDownloadService::class.java).setAction(cmd)
            val pi = PendingIntent.getService(ctx, code, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            return Notification.Action.Builder(Icon.createWithResource(ctx, android.R.drawable.stat_sys_download), label, pi).build()
        }

        fun build(ctx: Context): Notification {
            channel(ctx)
            val b = LossBridge
            val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(ctx, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(ctx)
            builder
                .setSmallIcon(if (b.dlState == 1) android.R.drawable.stat_sys_download else android.R.drawable.stat_sys_download_done)
                .setContentTitle(b.dlTitle)
                .setContentText(b.dlText)
                .setOnlyAlertOnce(true)
                .setShowWhen(false)
                .setOngoing(b.dlState == 1)
            open(ctx)?.let { builder.setContentIntent(it) }
            when (b.dlState) {
                1 -> {
                    if (b.dlProgress >= 0) builder.setProgress(1000, b.dlProgress, false) else builder.setProgress(0, 0, true)
                    builder.addAction(action(ctx, "Pause all", "loss.DL_PAUSE", 20))
                }
                2 -> builder.addAction(action(ctx, "Resume", "loss.DL_RESUME", 21))
            }
            return builder.build()
        }

        /** Nothing running: leave a dismissible summary (or nothing). */
        fun notifyIdle(ctx: Context) {
            val nm = ctx.getSystemService(NotificationManager::class.java)
            if (LossBridge.dlTitle.isEmpty()) nm.cancel(NOTIFICATION_ID) else nm.notify(NOTIFICATION_ID, build(ctx))
        }
    }

    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Loss:downloads").apply { setReferenceCounted(false) }
        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        @Suppress("DEPRECATION")
        wifiLock = wm.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "Loss:downloads").apply { setReferenceCounted(false) }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            "loss.DL_PAUSE" -> LossBridge.command("dl_pause_all")
            "loss.DL_RESUME" -> LossBridge.command("dl_resume_all")
        }
        refresh()
        return START_NOT_STICKY
    }

    fun refresh() {
        val n = build(this)
        if (LossBridge.dlState == 1) {
            try {
                if (Build.VERSION.SDK_INT >= 29) {
                    startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
                } else {
                    startForeground(NOTIFICATION_ID, n)
                }
            } catch (e: Exception) {
                android.util.Log.w("Loss", "download startForeground failed: $e")
            }
            wakeLock?.acquire(3 * 60 * 60 * 1000L)
            wifiLock?.acquire()
        } else {
            // Paused or finished: release everything and leave a normal notification.
            wakeLock?.release()
            wifiLock?.release()
            if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_DETACH) else @Suppress("DEPRECATION") stopForeground(false)
            notifyIdle(this)
            stopSelf()
        }
    }

    override fun onDestroy() {
        instance = null
        wakeLock?.release()
        wifiLock?.release()
        super.onDestroy()
    }
}
