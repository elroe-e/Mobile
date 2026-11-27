//! Loss' translations, using Project Fluent.
//!
//! Messages live in `locales/<lang>/loss.ftl` and are compiled into the app. UI code asks for text
//! with [`t!`]: `t!("settings-title")` or `t!("downloads-left", count = 3)`. A message missing in
//! the current language falls back to English, and a message missing in English shows its id
//! (and logs it), so a mistake is visible instead of blank.
//!
//! Two pseudo-locales help testing: `en-XA` (accented and longer, to find text that isn't
//! translated or doesn't fit) and `ar-XB` (mirrored, right to left).

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use std::borrow::Cow;
use std::sync::{Arc, RwLock};
use unic_langid::LanguageIdentifier;

/// A language Loss can show: its tag and its name in that language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    pub tag: &'static str,
    pub name: &'static str,
    /// Only for testing (pseudo-locales).
    pub pseudo: bool,
}

/// Every language Loss has, English first.
pub const LANGUAGES: &[Language] = &[
    Language {
        tag: "en",
        name: "English",
        pseudo: false,
    },
    Language {
        tag: "en-XA",
        name: "[Ƥşḗŭḓǿ]",
        pseudo: true,
    },
    Language {
        tag: "ar-XB",
        name: "Pseudo RTL",
        pseudo: true,
    },
];

const EN: &str = include_str!("../locales/en/loss.ftl");

/// Languages written right to left.
const RTL: &[&str] = &["ar", "he", "fa", "ur", "ps", "sd", "yi", "ckb"];

type Bundle = FluentBundle<FluentResource>;

struct State {
    tag: &'static str,
    bundle: Bundle,
    /// English, for messages the current language lacks.
    fallback: Option<Bundle>,
}

static STATE: RwLock<Option<Arc<State>>> = RwLock::new(None);

fn accented(s: &str) -> Cow<'_, str> {
    fluent_pseudo::transform(s, false, true)
}

fn flipped(s: &str) -> Cow<'_, str> {
    fluent_pseudo::transform(s, true, false)
}

fn bundle(tag: &'static str) -> Bundle {
    let id: LanguageIdentifier = tag.parse().unwrap_or_default();
    let mut b = FluentBundle::new_concurrent(vec![id]);
    // Unicode isolation marks around arguments confuse copying and tests; the web view handles
    // mixed directions by itself.
    b.set_use_isolating(false);
    match tag {
        "en-XA" => b.set_transform(Some(accented)),
        "ar-XB" => b.set_transform(Some(flipped)),
        _ => {}
    }
    let res = match FluentResource::try_new(EN.to_string()) {
        Ok(r) => r,
        Err((r, errors)) => {
            log::error!("en/loss.ftl has {} syntax errors", errors.len());
            r
        }
    };
    if let Err(errors) = b.add_resource(res) {
        log::error!("en/loss.ftl: {errors:?}");
    }
    b
}

/// The best available language for a BCP 47 tag such as `en-GB` or `fr` (English if none).
pub fn negotiate(wanted: &str) -> &'static str {
    let wanted = wanted.replace('_', "-");
    if let Some(l) = LANGUAGES
        .iter()
        .find(|l| l.tag.eq_ignore_ascii_case(&wanted))
    {
        return l.tag;
    }
    let lang = wanted.split('-').next().unwrap_or("");
    LANGUAGES
        .iter()
        .find(|l| !l.pseudo && l.tag.split('-').next() == Some(lang))
        .map(|l| l.tag)
        .unwrap_or("en")
}

/// Switches the language. `preference` is a tag, or "system" to follow `system` (the device's
/// language, if known).
pub fn set_language(preference: &str, system: Option<&str>) -> &'static str {
    let tag = if preference == "system" || preference.is_empty() {
        negotiate(system.unwrap_or("en"))
    } else {
        negotiate(preference)
    };
    let state = State {
        tag,
        bundle: bundle(tag),
        fallback: (tag != "en").then(|| bundle("en")),
    };
    if let Ok(mut s) = STATE.write() {
        *s = Some(Arc::new(state));
    }
    log::info!("language: {tag}");
    tag
}

