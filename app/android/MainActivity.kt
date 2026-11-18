// Loss Android glue. Everything else in the app is Rust; Android requires
// Java/Kotlin for a foreground media service, the MediaSession (lock screen,
// notification, Bluetooth buttons) and runtime permission prompts.
//
// Rust talks to this file through LossBridge's static methods (via JNI) and
// polls LossBridge.pollCommands() for button presses.

package dev.dioxus.main

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.view.KeyEvent
import java.io.File
import java.util.concurrent.ConcurrentLinkedQueue

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
            moveTaskToBack(true)
            return true
        }
        return super.onKeyDown(keyCode, event)
    }
}

/** Shared state between Rust and the playback service. */
object LossBridge {
    @Volatile var appContext: Context? = null
    private val commands = ConcurrentLinkedQueue<String>()
    private val main = Handler(Looper.getMainLooper())

    @Volatile var title: String = ""
    @Volatile var artist: String = ""
    @Volatile var album: String = ""
    @Volatile var artPath: String = ""
    @Volatile var playing: Boolean = false
    @Volatile var positionMs: Long = 0
    @Volatile var durationMs: Long = 0
    @Volatile var hasNext: Boolean = true

    fun push(cmd: String) { commands.add(cmd) }

    /** Rust: newline-separated commands since the last call ("" if none). */
    @JvmStatic
    fun pollCommands(): String {
        val sb = StringBuilder()
        while (true) {
            val c = commands.poll() ?: break
            if (sb.isNotEmpty()) sb.append('\n')
            sb.append(c)
        }
        return sb.toString()
    }

    /** Rust: something changed. Starts the service if needed and refreshes it. */
    @JvmStatic
    fun update(
        ctx: Context, title: String, artist: String, album: String, artPath: String,
        playing: Boolean, positionMs: Long, durationMs: Long
    ) {
        this.title = title; this.artist = artist; this.album = album; this.artPath = artPath
        this.playing = playing; this.positionMs = positionMs; this.durationMs = durationMs
        val app = ctx.applicationContext
        appContext = app
        main.post {
            val svc = LossPlaybackService.instance
            if (svc != null) {
                svc.refresh()
            } else {
                try {
                    val i = Intent(app, LossPlaybackService::class.java)
                    if (Build.VERSION.SDK_INT >= 26) app.startForegroundService(i) else app.startService(i)
                } catch (e: Exception) {
                    android.util.Log.w("Loss", "cannot start playback service: $e")
                }
            }
        }
    }

    /** Rust: playback stopped and the queue is empty. */
    @JvmStatic
    fun stop() {
        main.post { LossPlaybackService.instance?.shutdown() }
    }
}

class LossPlaybackService : Service() {
    companion object {
        @Volatile var instance: LossPlaybackService? = null
        private const val CHANNEL = "playback"
        private const val NOTIFICATION_ID = 1
    }

