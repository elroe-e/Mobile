//! The audio-info sheet (M2-18): the rows come from `crate::audio_info`.

use super::icons::Icon;
use super::kit::Sheet;
use crate::audio_info::{build, current_facts, report};
use dioxus::prelude::*;
use loss_i18n::t;

/// The sheet. Opened from the quality label in the player.
#[component]
pub fn AudioInfoSheet(open: Signal<bool>) -> Element {
    let st = super::state();
    if !open() {
        return rsx! {};
    }
    let groups =
        current_facts(|f| build(f, &|id, a| loss_i18n::tr_args(id, a))).unwrap_or_default();
    let text_report = report(&groups);
    rsx! {
        Sheet { open, title: t!("info-title"),
            div { class: "info-sheet",
                for g in groups.iter() {
                    div { class: "info-group", key: "{g.id}",
                        div { class: "info-group-title", {loss_i18n::tr(g.id)} }
                        for (i , r) in g.rows.iter().enumerate() {
                            div { class: "info-row", key: "{i}",
                                span { class: "info-label", {loss_i18n::tr(r.id)} }
                                span { class: "info-value", "{r.value}" }
                            }
                        }
                    }
                }
                button {
                    class: "text-btn info-copy",
                    onclick: move |_| super::copy_text(st, "Loss audio info", &text_report), // i18n: ignore
                    Icon { name: "copy", size: 18 }
                    {t!("info-copy")}
                }
            }
        }
    }
}
