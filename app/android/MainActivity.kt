// Loss Android glue. Everything else in the app is Rust; Android requires
// Java/Kotlin for a foreground media service, the MediaSession (lock screen,
// notification, Bluetooth buttons) and runtime permission prompts.
//
// Rust talks to this file through LossBridge's static methods (via JNI).
// Button presses, audio focus and unplug events go straight to Rust through
// LossNative's native methods; LossBridge.pollCommands() is only a fallback.

package dev.dioxus.main

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.hardware.usb.UsbManager
import android.media.AudioFormat
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
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

/** Calls into Rust (libmain.so, loaded by WryActivity). */
object LossNative {
    /** A playback command: play, pause, toggle, next, prev, "seek <ms>". */
    @JvmStatic external fun command(cmd: String)

    /** Back was pressed. Returns false when the app has nothing to go back to. */
    @JvmStatic external fun back(): Boolean
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

    /** Sends a command to Rust right away; queues it if the native call isn't available. */
    fun push(cmd: String) {
        try {
            LossNative.command(cmd)
        } catch (e: UnsatisfiedLinkError) {
            commands.add(cmd)
        }
    }

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

    /** Rust: put text on the clipboard. */
    @JvmStatic
    fun copyText(ctx: Context, label: String, text: String) {
        val app = ctx.applicationContext
        main.post {
            val cm = app.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
            cm.setPrimaryClip(android.content.ClipData.newPlainText(label, text))
        }
    }

