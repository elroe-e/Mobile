//! Native AAudio output for Android (M2-02). Replaces cpal on the phone so Loss chooses the
//! sample format (F32, I16, I24 packed, I32), sharing mode (shared or exclusive MMAP),
//! performance mode, usage and content type, the target device and the spatializer, and hears
//! about disconnects. Integer formats are dithered by [`Encoder`] when the samples changed.
//!
//! Functions newer than the app's minimum (API 26) are looked up at run time, so the library
//! still loads on older phones.

// The NDK bindings' constants are u32 or i32 depending on the constant (and the ABI); every
// one is cast to the API's i32.
#![allow(clippy::unnecessary_cast)]

use crate::sink::{
    Caps, Control, Encoder, OutputInfo, OutputPrefs, SampleFormat, Sink, Slot, Stream,
};
use ndk_sys as ffi;
use std::ffi::{c_void, CStr};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Default)]
pub struct AAudioSink {
    prefs: OutputPrefs,
    source_bits: Option<u32>,
    /// The last stream used the platform's bit-perfect path (turn it off when leaving).
    was_bit_perfect: bool,
}

/// What the real-time callback reads and writes. Lives on the heap until the stream is closed.
struct Shared {
    slot: Slot,
    played: Arc<AtomicU64>,
    latency: Arc<AtomicU64>,
    volume: Arc<AtomicU32>,
    dither: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    encoder: Encoder,
    format: SampleFormat,
    channels: usize,
    rate: u32,
    calls: u32,
    /// Bit-perfect: the samples go out untouched, so the software volume is ignored.
    fixed_volume: bool,
}

/// Optional entry points (API 28+, 32+, 34+).
struct Late {
    set_usage: Option<unsafe extern "C" fn(*mut ffi::AAudioStreamBuilder, i32)>,
    set_content_type: Option<unsafe extern "C" fn(*mut ffi::AAudioStreamBuilder, i32)>,
    set_spatialization: Option<unsafe extern "C" fn(*mut ffi::AAudioStreamBuilder, i32)>,
    hardware_format: Option<unsafe extern "C" fn(*mut ffi::AAudioStream) -> i32>,
    hardware_rate: Option<unsafe extern "C" fn(*mut ffi::AAudioStream) -> i32>,
}

fn late() -> &'static Late {
    static LATE: OnceLock<Late> = OnceLock::new();
    LATE.get_or_init(|| unsafe {
        let lib = libc::dlopen(c"libaaudio.so".as_ptr(), libc::RTLD_NOW);
        let sym = |name: &CStr| -> *mut c_void {
            if lib.is_null() {
                null_mut()
            } else {
                libc::dlsym(lib, name.as_ptr())
            }
        };
        type SetFn = unsafe extern "C" fn(*mut ffi::AAudioStreamBuilder, i32);
        type GetFn = unsafe extern "C" fn(*mut ffi::AAudioStream) -> i32;
        macro_rules! f {
            ($name:expr, $t:ty) => {{
                let p = sym($name);
                if p.is_null() {
                    None
                } else {
                    Some(std::mem::transmute::<*mut c_void, $t>(p))
                }
            }};
        }
        Late {
            set_usage: f!(c"AAudioStreamBuilder_setUsage", SetFn),
            set_content_type: f!(c"AAudioStreamBuilder_setContentType", SetFn),
            set_spatialization: f!(c"AAudioStreamBuilder_setSpatializationBehavior", SetFn),
            hardware_format: f!(c"AAudioStream_getHardwareFormat", GetFn),
            hardware_rate: f!(c"AAudioStream_getHardwareSampleRate", GetFn),
        }
    })
}

fn code(format: SampleFormat) -> i32 {
    match format {
        SampleFormat::F32 => ffi::AAUDIO_FORMAT_PCM_FLOAT as i32,
        SampleFormat::I16 => ffi::AAUDIO_FORMAT_PCM_I16 as i32,
        SampleFormat::I24 => ffi::AAUDIO_FORMAT_PCM_I24_PACKED as i32,
        SampleFormat::I32 => ffi::AAUDIO_FORMAT_PCM_I32 as i32,
    }
}