fn state() -> Arc<State> {
    if let Some(s) = STATE.read().ok().and_then(|s| s.clone()) {
        return s;
    }
    set_language("en", None);
    STATE
        .read()
        .ok()
        .and_then(|s| s.clone())
        .expect("language set")
}

/// The current language's tag.
pub fn current() -> &'static str {
    state().tag
}

/// Whether the current language is written right to left.
pub fn is_rtl() -> bool {
    let tag = current();
    tag == "ar-XB" || RTL.contains(&tag.split('-').next().unwrap_or(""))
}

/// An argument for a message.
pub enum Arg {
    Text(String),
    Number(f64),
}

macro_rules! number_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(n: $t) -> Self {
                Arg::Number(n as f64)
            }
        }
    )*};
}
number_arg!(i32, i64, u32, u64, usize, f32, f64);

impl From<&str> for Arg {
    fn from(s: &str) -> Self {
        Arg::Text(s.to_string())
    }
}
impl From<String> for Arg {
    fn from(s: String) -> Self {
        Arg::Text(s)
    }
}
impl From<&String> for Arg {
    fn from(s: &String) -> Self {
        Arg::Text(s.clone())
    }
}

fn format(b: &Bundle, id: &str, args: Option<&FluentArgs>) -> Option<String> {
    let msg = b.get_message(id)?;
    let pattern = msg.value()?;
    let mut errors = vec![];
    let out = b.format_pattern(pattern, args, &mut errors);
    if !errors.is_empty() {
        log::warn!("translation {id}: {errors:?}");
    }
    Some(out.into_owned())
}

/// The text of message `id` with these arguments.
pub fn tr_args(id: &str, args: &[(&str, Arg)]) -> String {
    let s = state();
    let fargs = (!args.is_empty()).then(|| {
        let mut a = FluentArgs::new();
        for (k, v) in args {
            match v {
                Arg::Text(t) => a.set(*k, FluentValue::from(t.clone())),
                Arg::Number(n) => a.set(*k, FluentValue::from(*n)),
            }
        }
        a
    });
    format(&s.bundle, id, fargs.as_ref())
        .or_else(|| {
            s.fallback
                .as_ref()
                .and_then(|f| format(f, id, fargs.as_ref()))
        })
        .unwrap_or_else(|| {
            log::warn!("missing translation: {id}");
            id.to_string()
        })
}

/// The English text of message `id` with these arguments, whatever language is set (for
/// reports people send us).
pub fn tr_en_args(id: &str, args: &[(&str, Arg)]) -> String {
    let mut fargs = FluentArgs::new();
    for (k, v) in args {
        match v {
            Arg::Text(t) => fargs.set(*k, FluentValue::from(t.clone())),
            Arg::Number(n) => fargs.set(*k, FluentValue::from(*n)),
        }
    }
    format(&bundle("en"), id, (!args.is_empty()).then_some(&fargs))
        .unwrap_or_else(|| id.to_string())
}

/// The text of message `id`, or None if no language has it (no warning: for optional text).
pub fn tr_opt(id: &str) -> Option<String> {
    let s = state();
    format(&s.bundle, id, None).or_else(|| s.fallback.as_ref().and_then(|f| format(f, id, None)))
}

/// The text of message `id`.
pub fn tr(id: &str) -> String {
    tr_args(id, &[])
}

/// Whether English has message `id` (for tests that check every id a table refers to).
pub fn has(id: &str) -> bool {
    bundle("en")
        .get_message(id)
        .is_some_and(|m| m.value().is_some())
}

/// Translated text: `t!("id")` or `t!("id", name = value, …)`.
#[macro_export]
macro_rules! t {
    ($id:literal) => {
        $crate::tr($id)
    };
    ($id:literal, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::tr_args($id, &[$((stringify!($k), $crate::Arg::from($v))),+])
    };
}

#[cfg(test)]
mod tests;