    /** Rust: open the share sheet with text. */
    @JvmStatic
    fun shareText(ctx: Context, subject: String, text: String) {
        val app = ctx.applicationContext
        main.post {
            val send = Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_SUBJECT, subject)
                putExtra(Intent.EXTRA_TEXT, text)
            }
            val chooser = Intent.createChooser(send, subject).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            try {
                app.startActivity(chooser)
            } catch (e: Exception) {
                android.util.Log.w("Loss", "share failed: $e")
            }
        }
    }

    /** Rust: device and audio-output details for bug reports and DAC checks. */
    @JvmStatic
    fun diagnostics(ctx: Context): String {
        val sb = StringBuilder()
        sb.append("Device: ${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE})\n")
        sb.append("Android: ${Build.VERSION.RELEASE} (SDK ${Build.VERSION.SDK_INT})\n")
        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        sb.append("Media volume: ${am.getStreamVolume(AudioManager.STREAM_MUSIC)}/${am.getStreamMaxVolume(AudioManager.STREAM_MUSIC)}\n")
        sb.append("System mixer: ${am.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)} Hz, " +
            "${am.getProperty(AudioManager.PROPERTY_OUTPUT_FRAMES_PER_BUFFER)} frames per buffer\n")
        val pm = ctx.packageManager
        sb.append("Low-latency audio: ${pm.hasSystemFeature(PackageManager.FEATURE_AUDIO_LOW_LATENCY)}, " +
            "pro audio: ${pm.hasSystemFeature(PackageManager.FEATURE_AUDIO_PRO)}\n")
        sb.append("Audio outputs:\n")
        for (d in am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)) {
            val rates = d.sampleRates
            sb.append("- ${deviceType(d.type)} \"${d.productName}\" (id ${d.id})\n")
            sb.append("    sample rates: ${if (rates.isEmpty()) "any" else rates.joinToString(", ")}\n")
            sb.append("    channels: ${d.channelCounts.joinToString(", ").ifEmpty { "any" }}\n")
            sb.append("    formats: ${d.encodings.joinToString(", ") { encoding(it) }.ifEmpty { "any" }}\n")
            if (Build.VERSION.SDK_INT >= 34) {
                try {
                    val mixer = am.getSupportedMixerAttributes(d)
                    val perfect = mixer.filter { it.mixerBehavior == android.media.AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT }
                    if (perfect.isNotEmpty()) {
                        sb.append("    bit-perfect: " + perfect.joinToString(", ") {
                            "${encoding(it.format.encoding)} ${it.format.sampleRate} Hz"
                        } + "\n")
                    } else if (mixer.isNotEmpty()) {
                        sb.append("    bit-perfect: not offered by this phone for this output\n")
                    }
                } catch (e: Exception) {
                    sb.append("    bit-perfect: unknown ($e)\n")
                }
            }
        }
        try {
            val um = ctx.getSystemService(Context.USB_SERVICE) as UsbManager
            for (u in um.deviceList.values) {
                val audio = (0 until u.interfaceCount).count { u.getInterface(it).interfaceClass == 1 }
                sb.append("USB: ${u.manufacturerName ?: ""} ${u.productName ?: ""} " +
                    "(vendor %04x, product %04x, %d audio interfaces)\n".format(u.vendorId, u.productId, audio))
            }
        } catch (e: Exception) {
            sb.append("USB: unavailable ($e)\n")
        }
        return sb.toString()
    }

    private fun deviceType(t: Int): String = when (t) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "Phone speaker"
        AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "Earpiece"
        AudioDeviceInfo.TYPE_WIRED_HEADSET -> "Wired headset"
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> "Wired headphones"
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> "Bluetooth"
        AudioDeviceInfo.TYPE_BLUETOOTH_SCO -> "Bluetooth (calls)"
        AudioDeviceInfo.TYPE_USB_HEADSET -> "USB headset"
        AudioDeviceInfo.TYPE_USB_DEVICE -> "USB audio"
        AudioDeviceInfo.TYPE_USB_ACCESSORY -> "USB accessory"
        AudioDeviceInfo.TYPE_HDMI -> "HDMI"
        AudioDeviceInfo.TYPE_TELEPHONY -> "Telephony"
        26 -> "Bluetooth LE headset"
        27 -> "Bluetooth LE speaker"
        else -> "Type $t"
    }

    private fun encoding(e: Int): String = when (e) {
        AudioFormat.ENCODING_PCM_16BIT -> "16-bit"
        AudioFormat.ENCODING_PCM_8BIT -> "8-bit"
        AudioFormat.ENCODING_PCM_FLOAT -> "32-bit float"
        AudioFormat.ENCODING_PCM_24BIT_PACKED -> "24-bit"
        AudioFormat.ENCODING_PCM_32BIT -> "32-bit"
        AudioFormat.ENCODING_AC3 -> "AC-3"
        AudioFormat.ENCODING_E_AC3 -> "E-AC-3"
        AudioFormat.ENCODING_DTS -> "DTS"
        AudioFormat.ENCODING_IEC61937 -> "IEC 61937"
        else -> "encoding $e"
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

    // Audio focus: pause for calls and other apps (PRB-1).
    private lateinit var audioManager: AudioManager
    private var focusRequest: AudioFocusRequest? = null
    private var hasFocus = false
    /** We paused because of a temporary focus loss (e.g. a call) and resume when it ends. */
    private var pausedForFocus = false
    private val mainHandler = Handler(Looper.getMainLooper())

    private val focusListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_LOSS -> {
                // Another app took over for good: pause and let go.
                hasFocus = false
                pausedForFocus = false
                if (LossBridge.playing) LossBridge.push("pause")
                abandonFocus()
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                // A call or a short sound: pause now, resume afterwards. We don't
                // hold focus meanwhile, so pressing play during a call asks again
                // (and is refused) instead of playing over it.
                hasFocus = false
                if (LossBridge.playing) {
                    pausedForFocus = true
                    LossBridge.push("pause")
                }
            }
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                // Android lowers our volume by itself (setWillPauseWhenDucked(false)).
            }
            AudioManager.AUDIOFOCUS_GAIN -> {
                hasFocus = true
                if (pausedForFocus) {
                    pausedForFocus = false
                    LossBridge.push("play")
                }
            }
        }
    }

    // Pause when headphones, the USB DAC or Bluetooth disconnect, instead of
    // switching to the speaker (PRB-2).
    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) pauseForUnplug()
        }
    }

    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) {
            val external = removed.any {
                it.isSink && it.type in setOf(
                    AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE,
                    AudioDeviceInfo.TYPE_USB_ACCESSORY, AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
                    AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
                    AudioDeviceInfo.TYPE_LINE_ANALOG, AudioDeviceInfo.TYPE_LINE_DIGITAL
                )
            }
            if (external) pauseForUnplug()
        }
    }

    private fun pauseForUnplug() {
        if (LossBridge.playing) {
            pausedForFocus = false
            LossBridge.push("pause")
        }
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
    }

    /** Called on every state update from Rust. */
    private fun syncFocus(playing: Boolean) {
        if (playing) {
            pausedForFocus = false
            // Refused, e.g. during a phone call: don't play over it.
            if (!requestFocus()) LossBridge.push("pause")
        } else if (hasFocus && !pausedForFocus) {
            // Paused by the user: let other apps play.
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
        syncFocus(b.playing)
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
        try { unregisterReceiver(noisyReceiver) } catch (e: IllegalArgumentException) { }
        audioManager.unregisterAudioDeviceCallback(deviceCallback)
        abandonFocus()
        wakeLock?.release()
        session.isActive = false
        session.release()
        super.onDestroy()
    }
}
