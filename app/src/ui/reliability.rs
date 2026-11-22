//! Settings → Background playback (M0-13): shows whether the phone lets Loss keep playing and
//! downloading with the screen off, and opens the screens that fix it. The decisions are in
//! `loss_platform::reliability`.

use super::icons::Icon;
use super::settings::{SettingsNav, SettingsPage};
use dioxus::prelude::*;
use loss_i18n::t;
use loss_platform::reliability::{CheckId, CheckState, Status};
use std::time::Duration;

fn title(id: CheckId) -> String {
    match id {
        CheckId::Battery => t!("reliability-battery"),
        CheckId::Background => t!("reliability-background"),
        CheckId::Notifications => t!("reliability-notifications"),
        CheckId::Autostart => t!("reliability-autostart"),
    }
}

fn description(id: CheckId) -> String {
    match id {
        CheckId::Battery => t!("reliability-battery-desc"),
        CheckId::Background => t!("reliability-background-desc"),
        CheckId::Notifications => t!("reliability-notifications-desc"),
        CheckId::Autostart => t!("reliability-autostart-desc"),
    }
}

#[component]
pub fn ReliabilityPage() -> Element {
    let mut status: Signal<Option<Status>> = use_signal(crate::platform::reliability);
    // The owner changes these in Android's settings and comes back: look again now and then.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let now = crate::platform::reliability();
            if *status.peek() != now {
                status.set(now);
            }
        }
    });
    let current = status();
    rsx! {
        button {
            class: "back",
            "aria-label": t!("action-back"),
            onclick: move |_| {
                let mut page = use_context::<SettingsNav>().0;
                page.set(SettingsPage::Main);
            },
            Icon { name: "back" }
        }
        header { class: "page-header", h1 { {t!("reliability-title")} } }
        p { class: "muted pad", {t!("reliability-intro")} }
        match current {
            None => rsx! { p { class: "muted pad", {t!("reliability-only-android")} } },
            Some(s) => rsx! {
                div { class: "card",
                    for c in s.checks() {
                        div { key: "{c.id:?}", class: "setting",
                            Icon {
                                name: match c.state {
                                    CheckState::Ok => "check",
                                    CheckState::NeedsAction => "info",
                                    CheckState::CheckYourself => "settings",
                                },
                                size: 22,
                            }
                            div { class: "row-text",
                                div { class: "title", {title(c.id)} }
                                div { class: "subtitle", {description(c.id)} }
                                div {
                                    class: match c.state {
                                        CheckState::Ok => "small ok", // i18n: ignore (CSS)
                                        CheckState::NeedsAction => "small error", // i18n: ignore (CSS)
                                        CheckState::CheckYourself => "small muted", // i18n: ignore (CSS)
                                    },
                                    match c.state {
                                        CheckState::Ok => t!("reliability-state-ok"),
                                        CheckState::NeedsAction => t!("reliability-state-off"),
                                        CheckState::CheckYourself => t!("reliability-state-unknown"),
                                    }
                                }
                            }
                            if c.state != CheckState::Ok {
                                button {
                                    class: "pill",
                                    onclick: move |_| crate::platform::open_reliability(c.id.screen()),
                                    if c.state == CheckState::NeedsAction { {t!("reliability-fix")} } else { {t!("action-open")} }
                                }
                            }
                        }
                    }
                }
                p { class: "muted small pad", {t!("reliability-footer")} }
            },
        }
    }
}
