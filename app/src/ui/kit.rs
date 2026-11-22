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

/// Where a virtual list or grid is: its top edge relative to the viewport, the viewport height,
/// and the list's width (px).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ListView {
    pub top: f64,
    pub height: f64,
    pub width: f64,
}

/// Watches element `id`: reports its position on scroll and resize, runs the fast-scroll handle
/// (`{id}-fs`) in the page so dragging stays smooth, and, with `zoom`, turns pinch and
/// Ctrl+wheel into zoom steps (factor > 1 is bigger) passed to `on_zoom`.
fn use_list_view(id: String, zoom: Option<Callback<f64>>) -> Signal<ListView> {
    let mut view = use_signal(|| ListView {
        top: 0.0,
        height: 900.0,
        width: 380.0,
    });
    use_future(move || {
        let id = id.clone();
        async move {
            let js = format!(
                r#"(function() {{
                    const id = {id};
                    let queued = false;
                    const el = () => document.getElementById(id);
                    const report = () => {{
                        queued = false;
                        const e = el();
                        if (e) {{ const r = e.getBoundingClientRect(); dioxus.send(['view', r.top, window.innerHeight, r.width]); }}
                    }};
                    const ask = () => {{ if (!queued) {{ queued = true; requestAnimationFrame(report); }} }};
                    window.addEventListener('scroll', ask, {{ passive: true }});
                    window.addEventListener('resize', ask);
                    setTimeout(report, 50);
                    // Fast scroller: drag the handle to jump through the list.
                    document.addEventListener('pointerdown', (ev) => {{
                        const track = ev.target.closest && ev.target.closest('#' + CSS.escape(id + '-fs'));
                        const e = el();
                        if (!track || !e) return;
                        ev.preventDefault();
                        track.classList.add('dragging');
                        const go = (y) => {{
                            const t = track.getBoundingClientRect();
                            const f = Math.min(1, Math.max(0, (y - t.top) / t.height));
                            const top = e.getBoundingClientRect().top + window.scrollY;
                            const span = Math.max(0, e.offsetHeight - window.innerHeight * 0.6);
                            window.scrollTo(0, top - 120 + f * span);
                        }};
                        go(ev.clientY);
                        const move = (m) => go(m.clientY);
                        const up = () => {{
                            track.classList.remove('dragging');
                            window.removeEventListener('pointermove', move);
                            window.removeEventListener('pointerup', up);
                            window.removeEventListener('pointercancel', up);
                        }};
                        window.addEventListener('pointermove', move);
                        window.addEventListener('pointerup', up);
                        window.addEventListener('pointercancel', up);
                    }});
                    if ({zoom}) {{
                        // Pinch (two fingers) or Ctrl+wheel resizes the grid.
                        window.addEventListener('wheel', (ev) => {{
                            const e = el();
                            if (!ev.ctrlKey || !e || !e.contains(ev.target)) return;
                            ev.preventDefault();
                            dioxus.send(['zoom', ev.deltaY < 0 ? 1.1 : 0.9, 0, 0]);
                        }}, {{ passive: false }});
                        let start = 0;
                        const dist = (t) => Math.hypot(t[0].clientX - t[1].clientX, t[0].clientY - t[1].clientY);
                        window.addEventListener('touchstart', (ev) => {{
                            if (ev.touches.length === 2 && el() && el().contains(ev.target)) start = dist(ev.touches);
                        }}, {{ passive: true }});
                        window.addEventListener('touchmove', (ev) => {{
                            if (ev.touches.length !== 2 || !start) return;
                            ev.preventDefault();
                            const f = dist(ev.touches) / start;
                            if (f > 1.15 || f < 0.87) {{ dioxus.send(['zoom', f, 0, 0]); start = dist(ev.touches); }}
                        }}, {{ passive: false }});
                        window.addEventListener('touchend', () => {{ start = 0; }}, {{ passive: true }});
                    }}
                }})();"#,
                id = serde_json::to_string(&id).unwrap_or_default(),
                zoom = zoom.is_some(),
            );
            let mut ev = document::eval(&js);
            while let Ok((kind, a, b, c)) = ev.recv::<(String, f64, f64, f64)>().await {
                match kind.as_str() {
                    "view" => {
                        let v = ListView {
                            top: a,
                            height: b,
                            width: c,
                        };
                        if *view.peek() != v {
                            view.set(v);
                        }
                    }
                    "zoom" => {
                        if let Some(z) = zoom {
                            z.call(a);
                        }
                    }
                    _ => {}
                }
            }
        }
    });
    view
}

