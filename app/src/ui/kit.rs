//! Loss' UI kit (M0-10): building blocks the screens share.
//!
//! * Layers: sheets and dialogs register while open, and Back closes the top one first.
//! * [`Sheet`] (bottom sheet), [`SheetItem`], [`Dialog`].
//! * Snackbar with Undo: [`super::AppState::toast_undo`].
//! * [`VirtualList`]: renders only the rows near the screen, for libraries with thousands of songs.
//! * [`Marquee`]: long titles scroll sideways instead of being cut off.
//! * [`haptic`]: vibration feedback through the platform bridge.

use super::icons::Icon;
use dioxus::prelude::*;
use loss_i18n::t;
use std::sync::atomic::{AtomicU64, Ordering};

// ---- Layers ----

/// Open sheets and dialogs, newest last.
#[derive(Clone, Copy)]
pub struct Layers(Signal<Vec<(u64, Signal<bool>)>>);

pub fn use_provide_layers() -> Layers {
    use_context_provider(|| Layers(Signal::new(Vec::new())))
}

impl Layers {
    /// Closes the newest open layer. Returns false if none was open.
    pub fn back(mut self) -> bool {
        let top = self.0.write().pop();
        match top {
            Some((_, mut open)) => {
                open.set(false);
                true
            }
            None => false,
        }
    }
}

/// Registers `open` as a layer while it is true, so Back closes it.
fn use_layer(open: Signal<bool>) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let layers = use_context::<Layers>();
    let id = use_hook(|| NEXT.fetch_add(1, Ordering::Relaxed));
    use_effect(move || {
        let is_open = open();
        let mut list = layers.0;
        let mut v = list.write();
        v.retain(|(i, _)| *i != id);
        if is_open {
            v.push((id, open));
        }
    });
    use_drop(move || {
        let mut list = layers.0;
        if let Ok(mut v) = list.try_write() {
            v.retain(|(i, _)| *i != id);
        };
    });
}

// ---- Sheet and dialog ----

/// A bottom sheet over everything, with a scrim. Tapping the scrim or pressing Back closes it.
#[component]
pub fn Sheet(
    mut open: Signal<bool>,
    #[props(default)] title: String,
    children: Element,
) -> Element {
    use_layer(open);
    if !open() {
        return rsx! {};
    }
    rsx! {
        div { class: "sheet-scrim", onclick: move |_| open.set(false) }
        div { class: "sheet", role: "dialog",
            div { class: "sheet-handle" }
            if !title.is_empty() {
                div { class: "sheet-title", "{title}" }
            }
            {children}
        }
    }
}

/// One action in a sheet.
#[component]
pub fn SheetItem(icon: &'static str, label: String, onclick: EventHandler<()>) -> Element {
    rsx! {
        button { class: "sheet-item", onclick: move |_| onclick.call(()),
            Icon { name: icon, size: 22 }
            span { "{label}" }
        }
    }
}

/// A confirmation dialog: title, text, Cancel and a confirm button.
#[component]
pub fn Dialog(
    mut open: Signal<bool>,
    title: String,
    #[props(default)] text: String,
    confirm: String,
    on_confirm: EventHandler<()>,
    /// The action destroys something (shown in the error colour).
    #[props(default)]
    danger: bool,
) -> Element {
    use_layer(open);
    if !open() {
        return rsx! {};
    }
    rsx! {
        div { class: "dialog-scrim", onclick: move |_| open.set(false) }
        div { class: "dialog", role: "alertdialog",
            h2 { class: "dialog-title", "{title}" }
            if !text.is_empty() {
                p { class: "dialog-text", "{text}" }
            }
            div { class: "dialog-actions",
                button { class: "text-btn", onclick: move |_| open.set(false), {t!("action-cancel")} }
                button {
                    class: if danger { "text-btn danger" } else { "text-btn" },
                    onclick: move |_| {
                        open.set(false);
                        on_confirm.call(());
                    },
                    "{confirm}"
                }
            }
        }
    }
}

// ---- Haptics ----

/// Kinds of vibration feedback (Android `HapticFeedbackConstants`).
#[derive(Clone, Copy, Debug)]
pub enum Haptic {
    /// A long press opened something.
    LongPress,
    /// A small step: a segment, a chip, a slider detent.
    Tick,
    /// Something was confirmed or switched on.
    Confirm,
}

pub fn haptic(kind: Haptic) {
    crate::platform::haptic(match kind {
        Haptic::LongPress => 0, // LONG_PRESS
        Haptic::Tick => 4,      // CLOCK_TICK
        Haptic::Confirm => 1,   // VIRTUAL_KEY (CONFIRM needs Android 11; the glue upgrades it)
    });
}

// ---- Virtual list ----

