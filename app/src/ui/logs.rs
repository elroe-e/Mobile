//! Settings → Logs and diagnostics: the app log, the last crash report, and
//! device/audio-output details for bug reports and DAC checks.

use super::icons::Icon;
use super::settings::{SettingsNav, SettingsPage};
use super::{copy_text, share_text, state};
use crate::diag;
use dioxus::prelude::*;
use loss_i18n::t;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Problems,
    Info,
    Debug,
}

impl Filter {
    fn label(self) -> String {
        match self {
            Filter::All => t!("logs-filter-all"),
            Filter::Problems => t!("logs-filter-problems"),
            Filter::Info => t!("logs-filter-info"),
            Filter::Debug => t!("logs-filter-debug"),
        }
    }
    fn keeps(self, level: log::Level) -> bool {
        match self {
            Filter::All => true,
            Filter::Problems => level <= log::Level::Warn,
            Filter::Info => level == log::Level::Info,
            Filter::Debug => level >= log::Level::Debug,
        }
    }
}

const SHOWN: usize = 400;

#[component]
pub fn LogsPage() -> Element {
    let st = state();
    let mut page = use_context::<SettingsNav>().0;
    let mut query = use_signal(String::new);
    let mut filter = use_signal(|| Filter::All);
    let mut tick = use_signal(|| 0u64);
    let mut crash = use_signal(|| diag::pending_crash().map(|(_, text)| text));
    let mut details = use_signal(|| None::<String>);

    // New lines appear while the page is open.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            tick += 1;
        }
    });
    let _ = tick();

    let q = query().to_lowercase();
    let f = filter();
    let all = diag::lines();
    let total = all.len();
    let lines: Vec<_> = all
        .into_iter()
        .rev()
        .filter(|l| f.keeps(l.level))
        .filter(|l| {
            q.is_empty()
                || l.message.to_lowercase().contains(&q)
                || l.target.to_lowercase().contains(&q)
        })
        .take(SHOWN)
        .collect();

    rsx! {
        button { class: "back", onclick: move |_| page.set(SettingsPage::Main), Icon { name: "back" } }
        header { class: "page-header", h1 { {t!("logs-title")} } }

        if let Some(report) = crash() {
            h2 { class: "section-title", {t!("logs-last-crash")} }
            div { class: "card",
                p { class: "small muted", {t!("logs-crash-text")} }
                pre { class: "pre-box", "{report}" }
                div { class: "hero-actions start",
                    button {
                        class: "pill",
                        onclick: {
                            let r = report.clone();
                            move |_| copy_text(st, &t!("logs-crash-report"), &r)
                        },
                        Icon { name: "check", size: 18 }
                        {t!("action-copy")}
                    }
                    button {
                        class: "pill",
                        onclick: {
                            let r = report.clone();
                            move |_| share_text(st, &t!("logs-crash-report"), &r)
                        },
                        {t!("action-share")}
                    }
                    button {
                        class: "pill",
                        onclick: move |_| {
                            diag::dismiss_crashes();
                            crash.set(None);
                        },
                        {t!("action-dismiss")}
                    }
                }
            }
        }

        h2 { class: "section-title", {t!("logs-device")} }
        div { class: "card",
            p { class: "small muted",
                {t!("logs-device-text")}
            }
            if let Some(text) = details() {
                pre { class: "pre-box", "{text}" }
            }
            div { class: "hero-actions start",
                button {
                    class: "pill",
                    onclick: move |_| details.set(Some(crate::platform::diagnostics())),
                    if details().is_some() { {t!("action-refresh")} } else { {t!("action-show")} }
                }
                button {
                    class: "pill",
                    onclick: move |_| copy_text(st, &t!("logs-diagnostics"), &full_report()),
                    {t!("action-copy")}
                }
                button {
                    class: "pill",
                    onclick: move |_| share_text(st, &t!("logs-diagnostics"), &full_report()),
                    {t!("action-share")}
                }
            }
        }

        h2 { class: "section-title", {t!("logs-app-log")} }
        div { class: "search-box",
            Icon { name: "search", size: 22 }
            input {
                r#type: "search",
                placeholder: t!("logs-search"),
                initial_value: "{query}",
                oninput: move |e| query.set(e.value()),
            }
        }
        div { class: "chips",
            for opt in [Filter::All, Filter::Problems, Filter::Info, Filter::Debug] {
                button {
                    class: if f == opt { "chip selected" } else { "chip" },
                    onclick: move |_| filter.set(opt),
                    "{opt.label()}"
                }
            }
        }
        div { class: "toolbar",
            span { class: "muted small", {t!("logs-count", shown = lines.len(), total = total)} }
            div { class: "header-actions",
                button { class: "text-btn", onclick: move |_| copy_text(st, &t!("logs-log"), &diag::export_text()), {t!("action-copy")} }
                button { class: "text-btn", onclick: move |_| share_text(st, &t!("logs-log"), &diag::export_text()), {t!("action-share")} }
                button {
                    class: "text-btn",
                    onclick: move |_| {
                        diag::clear();
                        st.toast(t!("logs-cleared"));
                    },
                    {t!("action-clear")}
                }
            }
        }
        if lines.is_empty() {
            p { class: "muted pad", {t!("logs-empty")} }
        }
        div { class: "log-list",
            for (i , l) in lines.iter().enumerate() {
                div { key: "{i}-{l.time_ms}", class: "log-line",
                    span { class: "log-meta",
                        span { class: "log-level lvl-{l.level.as_str().to_lowercase()}", "{l.level}" }
                        " {l.target}"
                    }
                    span { class: "log-msg", "{l.message}" }
                }
            }
        }
    }
}

/// Device details plus the log, for bug reports.
fn full_report() -> String {
    format!(
        "{}\nSettings: detailed logging {}\n\n{}", // i18n: ignore (bug reports stay English)
        crate::platform::diagnostics(),
        if crate::services::get().settings().detailed_logging {
            "on"
        } else {
            "off"
        },
        diag::export_text()
    )
}
