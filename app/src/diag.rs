//! Diagnostics: the app's logger, the in-app log viewer's data, crash reports.
//!
//! Every log line goes to the platform log (logcat / stderr), a ring buffer the
//! Logs screen reads, and a rotating file in the app's data folder. A panic hook
//! saves a crash report that the next launch offers to show.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

const RING_CAPACITY: usize = 3000;
const FILE_LIMIT: u64 = 1024 * 1024;
const CRASH_LOG_LINES: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    /// Milliseconds since the Unix epoch.
    pub time_ms: u64,
    pub level: log::Level,
    pub target: String,
    pub message: String,
}

impl LogLine {
    pub fn format(&self) -> String {
        format!(
            "{} {:<5} {}: {}",
            format_time(self.time_ms),
            self.level,
            self.target,
            self.message
        )
    }
}

/// `HH:MM:SS.mmm` in UTC (log files and reports carry the date separately).
fn format_time(ms: u64) -> String {
    let s = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        (s / 3600) % 24,
        (s / 60) % 60,
        s % 60,
        ms % 1000
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct State {
    ring: VecDeque<LogLine>,
    file: Option<File>,
    dir: Option<PathBuf>,
}

struct Logger {
    state: Mutex<State>,
    detailed: AtomicBool,
    #[cfg(target_os = "android")]
    platform: android_logger::AndroidLogger,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

fn logger() -> &'static Logger {
    LOGGER.get_or_init(|| Logger {
        state: Mutex::new(State {
            ring: VecDeque::with_capacity(RING_CAPACITY),
            file: None,
            dir: None,
        }),
        detailed: AtomicBool::new(false),
        #[cfg(target_os = "android")]
        platform: android_logger::AndroidLogger::new(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Trace)
                .with_tag("Loss"),
        ),
    })
}

impl Logger {
    fn max_level(&self) -> log::LevelFilter {
        if self.detailed.load(Ordering::Relaxed) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        }
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Other crates' chatter stays at warnings unless detailed logging is on.
        let own = metadata.target().starts_with("loss");
        if own || self.detailed.load(Ordering::Relaxed) {
            metadata.level() <= self.max_level()
        } else {
            metadata.level() <= log::Level::Warn
        }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        #[cfg(target_os = "android")]
        self.platform.log(record);
        #[cfg(not(target_os = "android"))]
        eprintln!(
            "[{}] {}: {}",
            record.level(),
            record.target(),
            record.args()
        );

