//! Icons: Material Symbols Rounded (Apache-2.0), as an embedded font subset to the icons Loss
//! uses (`app/assets/fonts/material-symbols-loss.woff2`, made by `scripts/make-icon-font.py`).
//! Each icon is the ligature of its Material name; adding one means adding it here and re-running
//! the script.

use dioxus::prelude::*;

/// Loss' icon names → Material Symbols names.
pub fn symbol(name: &str) -> &'static str {
    match name {
        "play" => "play_arrow",
        "pause" => "pause",
        "next" => "skip_next",
        "prev" => "skip_previous",
        "shuffle" => "shuffle",
        "repeat" => "repeat",
        "repeat_one" => "repeat_one",
        "library" => "library_music",
        "search" => "search",
        "download" => "download",
        "settings" => "settings",
        "close" => "close",
        "back" => "arrow_back",
        "chevron_down" => "keyboard_arrow_down",
        "queue" => "queue_music",
        "lyrics" => "lyrics",
        "add" => "add",
        "play_next" => "queue_play_next",
        "refresh" => "refresh",
        "note" => "music_note",
        "volume" => "volume_up",
        "delete" => "delete",
        "check" => "check",
        "link" => "link",
        "chevron_right" => "chevron_right",
        "retry" => "replay",
        "home" => "home",
        "explore" => "explore",
        "stats" => "bar_chart",
        "server" => "dns",
        "album" => "album",
        "person" => "person",
        "sort" => "sort",
        "more" => "more_vert",
        "eq" => "equalizer",
        "volume_down" => "volume_down",
        "volume_up" => "volume_up",
        "grid" => "grid_view",
        "list" => "list",
        "sparkle" => "auto_awesome",
        "folder" => "folder",
        "mic" => "mic",
        "history" => "history",
        "palette" => "palette",
        "info" => "info",
        _ => "info",
    }
}

/// Icons that point a direction and must mirror in right-to-left languages.
fn mirrors(name: &str) -> bool {
    matches!(name, "back" | "chevron_right")
}

#[component]
pub fn Icon(
    name: String,
    #[props(default = 24)] size: u32,
    #[props(default)] filled: bool,
) -> Element {
    let class = match (filled, mirrors(&name)) {
        (true, true) => "icon msr filled mirror",
        (true, false) => "icon msr filled",
        (false, true) => "icon msr mirror",
        (false, false) => "icon msr",
    };
    rsx! {
        span {
            class: "{class}",
            style: "font-size: {size}px; width: {size}px; height: {size}px",
            "aria-hidden": "true",
            {symbol(&name)}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_icon_has_a_symbol() {
        for n in ["play", "back", "settings", "eq"] {
            assert_ne!(super::symbol(n), "info", "{n}");
        }
        // Unknown names show something rather than nothing.
        assert_eq!(super::symbol("nope"), "info");
    }
}