/// The window of rows to render for a list at `top` (its top edge relative to the viewport, in
/// px), in a viewport `height` px tall. Rows are `row` px tall; `overscan` screens are added
/// above and below, and the start snaps to `chunk` rows so small scrolls don't re-render.
pub fn visible_window(
    len: usize,
    top: f64,
    height: f64,
    row: f64,
    overscan: f64,
    chunk: usize,
) -> (usize, usize) {
    if len == 0 || row <= 0.0 {
        return (0, 0);
    }
    let first_visible = ((-top) / row).floor().max(0.0) as usize;
    let per_screen = (height / row).ceil().max(1.0) as usize;
    let extra = (per_screen as f64 * overscan).ceil() as usize;
    let start = first_visible.saturating_sub(extra) / chunk.max(1) * chunk.max(1);
    let end = (first_visible + per_screen + extra).min(len);
    (start.min(end), end)
}

/// A list that renders only the rows near the screen. Rows must all be `row_height` px tall.
/// `render(i)` draws row `i`.
#[component]
pub fn VirtualList(
    id: String,
    len: usize,
    row_height: f64,
    render: Callback<usize, Element>,
) -> Element {
    // Where the list is on screen: (top of the list relative to the viewport, viewport height).
    let mut view = use_signal(|| (0.0f64, 900.0f64));
    let el_id = id.clone();
    use_future(move || {
        let el_id = el_id.clone();
        async move {
            let js = format!(
                r#"(function() {{
                    let queued = false;
                    const report = () => {{
                        queued = false;
                        const el = document.getElementById({id});
                        if (el) dioxus.send([el.getBoundingClientRect().top, window.innerHeight]);
                    }};
                    const ask = () => {{ if (!queued) {{ queued = true; requestAnimationFrame(report); }} }};
                    window.addEventListener('scroll', ask, {{ passive: true }});
                    window.addEventListener('resize', ask);
                    setTimeout(report, 50);
                }})();"#,
                id = serde_json::to_string(&el_id).unwrap_or_default()
            );
            let mut ev = document::eval(&js);
            while let Ok(v) = ev.recv::<(f64, f64)>().await {
                if *view.peek() != v {
                    view.set(v);
                }
            }
        }
    });
    let (top, height) = view();
    let (start, end) = visible_window(len, top, height, row_height, 1.0, 20);
    let before = start as f64 * row_height;
    let after = (len - end) as f64 * row_height;
    rsx! {
        div { id: "{id}", class: "vlist",
            div { style: "height: {before}px" }
            for i in start..end {
                div { key: "{i}", class: "vrow", style: "height: {row_height}px", {render.call(i)} }
            }
            div { style: "height: {after}px" }
        }
    }
}

// ---- Marquee ----

/// Text on one line that scrolls sideways when it doesn't fit (instead of "…").
#[component]
pub fn Marquee(text: String, #[props(default)] class: String) -> Element {
    // The CSS animation only runs when the text overflows; a script measures it.
    let id = use_hook(|| {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        format!("mq-{}", NEXT.fetch_add(1, Ordering::Relaxed))
    });
    let measure_id = id.clone();
    let measured_text = text.clone();
    use_effect(use_reactive!(|measured_text| {
        let _ = &measured_text;
        let js = format!(
            r#"requestAnimationFrame(() => {{
                const el = document.getElementById({id});
                if (!el) return;
                const inner = el.firstElementChild;
                const over = inner.scrollWidth - el.clientWidth;
                el.classList.toggle('scrolls', over > 4);
                el.style.setProperty('--mq-shift', (-over - 16) + 'px');
                el.style.setProperty('--mq-time', Math.max(6, over / 25) + 's');
            }});"#,
            id = serde_json::to_string(&measure_id).unwrap_or_default()
        );
        document::eval(&js);
    }));
    rsx! {
        div { id: "{id}", class: "marquee {class}",
            span { "{text}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::visible_window;

    #[test]
    fn window_covers_the_screen_with_overscan() {
        // 10,000 rows of 64 px; list starts at the top of a 900 px screen.
        let (s, e) = visible_window(10_000, 0.0, 900.0, 64.0, 1.0, 20);
        assert_eq!(s, 0);
        assert!((15..=40).contains(&e), "{e}");
        // Scrolled down 64,000 px: row 1000 is at the top.
        let (s, e) = visible_window(10_000, -64_000.0, 900.0, 64.0, 1.0, 20);
        assert!(s <= 1000 - 15 && s % 20 == 0, "{s}");
        assert!(e >= 1000 + 15, "{e}");
        assert!(e - s < 80, "renders {} rows", e - s);
    }

    #[test]
    fn window_at_the_end_and_edge_cases() {
        let (s, e) = visible_window(100, -100.0 * 64.0, 900.0, 64.0, 1.0, 20);
        assert_eq!(e, 100);
        assert!(s <= e);
        assert_eq!(visible_window(0, 0.0, 900.0, 64.0, 1.0, 20), (0, 0));
        // List below the screen: starts at 0.
        assert_eq!(visible_window(50, 2000.0, 900.0, 64.0, 1.0, 20).0, 0);
    }

    #[test]
    fn small_scrolls_keep_the_same_start() {
        let a = visible_window(10_000, -64_000.0, 900.0, 64.0, 1.0, 20).0;
        let b = visible_window(10_000, -64_100.0, 900.0, 64.0, 1.0, 20).0;
        assert_eq!(a, b);
    }
}