        let line = LogLine {
            time_ms: now_ms(),
            level: record.level(),
            target: record.target().to_string(),
            message: record.args().to_string(),
        };
        let Ok(mut st) = self.state.lock() else {
            return;
        };
        if let Some(f) = st.file.as_mut() {
            let _ = writeln!(f, "{}", line.format());
        }
        rotate_if_needed(&mut st);
        if st.ring.len() == RING_CAPACITY {
            st.ring.pop_front();
        }
        st.ring.push_back(line);
    }

    fn flush(&self) {
        if let Ok(mut st) = self.state.lock() {
            if let Some(f) = st.file.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join("loss.log")
}

fn rotate_if_needed(st: &mut State) {
    let Some(dir) = st.dir.clone() else { return };
    let path = log_path(&dir);
    let too_big = fs::metadata(&path)
        .map(|m| m.len() > FILE_LIMIT)
        .unwrap_or(false);
    if too_big {
        st.file = None;
        let _ = fs::rename(&path, dir.join("loss.1.log"));
        st.file = open_log(&path);
    }
}

fn open_log(path: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Installs the logger and the panic hook. Call first thing in `main`.
pub fn init() {
    let l = logger();
    if log::set_logger(l).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
    install_panic_hook();
}

/// Starts writing the log file once the data folder is known, and writes the
/// lines logged before that.
pub fn set_dir(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let Ok(mut st) = logger().state.lock() else {
        return;
    };
    st.dir = Some(dir.to_path_buf());
    st.file = open_log(&log_path(dir));
    let early: Vec<String> = st.ring.iter().map(LogLine::format).collect();
    if let Some(f) = st.file.as_mut() {
        let _ = writeln!(f, "---- Loss {} started ----", env!("CARGO_PKG_VERSION"));
        for l in early {
            let _ = writeln!(f, "{l}");
        }
    }
}

/// "Detailed logging" setting: debug lines and other libraries' info lines.
pub fn set_detailed(on: bool) {
    logger().detailed.store(on, Ordering::Relaxed);
}

/// Lines in the ring buffer, oldest first.
pub fn lines() -> Vec<LogLine> {
    logger()
        .state
        .lock()
        .map(|st| st.ring.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn clear() {
    if let Ok(mut st) = logger().state.lock() {
        st.ring.clear();
        if let Some(dir) = st.dir.clone() {
            st.file = None;
            let _ = fs::remove_file(dir.join("loss.1.log"));
            let _ = File::create(log_path(&dir));
            st.file = open_log(&log_path(&dir));
        }
    }
}

/// The whole ring buffer as text, for copying or sharing.
pub fn export_text() -> String {
    let mut out = format!("Loss {} log\n", env!("CARGO_PKG_VERSION"));
    for l in lines() {
        out.push_str(&l.format());
        out.push('\n');
    }
    out
}

// ---- Crash reports ----

fn crash_dir() -> Option<PathBuf> {
    logger()
        .state
        .lock()
        .ok()?
        .dir
        .as_ref()
        .map(|d| d.join("crashes"))
}

/// Builds the text of a crash report.
pub fn crash_report(message: &str, location: &str, thread: &str, backtrace: &str) -> String {
    let recent: Vec<String> = {
        let all = lines();
        let skip = all.len().saturating_sub(CRASH_LOG_LINES);
        all[skip..].iter().map(LogLine::format).collect()
    };
    format!(
        "Loss {version} stopped unexpectedly\n\
         Time (UTC ms): {time}\n\
         Thread: {thread}\n\
         Where: {location}\n\
         What: {message}\n\n\
         Recent log:\n{log}\n\n\
         Backtrace:\n{backtrace}\n",
        version = env!("CARGO_PKG_VERSION"),
        time = now_ms(),
        log = recent.join("\n"),
    )
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        log::error!("panic in {thread} at {location}: {message}");
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        if let Some(dir) = crash_dir() {
            let _ = fs::create_dir_all(&dir);
            let path = dir.join(format!("crash-{}.txt", now_ms()));
            let _ = fs::write(path, crash_report(&message, &location, &thread, &bt));
        }
        logger_flush();
        previous(info);
    }));
}

fn logger_flush() {
    log::logger().flush();
}

/// The newest crash report that hasn't been dismissed: (file, text).
pub fn pending_crash() -> Option<(PathBuf, String)> {
    let dir = crash_dir()?;
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "txt").unwrap_or(false))
        .collect();
    files.sort();
    let newest = files.pop()?;
    let text = fs::read_to_string(&newest).ok()?;
    Some((newest, text))
}

/// Moves crash reports out of the way once the user has seen them.
pub fn dismiss_crashes() {
    let Some(dir) = crash_dir() else { return };
    let seen = dir.join("seen");
    let _ = fs::create_dir_all(&seen);
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Some(name) = p.file_name() {
                    let _ = fs::rename(&p, seen.join(name));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_lines_and_reports() {
        let l = LogLine {
            time_ms: 3_723_004,
            level: log::Level::Warn,
            target: "loss::audio".into(),
            message: "hello".into(),
        };
        assert_eq!(l.format(), "01:02:03.004 WARN  loss::audio: hello");
        let r = crash_report("boom", "src/x.rs:1", "main", "bt");
        assert!(r.contains("What: boom"));
        assert!(r.contains("Where: src/x.rs:1"));
    }

    #[test]
    fn ring_file_and_crashes() {
        init();
        let dir = std::env::temp_dir().join(format!("loss-diag-{}", std::process::id()));
        set_dir(&dir);
        log::info!(target: "loss::test", "first line");
        assert!(lines().iter().any(|l| l.message == "first line"));
        log::logger().flush();
        let text = fs::read_to_string(dir.join("loss.log")).unwrap();
        assert!(text.contains("first line"));

        let crashes = dir.join("crashes");
        fs::create_dir_all(&crashes).unwrap();
        fs::write(crashes.join("crash-1.txt"), "old").unwrap();
        fs::write(crashes.join("crash-2.txt"), "new").unwrap();
        assert_eq!(pending_crash().unwrap().1, "new");
        dismiss_crashes();
        assert!(pending_crash().is_none());

        clear();
        assert!(lines().is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
