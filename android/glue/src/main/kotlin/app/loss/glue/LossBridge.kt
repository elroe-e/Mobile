// Static helpers Rust calls through JNI, plus state shared with the services.
// Kotlin only forwards to Rust; decisions live in Rust.

package app.loss.glue

import android.app.Activity
import android.view.HapticFeedbackConstants
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.hardware.usb.UsbManager
import android.media.AudioFormat
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.util.concurrent.ConcurrentLinkedQueue
import org.json.JSONArray
import org.json.JSONObject

/** Shared state between Rust and the playback service. */
object LossBridge {
    @Volatile var appContext: Context? = null
    private val main = Handler(Looper.getMainLooper())

    @Volatile var title: String = ""
    @Volatile var artist: String = ""
    @Volatile var album: String = ""
    @Volatile var artPath: String = ""
    @Volatile var playing: Boolean = false
    @Volatile var positionMs: Long = 0
    @Volatile var durationMs: Long = 0
    @Volatile var hasNext: Boolean = true
    /** Rust: keep the audio-focus request while paused, because a call paused us. */
    @Volatile var keepFocus: Boolean = false

    // ---- Events to Rust ----

    @Volatile private var nativeReady = false
    private val pending = ConcurrentLinkedQueue<Pair<String, String>>()

    /** Sends an event to Rust; queues it (up to 64) until Rust has registered its natives. */
    fun emit(kind: String, json: String = "{}") {
        if (nativeReady) {
            try {
                LossNative.onEvent(kind, json)
                return
            } catch (e: UnsatisfiedLinkError) {
                nativeReady = false
            }
        }
        if (pending.size < 64) pending.add(kind to json)
    }

    /** A playback or download command: play, pause, toggle, next, prev, "seek <ms>", dl_*. */
    fun command(cmd: String) = emit("media", JSONObject().put("cmd", cmd).toString())

    fun outputs(kind: String, devices: Array<out AudioDeviceInfo>) {
        val types = JSONArray()
        devices.filter { it.isSink }.forEach { types.put(it.type) }
        if (types.length() > 0) emit(kind, JSONObject().put("types", types).toString())
    }

    private fun emitVolume(ctx: Context) {
        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        emit(
            "volume",
            JSONObject()
                .put("level", am.getStreamVolume(AudioManager.STREAM_MUSIC))
                .put("max", am.getStreamMaxVolume(AudioManager.STREAM_MUSIC))
                .toString()
        )
    }