fn from_code(c: i32) -> Option<SampleFormat> {
    [
        SampleFormat::F32,
        SampleFormat::I16,
        SampleFormat::I24,
        SampleFormat::I32,
    ]
    .into_iter()
    .find(|&f| code(f) == c)
}

fn text(r: i32) -> String {
    unsafe {
        let p = ffi::AAudio_convertResultToText(r);
        if p.is_null() {
            format!("AAudio error {r}")
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

fn monotonic_ns() -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec * 1_000_000_000 + ts.tv_nsec
}

unsafe extern "C" fn on_data(
    stream: *mut ffi::AAudioStream,
    user: *mut c_void,
    data: *mut c_void,
    frames: i32,
) -> ffi::aaudio_data_callback_result_t {
    let sh = &mut *(user as *mut Shared);
    let samples = frames.max(0) as usize * sh.channels;
    let out = std::slice::from_raw_parts_mut(data as *mut u8, samples * sh.format.bytes());
    let vol = if sh.fixed_volume {
        1.0
    } else {
        f32::from_bits(sh.volume.load(Ordering::Relaxed))
    };
    let dither = sh.dither.load(Ordering::Relaxed) || vol != 1.0;
    let mut popped = 0u64;
    match sh.slot.try_lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(cons) => sh.encoder.write(out, dither, || match cons.pop() {
                Ok(v) => {
                    popped += 1;
                    // Unity volume leaves samples exactly as decoded.
                    if vol == 1.0 {
                        v
                    } else {
                        v * vol
                    }
                }
                Err(_) => 0.0,
            }),
            None => out.fill(0),
        },
        Err(_) => out.fill(0),
    }
    sh.played.fetch_add(popped, Ordering::Relaxed);

    // Latency: frames written (including this buffer) minus frames the DAC has presented.
    sh.calls = sh.calls.wrapping_add(1);
    if sh.calls % 16 == 1 {
        let (mut pos, mut at) = (0i64, 0i64);
        let r = ffi::AAudioStream_getTimestamp(stream, libc::CLOCK_MONOTONIC, &mut pos, &mut at);
        let latency = if r == ffi::AAUDIO_OK as i32 {
            let written = ffi::AAudioStream_getFramesWritten(stream) + frames as i64;
            let presented = pos + (monotonic_ns() - at) * sh.rate as i64 / 1_000_000_000;
            (written - presented).max(0) as u64
        } else {
            ffi::AAudioStream_getBufferSizeInFrames(stream).max(0) as u64
        };
        sh.latency.store(latency, Ordering::Relaxed);
    }
    ffi::AAUDIO_CALLBACK_RESULT_CONTINUE as i32
}

unsafe extern "C" fn on_error(_stream: *mut ffi::AAudioStream, user: *mut c_void, error: i32) {
    // Usually AAUDIO_ERROR_DISCONNECTED (unplugged or re-routed). The stream can't be reopened
    // from here; the engine sees `lost` and opens a new one.
    let sh = &*(user as *const Shared);
    sh.lost.store(true, Ordering::Relaxed);
    let _ = error;
}

struct Handle {
    stream: *mut ffi::AAudioStream,
    shared: *mut Shared,
}

impl Control for Handle {
    fn play(&self) {
        unsafe { ffi::AAudioStream_requestStart(self.stream) };
    }
    fn pause(&self) {
        unsafe { ffi::AAudioStream_requestPause(self.stream) };
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            ffi::AAudioStream_requestStop(self.stream);
            // After close no callback runs, so the shared state can go.
            ffi::AAudioStream_close(self.stream);
            drop(Box::from_raw(self.shared));
        }
    }
}