    private lateinit var session: MediaSession
    private var wakeLock: PowerManager.WakeLock? = null
    private var lastArt: String? = null
    private var art: Bitmap? = null
    private var isForeground = false
    private var everForeground = false

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        if (Build.VERSION.SDK_INT >= 26) {
            val ch = NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW)
            ch.setShowBadge(false)
            getSystemService(NotificationManager::class.java).createNotificationChannel(ch)
        }
        session = MediaSession(this, "Loss").apply {
            setCallback(object : MediaSession.Callback() {
                override fun onPlay() = LossBridge.push("play")
                override fun onPause() = LossBridge.push("pause")
                override fun onSkipToNext() = LossBridge.push("next")
                override fun onSkipToPrevious() = LossBridge.push("prev")
                override fun onStop() = LossBridge.push("pause")
                override fun onSeekTo(pos: Long) = LossBridge.push("seek $pos")
            })
            val open = packageManager.getLaunchIntentForPackage(packageName)
            if (open != null) {
                setSessionActivity(PendingIntent.getActivity(this@LossPlaybackService, 0, open, PendingIntent.FLAG_IMMUTABLE))
            }
            isActive = true
        }
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Loss:playback").apply { setReferenceCounted(false) }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            "loss.PREV" -> LossBridge.push("prev")
            "loss.TOGGLE" -> LossBridge.push("toggle")
            "loss.NEXT" -> LossBridge.push("next")
        }
        refresh()
        return START_NOT_STICKY
    }

    private fun action(icon: Int, label: String, act: String, code: Int): Notification.Action {
        val i = Intent(this, LossPlaybackService::class.java).setAction(act)
        val pi = PendingIntent.getService(this, code, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        return Notification.Action.Builder(Icon.createWithResource(this, icon), label, pi).build()
    }

    fun refresh() {
        val b = LossBridge
        if (b.artPath != lastArt) {
            lastArt = b.artPath
            art = if (b.artPath.isNotEmpty() && File(b.artPath).exists()) BitmapFactory.decodeFile(b.artPath) else null
        }
        val meta = MediaMetadata.Builder()
            .putString(MediaMetadata.METADATA_KEY_TITLE, b.title)
            .putString(MediaMetadata.METADATA_KEY_ARTIST, b.artist)
            .putString(MediaMetadata.METADATA_KEY_ALBUM, b.album)
            .putLong(MediaMetadata.METADATA_KEY_DURATION, b.durationMs)
        art?.let { meta.putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, it) }
        session.setMetadata(meta.build())
        session.setPlaybackState(
            PlaybackState.Builder()
                .setActions(
                    PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or PlaybackState.ACTION_PLAY_PAUSE or
                        PlaybackState.ACTION_SKIP_TO_NEXT or PlaybackState.ACTION_SKIP_TO_PREVIOUS or
                        PlaybackState.ACTION_SEEK_TO or PlaybackState.ACTION_STOP
                )
                .setState(
                    if (b.playing) PlaybackState.STATE_PLAYING else PlaybackState.STATE_PAUSED,
                    b.positionMs, if (b.playing) 1f else 0f
                )
                .build()
        )

        val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(this)
        builder
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(b.title)
            .setContentText(listOf(b.artist, b.album).filter { it.isNotEmpty() }.joinToString(" · "))
            .setLargeIcon(art)
            .setOngoing(b.playing)
            .setShowWhen(false)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .addAction(action(android.R.drawable.ic_media_previous, "Previous", "loss.PREV", 1))
            .addAction(
                if (b.playing) action(android.R.drawable.ic_media_pause, "Pause", "loss.TOGGLE", 2)
                else action(android.R.drawable.ic_media_play, "Play", "loss.TOGGLE", 2)
            )
            .addAction(action(android.R.drawable.ic_media_next, "Next", "loss.NEXT", 3))
            .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken).setShowActionsInCompactView(0, 1, 2))
        session.controller.sessionActivity?.let { builder.setContentIntent(it) }
        val n = builder.build()

        // Playing: foreground service (Android won't stop it).
        // Paused: an ordinary notification the user can swipe away.
        // The very first call must always go to foreground (Android requires
        // it within seconds of startForegroundService).
        if (b.playing || !everForeground) {
            try {
                if (Build.VERSION.SDK_INT >= 29) {
                    startForeground(NOTIFICATION_ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
                } else {
                    startForeground(NOTIFICATION_ID, n)
                }
                isForeground = true
                everForeground = true
            } catch (e: Exception) {
                android.util.Log.w("Loss", "startForeground failed: $e")
                getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, n)
            }
        }
        if (!b.playing) {
            if (isForeground && Build.VERSION.SDK_INT >= 24) {
                stopForeground(STOP_FOREGROUND_DETACH)
                isForeground = false
            }
            getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, n)
        }
        if (b.playing) wakeLock?.acquire(6 * 60 * 60 * 1000L) else wakeLock?.release()
    }

    fun shutdown() {
        wakeLock?.release()
        isForeground = false
        if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE) else @Suppress("DEPRECATION") stopForeground(true)
        stopSelf()
    }

    override fun onDestroy() {
        instance = null
        wakeLock?.release()
        session.isActive = false
        session.release()
        super.onDestroy()
    }
}