/// The draggable handle at the screen's edge for jumping through long lists. Shown when the
/// list is on screen and more than three screens long.
#[component]
fn FastScroller(id: String, view: ListView, total: f64) -> Element {
    let on_screen = view.top < view.height && view.top + total > 0.0;
    if total < view.height * 3.0 || !on_screen {
        return rsx! {};
    }
    let span = (total - view.height * 0.6).max(1.0);
    let frac = ((120.0 - view.top) / span).clamp(0.0, 1.0);
    rsx! {
        div { id: "{id}-fs", class: "fast-scroll", "aria-hidden": "true",
            div { class: "fast-scroll-thumb", style: "top: calc({frac} * (100% - 44px))" }
        }
    }
}

/// A list that renders only the rows near the screen. Rows must all be `row_height` px tall.
/// `render(i)` draws row `i`. Long lists get a fast-scroll handle.
#[component]
pub fn VirtualList(
    id: String,
    len: usize,
    row_height: f64,
    render: Callback<usize, Element>,
) -> Element {
    let view = use_list_view(id.clone(), None)();
    let (start, end) = visible_window(len, view.top, view.height, row_height, 1.0, 20);
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
        FastScroller { id: id.clone(), view, total: len as f64 * row_height }
    }
}

/// Grid layout for a width: (columns, item width, row height). Items are squares plus
/// `extra` px of text below; rows are `gap` apart.
pub fn grid_layout(width: f64, min_item: f64, extra: f64, gap: f64) -> (usize, f64, f64) {
    let cols = (((width + gap) / (min_item + gap)).floor() as usize).max(2);
    let item = ((width - gap * (cols as f64 - 1.0)) / cols as f64).max(1.0);
    (cols, item, item + extra + gap)
}

/// A grid that renders only the rows near the screen. Items are squares (cover art) with
/// `extra` px of text below, at least `min_item` px wide. Pinch or Ctrl+wheel calls `on_zoom`
/// with a factor (> 1 means bigger items).
#[component]
pub fn VirtualGrid(
    id: String,
    len: usize,
    min_item: f64,
    extra: f64,
    render: Callback<usize, Element>,
    #[props(default)] on_zoom: Option<Callback<f64>>,
) -> Element {
    let view = use_list_view(id.clone(), on_zoom)();
    let gap = 14.0;
    let (cols, _item, row_h) = grid_layout(view.width.max(200.0), min_item, extra, gap);
    let rows = len.div_ceil(cols);
    let (start, end) = visible_window(rows, view.top, view.height, row_h, 1.0, 4);
    let before = start as f64 * row_h;
    let after = (rows - end) as f64 * row_h;
    rsx! {
        div { id: "{id}", class: "vgrid",
            div { style: "height: {before}px" }
            for r in start..end {
                div {
                    key: "{r}",
                    class: "vgrid-row",
                    style: "height: {row_h}px; grid-template-columns: repeat({cols}, minmax(0, 1fr)); column-gap: {gap}px",
                    for i in (r * cols)..((r + 1) * cols).min(len) {
                        div { key: "{i}", class: "vgrid-cell", {render.call(i)} }
                    }
                }
            }
            div { style: "height: {after}px" }
        }
        FastScroller { id: id.clone(), view, total: rows as f64 * row_h }
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
    fn grid_layout_fits_columns() {
        use super::grid_layout;
        // A phone (380 px wide) with 150 px minimum: 2 columns.
        let (c, w, h) = grid_layout(380.0, 150.0, 54.0, 14.0);
        assert_eq!(c, 2);
        assert!((w * 2.0 + 14.0 - 380.0).abs() < 0.01);
        assert!((h - (w + 54.0 + 14.0)).abs() < 0.01);
        // Zoomed out (100 px): 3 columns. Never fewer than 2.
        assert_eq!(grid_layout(380.0, 100.0, 54.0, 14.0).0, 3);
        assert_eq!(grid_layout(200.0, 400.0, 54.0, 14.0).0, 2);
    }

    #[test]
    fn small_scrolls_keep_the_same_start() {
        let a = visible_window(10_000, -64_000.0, 900.0, 64.0, 1.0, 20).0;
        let b = visible_window(10_000, -64_100.0, 900.0, 64.0, 1.0, 20).0;
        assert_eq!(a, b);
    }
}