/// Opens a stream; `exclusive` overrides the preference so a refused exclusive open can retry
/// shared.
unsafe fn open_stream(
    prefs: &OutputPrefs,
    exclusive: bool,
    rate: u32,
    shared: *mut Shared,
) -> Result<*mut ffi::AAudioStream, String> {
    let mut b: *mut ffi::AAudioStreamBuilder = null_mut();
    let r = ffi::AAudio_createStreamBuilder(&mut b);
    if r != ffi::AAUDIO_OK as i32 {
        return Err(text(r));
    }
    ffi::AAudioStreamBuilder_setDirection(b, ffi::AAUDIO_DIRECTION_OUTPUT as i32);
    ffi::AAudioStreamBuilder_setSampleRate(b, rate as i32);
    ffi::AAudioStreamBuilder_setChannelCount(b, 2);
    // Shared streams go through Android's float mixer, so float loses nothing; exclusive ones
    // take the device's own format unless one was chosen.
    let format = match (prefs.format, exclusive) {
        (Some(f), _) => code(f),
        (None, false) => code(SampleFormat::F32),
        (None, true) => ffi::AAUDIO_FORMAT_UNSPECIFIED as i32,
    };
    ffi::AAudioStreamBuilder_setFormat(b, format);
    ffi::AAudioStreamBuilder_setSharingMode(
        b,
        if exclusive {
            ffi::AAUDIO_SHARING_MODE_EXCLUSIVE as i32
        } else {
            ffi::AAUDIO_SHARING_MODE_SHARED as i32
        },
    );
    // MMAP (exclusive) streams need the low-latency mode.
    ffi::AAudioStreamBuilder_setPerformanceMode(
        b,
        if prefs.low_latency || exclusive {
            ffi::AAUDIO_PERFORMANCE_MODE_LOW_LATENCY as i32
        } else {
            ffi::AAUDIO_PERFORMANCE_MODE_POWER_SAVING as i32
        },
    );
    if prefs.device_id > 0 {
        ffi::AAudioStreamBuilder_setDeviceId(b, prefs.device_id);
    }
    let l = late();
    if let Some(f) = l.set_usage {
        f(b, ffi::AAUDIO_USAGE_MEDIA as i32);
    }
    if let Some(f) = l.set_content_type {
        f(b, ffi::AAUDIO_CONTENT_TYPE_MUSIC as i32);
    }
    if let Some(f) = l.set_spatialization {
        let behavior = if prefs.spatial {
            ffi::AAUDIO_SPATIALIZATION_BEHAVIOR_AUTO as i32
        } else {
            ffi::AAUDIO_SPATIALIZATION_BEHAVIOR_NEVER as i32
        };
        f(b, behavior as i32);
    }
    ffi::AAudioStreamBuilder_setDataCallback(b, Some(on_data), shared as *mut c_void);
    ffi::AAudioStreamBuilder_setErrorCallback(b, Some(on_error), shared as *mut c_void);
    let mut stream: *mut ffi::AAudioStream = null_mut();
    let r = ffi::AAudioStreamBuilder_openStream(b, &mut stream);
    ffi::AAudioStreamBuilder_delete(b);
    if r != ffi::AAUDIO_OK as i32 || stream.is_null() {
        return Err(text(r));
    }
    Ok(stream)
}

impl Sink for AAudioSink {
    fn caps(&mut self) -> Result<Caps, String> {
        // Android's mixer takes any rate; the device's real rates come from the platform's
        // device list (M2-03) as the engine's caps override.
        Ok(Caps {
            rates: vec![],
            any_rate: true,
            channels: 2,
        })
    }

    fn set_prefs(&mut self, prefs: OutputPrefs) {
        self.prefs = prefs;
    }

    fn set_source_bits(&mut self, bits: Option<u32>) {
        self.source_bits = bits;
    }

