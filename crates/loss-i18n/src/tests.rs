use super::*;
use std::sync::Mutex;

// The language is global; tests that change it take this lock.
static LOCK: Mutex<()> = Mutex::new(());

#[test]
fn english_file_has_no_errors() {
    assert!(FluentResource::try_new(EN.to_string()).is_ok());
    let b = bundle("en");
    let res = FluentResource::try_new(EN.to_string()).unwrap();
    // Every message formats without errors when given plausible arguments.
    let mut args = FluentArgs::new();
    for k in [
        "count", "songs", "plays", "albums", "running", "done", "failed", "shown", "total",
        "owned", "attempt", "number", "channels", "bits", "pct", "percent",
    ] {
        args.set(k, FluentValue::from(2));
    }
    for k in [
        "title", "query", "folder", "speed", "length", "error", "version", "name", "rates",
        "format", "min", "max", "mode", "reason", "list", "db", "value", "device", "source", "rig",
    ] {
        args.set(k, FluentValue::from("x"));
    }
    for entry in res.entries() {
        if let fluent_syntax::ast::Entry::Message(m) = entry {
            let msg = b.get_message(m.id.name).unwrap();
            let mut errors = vec![];
            b.format_pattern(msg.value().unwrap(), Some(&args), &mut errors);
            assert!(errors.is_empty(), "{}: {errors:?}", m.id.name);
        }
    }
}

#[test]
fn messages_plurals_and_fallbacks() {
    let _g = LOCK.lock().unwrap();
    set_language("en", None);
    assert_eq!(t!("tab-home"), "Home");
    assert_eq!(t!("notify-songs-left", count = 1), "1 song left");
    assert_eq!(t!("notify-songs-left", count = 3), "3 songs left");
    assert_eq!(t!("home-downloads-tile", running = 0), "Downloads");
    assert_eq!(t!("home-downloads-tile", running = 2), "Downloads · 2");
    assert_eq!(t!("job-failed", error = "timeout"), "Failed: timeout");
    // A missing message shows its id instead of nothing.
    assert_eq!(tr("no-such-message"), "no-such-message");
    assert_eq!(tr_opt("no-such-message"), None);
}

#[test]
fn pseudo_locales() {
    let _g = LOCK.lock().unwrap();
    set_language("en-XA", None);
    let accented = t!("tab-library");
    assert_ne!(accented, "Library");
    assert!(accented.chars().count() >= "Library".len());
    assert!(!is_rtl());
    // Arguments are not transformed.
    assert!(t!("job-failed", error = "timeout").contains("timeout"));
    set_language("ar-XB", None);
    assert!(is_rtl());
    assert_ne!(t!("tab-library"), "Library");
    set_language("en", None);
}

#[test]
fn language_choice() {
    assert_eq!(negotiate("en-GB"), "en");
    assert_eq!(negotiate("en_US"), "en");
    assert_eq!(negotiate("fr-FR"), "en"); // not translated yet
    assert_eq!(negotiate("en-XA"), "en-XA");
    let _g = LOCK.lock().unwrap();
    assert_eq!(set_language("system", Some("de-DE")), "en");
    assert_eq!(set_language("system", None), "en");
    assert_eq!(current(), "en");
}
