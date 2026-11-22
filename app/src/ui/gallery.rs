//! Settings → Help → Component gallery (debug builds only, M0-10): every UI-kit piece on one
//! page, for checking looks, languages and right-to-left layout. `scripts/ui-smoke.sh` visits it.

use super::icons::Icon;
use super::kit::{haptic, Dialog, Haptic, Marquee, Sheet, SheetItem, VirtualList};
use super::settings::{SettingsNav, SettingsPage};
use super::state;
use dioxus::prelude::*;
use loss_i18n::t;

#[component]
pub fn GalleryPage() -> Element {
    let st = state();
    let mut sheet = use_signal(|| false);
    let mut dialog = use_signal(|| false);
    let mut seg = use_signal(|| 1usize);
    let mut on = use_signal(|| true);
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
        header { class: "page-header", h1 { {t!("gallery-title")} } }

        h2 { class: "section-title", {t!("gallery-buttons")} }
        div { class: "card",
            div { class: "hero-actions start",
                button { class: "primary", Icon { name: "play", size: 20 } {t!("action-play")} }
                button { class: "pill", Icon { name: "shuffle", size: 18 } {t!("action-shuffle")} }
                button { class: "text-btn", {t!("action-play-all")} }
                button { class: "icon-btn", "aria-label": t!("action-more"), Icon { name: "more" } }
            }
            div { class: "stepper",
                for n in 1..=3usize {
                    button {
                        class: if seg() == n { "seg active" } else { "seg" },
                        onclick: move |_| {
                            haptic(Haptic::Tick);
                            seg.set(n);
                        },
                        "{n}"
                    }
                }
            }
            div { class: "setting", onclick: move |_| on.set(!on()),
                div { class: "row-text", div { class: "title", {t!("gallery-switch")} } }
                div { class: if on() { "switch on" } else { "switch" }, div { class: "knob" } }
            }
        }

        h2 { class: "section-title", {t!("gallery-layers")} }
        div { class: "card",
            div { class: "hero-actions start",
                button { class: "pill", onclick: move |_| sheet.set(true), {t!("gallery-open-sheet")} }
                button { class: "pill", onclick: move |_| dialog.set(true), {t!("gallery-open-dialog")} }
                button {
                    class: "pill",
                    onclick: move |_| st.toast_undo(t!("gallery-snackbar"), move || st.toast(t!("gallery-undone"))),
                    {t!("gallery-show-snackbar")}
                }
                button { class: "pill", onclick: move |_| haptic(Haptic::LongPress), {t!("gallery-vibrate")} }
            }
            p { class: "muted small", {t!("gallery-long-press")} }
            div {
                class: "row",
                oncontextmenu: move |e| {
                    e.prevent_default();
                    haptic(Haptic::LongPress);
                    sheet.set(true);
                },
                div { class: "row-text", div { class: "title", {t!("gallery-long-press-row")} } }
            }
        }
        Sheet { open: sheet, title: t!("gallery-sheet-title"),
            SheetItem { icon: "play_next", label: t!("action-play-next"), onclick: move |_| sheet.set(false) }
            SheetItem { icon: "queue", label: t!("action-add-queue"), onclick: move |_| sheet.set(false) }
        }
        Dialog {
            open: dialog,
            title: t!("gallery-dialog-title"),
            text: t!("gallery-dialog-text"),
            confirm: t!("gallery-dialog-confirm"),
            danger: true,
            on_confirm: move |_| st.toast(t!("gallery-confirmed")),
        }

        h2 { class: "section-title", {t!("gallery-marquee")} }
        div { class: "card",
            div { style: "max-width: 220px",
                Marquee { text: t!("gallery-marquee-text"), class: "title" }
            }
        }

        h2 { class: "section-title", {t!("gallery-virtual-list")} }
        VirtualList {
            id: "gallery-vlist",
            len: 10_000,
            row_height: 56.0,
            render: move |i: usize| rsx! {
                div { class: "row",
                    div { class: "row-text",
                        div { class: "title", {t!("gallery-row", number = i + 1)} }
                    }
                }
            },
        }
    }
}