    // The phone's media volume changed (buttons, other apps, Loss itself). The broadcast only
    // triggers a fresh read, so its contents don't matter.
    private val volumeReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = emitVolume(context)
    }
    private var volumeRegistered = false

    // Outputs coming and going (headphones, the USB DAC, Bluetooth), for the whole app's life:
    // Rust pauses on unplug and picks the new output's sample rates (M1-02, M2-03).
    private val deviceCallback = object : android.media.AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) = outputs("outputs_added", added)
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) = outputs("outputs_removed", removed)
    }

    /** Rust: natives are registered. Delivers queued events and starts reporting volume. */
    @JvmStatic
    fun nativeReady(ctx: Context) {
        val app = ctx.applicationContext
        appContext = app
        nativeReady = true
        while (true) {
            val (kind, json) = pending.poll() ?: break
            emit(kind, json)
        }
        main.post {
            if (!volumeRegistered) {
                (app.getSystemService(Context.AUDIO_SERVICE) as AudioManager)
                    .registerAudioDeviceCallback(deviceCallback, main)
                val filter = IntentFilter("android.media.VOLUME_CHANGED_ACTION")
                if (Build.VERSION.SDK_INT >= 33) {
                    app.registerReceiver(volumeReceiver, filter, Context.RECEIVER_EXPORTED)
                } else {
                    app.registerReceiver(volumeReceiver, filter)
                }
                volumeRegistered = true
            }
            emitVolume(app)
        }
    }

    /** Rust: something changed. Starts the service if needed and refreshes it. */
    @JvmStatic
    fun update(
        ctx: Context, title: String, artist: String, album: String, artPath: String,
        playing: Boolean, positionMs: Long, durationMs: Long, keepFocus: Boolean
    ) {
        this.title = title; this.artist = artist; this.album = album; this.artPath = artPath
        this.playing = playing; this.positionMs = positionMs; this.durationMs = durationMs
        this.keepFocus = keepFocus
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

    // ---- Downloads (M1-05) ----
    @Volatile var dlState: Int = 0 // 0 idle, 1 running, 2 paused
    @Volatile var dlTitle: String = ""
    @Volatile var dlText: String = ""
    @Volatile var dlProgress: Int = -1 // 0..1000, -1 = unknown

    /**
     * Rust: download state changed. state 1 = running (keeps a foreground
     * service so Android doesn't stop downloads in the background), 2 = all
     * paused, 0 = nothing left (title/text describe the result).
     */
    @JvmStatic
    fun downloads(ctx: Context, state: Int, title: String, text: String, progress: Int) {
        dlState = state; dlTitle = title; dlText = text; dlProgress = progress
        val app = ctx.applicationContext
        main.post {
            val svc = LossDownloadService.instance
            when {
                svc != null -> svc.refresh()
                state == 1 -> try {
                    val i = Intent(app, LossDownloadService::class.java)
                    if (Build.VERSION.SDK_INT >= 26) app.startForegroundService(i) else app.startService(i)
                } catch (e: Exception) {
                    android.util.Log.w("Loss", "cannot start download service: $e")
                }
                else -> LossDownloadService.notifyIdle(app)
            }
        }
    }

    // ---- Haptics (M0-10) ----

    /** Rust: vibration feedback (an Android HapticFeedbackConstants value). */
    @JvmStatic
    fun haptic(act: Activity, kind: Int) {
        // VIRTUAL_KEY (1) becomes CONFIRM where the phone has it (Android 11+).
        val k = if (kind == 1 && Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.CONFIRM else kind
        act.runOnUiThread { act.window?.decorView?.performHapticFeedback(k) }
    }

    // ---- Background reliability (M0-13) ----

    /** Rust: the phone's background settings as JSON (see LossReliability). */
    @JvmStatic
    fun reliability(ctx: Context): String = LossReliability.status(ctx)

    /** Rust: opens a settings screen: battery, autostart, notifications or app. */
    @JvmStatic
    fun openReliability(ctx: Context, what: String) = LossReliability.open(ctx, what)

    // ---- Vault key (M1-07): Rust's vault key, encrypted by an Android Keystore key ----

    private fun keystoreKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey("loss_vault", null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder("loss_vault", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return gen.generateKey()
    }

    /** Rust: encrypt the vault key. Returns IV (12 bytes) + ciphertext, or null. */
    @JvmStatic
    fun wrapKey(raw: ByteArray): ByteArray? = try {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, keystoreKey())
        c.iv + c.doFinal(raw)
    } catch (e: Exception) {
        android.util.Log.w("Loss", "vault key wrap failed: $e")
        null
    }

    /** Rust: decrypt the vault key, or null if it can't be (e.g. Keystore reset). */
    @JvmStatic
    fun unwrapKey(wrapped: ByteArray): ByteArray? = try {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.DECRYPT_MODE, keystoreKey(), GCMParameterSpec(128, wrapped, 0, 12))
        c.doFinal(wrapped, 12, wrapped.size - 12)
    } catch (e: Exception) {
        android.util.Log.w("Loss", "vault key unwrap failed: $e")
        null
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
    /**
     * Rust: every audio output as JSON, and the ids media plays through right now (Android 13+;
     * empty before). Rust picks the active output and its rates (M2-03).
     */
    @JvmStatic
    fun outputs(ctx: Context): String {
        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val list = JSONArray()
        for (d in am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)) {
            list.put(
                JSONObject()
                    .put("id", d.id)
                    .put("type", d.type)
                    .put("name", d.productName?.toString() ?: "")
                    .put("rates", JSONArray(d.sampleRates.toList()))
                    .put("channels", JSONArray(d.channelCounts.toList()))
                    .put("encodings", JSONArray(d.encodings.toList()))
            )
        }
        val active = JSONArray()
        if (Build.VERSION.SDK_INT >= 33) {
            try {
                val attrs = android.media.AudioAttributes.Builder()
                    .setUsage(android.media.AudioAttributes.USAGE_MEDIA).build()
                for (d in am.getAudioDevicesForAttributes(attrs)) active.put(d.id)
            } catch (e: Exception) {
                android.util.Log.w("Loss", "media route unknown: $e")
            }
        }
        return JSONObject().put("devices", list).put("active", active).toString()
    }

    // ---- USB DACs (M2-06; M2-05 builds on it). Rust parses everything; Kotlin only lists
    // devices, asks Android for permission and hands over the open file descriptor.

    private const val USB_PERMISSION = "app.loss.USB_PERMISSION"
    private val usbOpen = HashMap<String, android.hardware.usb.UsbDeviceConnection>()
    private var usbReceiverRegistered = false

    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            val dev: android.hardware.usb.UsbDevice? = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, android.hardware.usb.UsbDevice::class.java)
            } else {
                @Suppress("DEPRECATION") intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
            }
            val granted = intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)
            emit("usb_permission", JSONObject().put("name", dev?.deviceName ?: "").put("granted", granted).toString())
        }
    }

    /** Rust: USB devices as JSON (name, vendor, product, names, interface classes, permission). */
    @JvmStatic
    fun usbDevices(ctx: Context): String {
        val um = ctx.getSystemService(Context.USB_SERVICE) as UsbManager
        val list = JSONArray()
        for (d in um.deviceList.values) {
            val classes = JSONArray()
            for (i in 0 until d.interfaceCount) classes.put(d.getInterface(i).interfaceClass)
            list.put(
                JSONObject()
                    .put("name", d.deviceName)
                    .put("vendor", d.vendorId)
                    .put("product", d.productId)
                    .put("manufacturer", d.manufacturerName ?: "")
                    .put("product_name", d.productName ?: "")
                    .put("classes", classes)
                    .put("permitted", um.hasPermission(d))
            )
        }
        return list.toString()
    }

    /**
     * Rust: open the device and return its file descriptor; -1 and a permission prompt when
     * Android hasn't allowed it yet (the answer arrives as a "usb_permission" event); -2 when
     * the device is gone or can't be opened.
     */
    @JvmStatic
    fun usbOpen(ctx: Context, name: String): Int {
        val app = ctx.applicationContext
        val um = app.getSystemService(Context.USB_SERVICE) as UsbManager
        val dev = um.deviceList[name] ?: return -2
        if (!um.hasPermission(dev)) {
            if (!usbReceiverRegistered) {
                val filter = IntentFilter(USB_PERMISSION)
                if (Build.VERSION.SDK_INT >= 33) {
                    app.registerReceiver(usbReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
                } else {
                    app.registerReceiver(usbReceiver, filter)
                }
                usbReceiverRegistered = true
            }
            val flags = if (Build.VERSION.SDK_INT >= 31) android.app.PendingIntent.FLAG_MUTABLE else 0
            val intent = Intent(USB_PERMISSION).setPackage(app.packageName)
            um.requestPermission(dev, android.app.PendingIntent.getBroadcast(app, 0, intent, flags))
            return -1
        }
        synchronized(usbOpen) {
            usbOpen[name]?.let { return it.fileDescriptor }
            val conn = um.openDevice(dev) ?: return -2
            usbOpen[name] = conn
            return conn.fileDescriptor
        }
    }

    /** Rust: done with the device. */
    @JvmStatic
    fun usbClose(name: String) {
        synchronized(usbOpen) { usbOpen.remove(name)?.close() }
    }

    // ---- Output modes (M2-04). Rust decides which mode and format; Kotlin reports what the
    // mixer offers and switches Android 14's bit-perfect mixer for the USB DAC.

    private fun usbOutput(am: AudioManager): AudioDeviceInfo? =
        am.getDevices(AudioManager.GET_DEVICES_OUTPUTS).firstOrNull {
            it.type == AudioDeviceInfo.TYPE_USB_HEADSET || it.type == AudioDeviceInfo.TYPE_USB_DEVICE
        }

    private val mediaAttributes: android.media.AudioAttributes
        get() = android.media.AudioAttributes.Builder()
            .setUsage(android.media.AudioAttributes.USAGE_MEDIA)
            .setContentType(android.media.AudioAttributes.CONTENT_TYPE_MUSIC)
            .build()

    /** Rust: {sdk, mixer_rate, usb: {id, name, bit_perfect: [{rate, encoding}]}} as JSON. */
    @JvmStatic
    fun mixerInfo(ctx: Context): String {
        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val out = JSONObject().put("sdk", Build.VERSION.SDK_INT)
        am.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)?.toIntOrNull()?.let { out.put("mixer_rate", it) }
        val usb = usbOutput(am)
        if (usb != null) {
            val formats = JSONArray()
            if (Build.VERSION.SDK_INT >= 34) {
                try {
                    for (m in am.getSupportedMixerAttributes(usb)) {
                        if (m.mixerBehavior == android.media.AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT) {
                            formats.put(JSONObject().put("rate", m.format.sampleRate).put("encoding", m.format.encoding))
                        }
                    }
                } catch (e: Exception) {
                    android.util.Log.w("Loss", "mixer attributes unknown: $e")
                }
            }
            out.put(
                "usb",
                JSONObject().put("id", usb.id).put("name", usb.productName?.toString() ?: "").put("bit_perfect", formats)
            )
        }
        spatializer(am)?.let { out.put("spatial", it) }
        return out.toString()
    }

    /**
     * The system Spatializer (Android 12+): {level, enabled, available, head_tracking, output}.
     * `output` says whether the current media route can be spatialized (stereo PCM) right now.
     */
    private fun spatializer(am: AudioManager): JSONObject? {
        if (Build.VERSION.SDK_INT < 32) return null
        return try {
            val sp = am.spatializer
            val format = android.media.AudioFormat.Builder()
                .setEncoding(android.media.AudioFormat.ENCODING_PCM_FLOAT)
                .setSampleRate(48000)
                .setChannelMask(android.media.AudioFormat.CHANNEL_OUT_STEREO)
                .build()
            JSONObject()
                .put("level", sp.immersiveAudioLevel)
                .put("enabled", sp.isEnabled)
                .put("available", sp.isAvailable)
                .put("head_tracking", sp.isHeadTrackerAvailable)
                .put("output", sp.canBeSpatialized(mediaAttributes, format))
        } catch (e: Exception) {
            android.util.Log.w("Loss", "spatializer unknown: $e")
            null
        }
    }

    /**
     * Rust: ask for the bit-perfect mixer at this rate and encoding on the USB DAC; true only if
     * Android confirms it as the preferred mixer. Rate 0 clears it.
     */
    @JvmStatic
    fun setBitPerfect(ctx: Context, rate: Int, encoding: Int): Boolean {
        if (Build.VERSION.SDK_INT < 34) return false
        val am = ctx.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        val usb = usbOutput(am) ?: return false
        return try {
            if (rate == 0) {
                am.clearPreferredMixerAttributes(mediaAttributes, usb)
                return true
            }
            val pick = am.getSupportedMixerAttributes(usb).firstOrNull {
                it.mixerBehavior == android.media.AudioMixerAttributes.MIXER_BEHAVIOR_BIT_PERFECT &&
                    it.format.sampleRate == rate && it.format.encoding == encoding
            } ?: return false
            am.setPreferredMixerAttributes(mediaAttributes, usb, pick) &&
                am.getPreferredMixerAttributes(mediaAttributes, usb) == pick
        } catch (e: Exception) {
            android.util.Log.w("Loss", "bit-perfect mixer refused: $e")
            false
        }
    }

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