    fn open(&mut self, rate: u32, volume: Arc<AtomicU32>) -> Result<Stream, String> {
        let slot: Slot = Arc::new(Mutex::new(None));
        let played = Arc::new(AtomicU64::new(0));
        let latency = Arc::new(AtomicU64::new(0));
        let dither = Arc::new(AtomicBool::new(false));
        let lost = Arc::new(AtomicBool::new(false));
        let shared = Box::into_raw(Box::new(Shared {
            slot: slot.clone(),
            played: played.clone(),
            latency: latency.clone(),
            volume,
            dither: dither.clone(),
            lost: lost.clone(),
            encoder: Encoder::new(SampleFormat::F32, 2),
            format: SampleFormat::F32,
            channels: 2,
            rate,
            calls: 0,
            fixed_volume: false,
        }));
        let mut prefs = self.prefs;
        // Bit-perfect: the platform prepares the path for this rate and depth and says which
        // format to send where; without it, play normally.
        let mut bit_perfect = false;
        if prefs.bit_perfect {
            match crate::sink::bit_perfect(rate, self.source_bits) {
                Some((format, device)) => {
                    prefs.format = Some(format);
                    prefs.device_id = device;
                    prefs.exclusive = false;
                    bit_perfect = true;
                }
                None => log::warn!("no bit-perfect path for {rate} Hz; normal output"),
            }
        } else if self.was_bit_perfect {
            crate::sink::bit_perfect(0, None);
        }
        self.was_bit_perfect = bit_perfect;
        let opened = unsafe {
            match open_stream(&prefs, prefs.exclusive, rate, shared) {
                Err(e) if prefs.exclusive => {
                    log::warn!("exclusive output refused ({e}); using shared");
                    open_stream(&prefs, false, rate, shared)
                }
                other => other,
            }
        };
        let stream = match opened {
            Ok(s) => s,
            Err(e) => {
                unsafe { drop(Box::from_raw(shared)) };
                return Err(e);
            }
        };
        // The callback only runs after requestStart, so the shared state can be finished here.
        let handle = Handle { stream, shared };
        let (got_rate, channels, format_code, sharing, perf, device, buffer) = unsafe {
            (
                ffi::AAudioStream_getSampleRate(stream).max(1) as u32,
                ffi::AAudioStream_getChannelCount(stream).max(1) as usize,
                ffi::AAudioStream_getFormat(stream),
                ffi::AAudioStream_getSharingMode(stream),
                ffi::AAudioStream_getPerformanceMode(stream),
                ffi::AAudioStream_getDeviceId(stream),
                ffi::AAudioStream_getBufferSizeInFrames(stream),
            )
        };
        let Some(format) = from_code(format_code) else {
            return Err(format!("output opened with unknown format {format_code}"));
        };
        unsafe {
            let sh = &mut *shared;
            sh.format = format;
            sh.channels = channels;
            sh.rate = got_rate;
            sh.encoder = Encoder::new(format, channels);
            sh.fixed_volume = bit_perfect;
        }
        latency.store(buffer.max(0) as u64, Ordering::Relaxed);
        let l = late();
        let hardware = match (l.hardware_format, l.hardware_rate) {
            (Some(hf), Some(hr)) => unsafe {
                let (f, r) = (hf(stream), hr(stream));
                let name = from_code(f)
                    .map(|f| f.name().to_string())
                    .unwrap_or(format!("format {f}"));
                (r > 0).then(|| format!("{name} {r} Hz"))
            },
            _ => None,
        };
        let info = OutputInfo {
            format,
            exclusive: sharing == ffi::AAUDIO_SHARING_MODE_EXCLUSIVE as i32,
            low_latency: perf == ffi::AAUDIO_PERFORMANCE_MODE_LOW_LATENCY as i32,
            device_id: device,
            buffer_frames: buffer.max(0) as u32,
            hardware,
            bit_perfect,
        };
        let asked = format!(
            "{} {} {rate} Hz",
            prefs.format.map(|f| f.name()).unwrap_or("auto"),
            if prefs.exclusive {
                "exclusive"
            } else {
                "shared"
            }
        );
        log::info!(
            "AAudio asked {asked}; opened {} (device {device})",
            info.describe(got_rate)
        );
        Ok(Stream {
            slot,
            played,
            latency_frames: latency,
            rate: got_rate,
            channels,
            info,
            dither,
            lost,
            control: Box::new(handle),
        })
    }
}
