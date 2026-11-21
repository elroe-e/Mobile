// Foreground media service: MediaSession (lock screen, notification, headset and Bluetooth
// buttons), audio focus, and pausing when the headphones are unplugged.
// Kotlin only forwards to Rust; decisions live in Rust.

package app.loss.glue

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import java.io.File
import org.json.JSONObject

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

    // Audio focus (PRB-1) and unplugging (PRB-2). Kotlin holds the focus request and reports
    // changes; Rust (loss-platform policy) decides whether to pause or resume.
    private lateinit var audioManager: AudioManager
    private var focusRequest: AudioFocusRequest? = null
    /** A focus request is outstanding (not abandoned). */
    private var requested = false
    /** We hold focus right now (false during a call even while `requested`). */
    private var hasFocus = false
    private val mainHandler = Handler(Looper.getMainLooper())

    private fun focusEvent(change: String) =
        LossBridge.emit("focus", JSONObject().put("change", change).toString())

    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_LOSS -> {
                // The request is finished for good; let go of it.
                abandonFocus()
                focusEvent("loss")
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                // Keep the request so Android tells us when the call ends.
                hasFocus = false
                focusEvent("loss_transient")
            }
            // Android lowers our volume by itself (setWillPauseWhenDucked(false)).
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> focusEvent("duck")
            AudioManager.AUDIOFOCUS_GAIN -> {
                hasFocus = true
                focusEvent("gain")
            }
        }
    }

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) LossBridge.emit("noisy")
        }
    }

    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) =
            LossBridge.outputs("outputs_added", added)

        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) =
            LossBridge.outputs("outputs_removed", removed)
    }

    private fun requestFocus(): Boolean {
        if (hasFocus) return true
        val granted = if (Build.VERSION.SDK_INT >= 26) {
            val req = focusRequest ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build()
                )
                .setWillPauseWhenDucked(false)
                .setOnAudioFocusChangeListener(focusListener, mainHandler)
                .build()
                .also { focusRequest = it }
            audioManager.requestAudioFocus(req)
        } else {
            @Suppress("DEPRECATION")
            audioManager.requestAudioFocus(focusListener, AudioManager.STREAM_MUSIC, AudioManager.AUDIOFOCUS_GAIN)
        }
        hasFocus = granted == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
        requested = hasFocus
        return hasFocus
    }

    private fun abandonFocus() {
        if (Build.VERSION.SDK_INT >= 26) {
            focusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
        } else {
            @Suppress("DEPRECATION")
            audioManager.abandonAudioFocus(focusListener)
        }
        hasFocus = false
        requested = false
    }

    /** Called on every state update from Rust: hold focus while playing, or while Rust waits
     *  for a call to end (keepFocus); otherwise let other apps play. */
    private fun syncFocus(playing: Boolean, keepFocus: Boolean) {
        if (playing) {
            if (!requestFocus()) focusEvent("refused")
        } else if (requested && !keepFocus) {
            abandonFocus()
        }
    }

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
                override fun onPlay() = LossBridge.command("play")
                override fun onPause() = LossBridge.command("pause")
                override fun onSkipToNext() = LossBridge.command("next")
                override fun onSkipToPrevious() = LossBridge.command("prev")
                override fun onStop() = LossBridge.command("pause")
                override fun onSeekTo(pos: Long) = LossBridge.command("seek $pos")
            })
            val open = packageManager.getLaunchIntentForPackage(packageName)
            if (open != null) {
                setSessionActivity(PendingIntent.getActivity(this@LossPlaybackService, 0, open, PendingIntent.FLAG_IMMUTABLE))
            }
            isActive = true
        }
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "Loss:playback").apply { setReferenceCounted(false) }

        audioManager = getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val noisy = IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY)
        if (Build.VERSION.SDK_INT >= 33) {
            registerReceiver(noisyReceiver, noisy, Context.RECEIVER_NOT_EXPORTED)
        } else {
            registerReceiver(noisyReceiver, noisy)
        }
        audioManager.registerAudioDeviceCallback(deviceCallback, mainHandler)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            "loss.PREV" -> LossBridge.command("prev")
            "loss.TOGGLE" -> LossBridge.command("toggle")
            "loss.NEXT" -> LossBridge.command("next")
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
        syncFocus(b.playing, b.keepFocus)
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
            .setSmallIcon(R.drawable.loss_ic_notification)
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
        try { unregisterReceiver(noisyReceiver) } catch (e: IllegalArgumentException) { }
        audioManager.unregisterAudioDeviceCallback(deviceCallback)
        abandonFocus()
        wakeLock?.release()
        session.isActive = false
        session.release()
        super.onDestroy()
    }
}
