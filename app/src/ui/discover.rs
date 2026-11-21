//! Discover: search free/legal sources and your own server, then download.

use super::icons::Icon;
use super::{state, RemoteCover, Tab};
use crate::services;
use dioxus::prelude::*;
use loss_core::format_duration;
use loss_download::sources::{archive, direct, jamendo, podcast, subsonic, ItemAction};
use loss_download::{Collection, DownloadRequest, SearchItem, SourceKind};

/// The opened collection (album, podcast…), kept by the App so Android's Back
/// button can close it.
#[derive(Clone, Copy)]
pub struct DiscoverNav(pub Signal<Option<(Collection, Vec<SearchItem>)>>);

pub fn use_provide_nav() -> DiscoverNav {
    use_context_provider(|| DiscoverNav(Signal::new(None)))
}

const SOURCES: [SourceKind; 5] = [
    SourceKind::InternetArchive,
    SourceKind::Jamendo,
    SourceKind::Podcast,
    SourceKind::Subsonic,
    SourceKind::Direct,
];

fn hint(source: SourceKind) -> &'static str {
    match source {
        SourceKind::InternetArchive => "Free concerts, netlabels and public-domain recordings. Each result shows its licence.",
        SourceKind::Jamendo => "Independent artists sharing music under Creative Commons. Needs a free client id (Settings).",
        SourceKind::Podcast => "Search podcasts and download episodes from the show's own feed.",
        SourceKind::Subsonic => "Download from your own Navidrome / Subsonic server for offline listening.",
        SourceKind::Direct => "Paste download links, one per line: Bandcamp or Qobuz purchase links, files on your NAS, etc. Zip albums are unpacked automatically.",
    }
}

/// Runs a search future on the download runtime and returns its result.
async fn run<T: Send + 'static>(
    fut: impl std::future::Future<Output = loss_download::Result<T>> + Send + 'static,
) -> Result<T, String> {
    match services::get().downloads.runtime().spawn(fut).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

async fn search(source: SourceKind, query: String) -> Result<Vec<SearchItem>, String> {
    let svc = services::get();
    let client = svc.downloads.client();
    let settings = svc.settings();
    match source {
        SourceKind::InternetArchive => {
            let cc = settings.archive_cc_only;
            run(async move { archive::search(&client, &query, cc, 1).await }).await
        }
        SourceKind::Jamendo => {
            let id = settings.jamendo_client_id.clone();
            run(async move { jamendo::search(&client, &id, &query).await }).await
        }
        SourceKind::Podcast => {
            if query.starts_with("http://") || query.starts_with("https://") {
                run(async move { podcast::open(&client, &query).await }).await
            } else {
                run(async move { podcast::search(&client, &query).await }).await
            }
        }
        SourceKind::Subsonic => {
            let Some(cfg) = settings.subsonic.clone() else {
                return Err("Connect your server in Settings first.".into());
            };
            if query.trim().is_empty() {
                run(async move { subsonic::recent_albums(&client, &cfg).await }).await
            } else {
                run(async move { subsonic::search(&client, &cfg, &query).await }).await
            }
        }
        SourceKind::Direct => Ok(vec![]),
    }
}

async fn open(c: Collection) -> Result<Vec<SearchItem>, String> {
    let svc = services::get();
    let client = svc.downloads.client();
    match c.source {
        SourceKind::InternetArchive => {
            run(async move { archive::open(&client, &c.id).await }).await
        }
        SourceKind::Podcast => run(async move { podcast::open(&client, &c.id).await }).await,
        SourceKind::Subsonic => {
            let cfg = svc
                .settings()
                .subsonic
                .clone()
                .ok_or("Server not configured")?;
            run(async move { subsonic::open_album(&client, &cfg, &c.id).await }).await
        }
        _ => Ok(vec![]),
    }
}

#[component]
pub fn DiscoverTab() -> Element {
    let st = state();
    let mut source = use_signal(|| SourceKind::InternetArchive);
    let mut query = use_signal(String::new);
    let mut results: Signal<Vec<SearchItem>> = use_signal(Vec::new);
    let mut loading = use_signal(|| false);
    let mut error: Signal<Option<String>> = use_signal(|| None);
    let mut opened = use_context::<DiscoverNav>().0;
    let mut links = use_signal(String::new);

    let mut do_search = move || {
        let (src, q) = (source(), query());
        if q.trim().is_empty() && src != SourceKind::Subsonic {
            return;
        }
        loading.set(true);
        error.set(None);
        opened.set(None);
        spawn(async move {
            let r = search(src, q).await;
            loading.set(false);
            match r {
                Ok(items) => {
                    if items.is_empty() {
                        error.set(Some("Nothing found. Try other words.".into()));
                    }
                    results.set(items);
                }
                Err(e) => {
                    results.set(vec![]);
                    error.set(Some(e));
                }
            }
        });
    };

    let do_open = move |c: Collection| {
        loading.set(true);
        error.set(None);
        spawn(async move {
            let r = open(c.clone()).await;
            loading.set(false);
            match r {
                Ok(items) => {
                    if items.is_empty() {
                        error.set(Some("No downloadable audio in this item.".into()));
                    }
                    opened.set(Some((c, items)));
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };

    let queued_titles: Vec<String> = st.jobs.read().iter().map(|j| j.title.clone()).collect();

    let src = source();
    let opened_now = opened();

    rsx! {
        header { class: "page-header", h1 { "Discover" } }
        div { class: "chips",
            for s in SOURCES {
                button {
                    class: if src == s { "chip active" } else { "chip" },
                    onclick: move |_| {
                        source.set(s);
                        results.set(vec![]);
                        opened.set(None);
                        error.set(None);
                        if s == SourceKind::Subsonic && services::get().settings().subsonic.is_some() {
                            query.set(String::new());
                            do_search();
                        }
                    },
                    "{s.label()}"
                }
            }
        }
        p { class: "muted small pad", "{hint(src)}" }

        if src == SourceKind::Direct {
            div { class: "card",
                textarea {
                    class: "links",
                    id: "links-input",
                    rows: "5",
                    placeholder: "https://…/album.zip\nhttps://…/song.flac",
                    initial_value: "{links}",
                    oninput: move |e| links.set(e.value()),
                }
                button {
                    class: "primary",
                    onclick: move |_| match direct::requests_from_text(&links()) {
                        Ok(reqs) => {
                            let n = services::get().downloads.enqueue(reqs).len();
                            links.set(String::new());
                            document::eval("document.getElementById('links-input').value = ''");
                            st.toast(format!("{n} download(s) added"));
                        }
                        Err(e) => error.set(Some(e)),
                    },
                    Icon { name: "download", size: 20 }
                    " Download"
                }
            }
        } else {
            form {
                class: "search-box",
                onsubmit: move |e| {
                    e.prevent_default();
                    do_search();
                },
                Icon { name: "search", size: 20 }
                input {
                    r#type: "search",
                    placeholder: match src {
                        SourceKind::Podcast => "Podcast name or feed URL",
                        SourceKind::Subsonic => "Search your server (empty = newest albums)",
                        _ => "Artist, album, genre…",
                    },
                    initial_value: "{query}",
                    oninput: move |e| query.set(e.value()),
                }
                button { class: "pill", r#type: "submit", "Search" }
            }
        }

        if loading() {
            div { class: "info-bar", span { class: "spinner" } "Loading…" }
        }
        if let Some(e) = error() {
            div { class: "error pad", "{e}" }
            if e.contains("Settings") {
                button { class: "pill center", onclick: move |_| { let mut t = st.tab; t.set(Tab::Settings) }, "Open Settings" }
            }
        }

        if let Some((col, items)) = opened_now {
            button { class: "back", onclick: move |_| opened.set(None), Icon { name: "back" } " Results" }
            div { class: "collection-head",
                h2 { "{col.title}" }
                {
                    let mut reqs: Vec<DownloadRequest> = items.iter().filter_map(|i| i.request().cloned()).collect();
                    // Feeds can have hundreds of episodes: offer the newest ones.
                    let capped = col.source == SourceKind::Podcast && reqs.len() > 25;
                    reqs.truncate(if capped { 25 } else { reqs.len() });
                    let n = reqs.len();
                    let label = if capped { format!(" Download newest {n}") } else { format!(" Download all ({n})") };
                    rsx! {
                        if n > 0 {
                            button {
                                class: "primary",
                                onclick: move |_| {
                                    let added = services::get().downloads.enqueue(reqs.clone()).len();
                                    st.toast(format!("{added} downloads added"));
                                },
                                Icon { name: "download", size: 20 }
                                "{label}"
                            }
                        }
                    }
                }
            }
            div { class: "list",
                for (i , item) in items.iter().enumerate() {
                    ResultRow { key: "{i}", item: item.clone(), queued: queued_titles.contains(&item.title), on_open: do_open }
                }
            }
        } else {
            div { class: "list",
                for (i , item) in results().iter().enumerate() {
                    ResultRow { key: "{i}", item: item.clone(), queued: queued_titles.contains(&item.title), on_open: do_open }
                }
            }
        }
        if src == SourceKind::InternetArchive || src == SourceKind::Jamendo {
            p { class: "muted small pad legal",
                "Check each item's licence before sharing or reusing it. "
                "Loss only lists sources that let you download."
            }
        }
    }
}

#[component]
fn ResultRow(item: SearchItem, queued: bool, on_open: EventHandler<Collection>) -> Element {
    let st = state();
    let mut added = use_signal(|| false);
    let meta = [
        item.format.clone(),
        item.duration.filter(|d| *d > 0.0).map(format_duration),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let action = item.action.clone();
    rsx! {
        div { class: "row result",
            RemoteCover { url: item.cover_url.clone(), class: "cover row-cover" }
            div { class: "row-text",
                div { class: "title", "{item.title}" }
                div { class: "subtitle", "{item.subtitle}" }
                div { class: "tags",
                    if let Some(l) = &item.license {
                        span { class: "tag license", "{l}" }
                    }
                    if !meta.is_empty() {
                        span { class: "tag", "{meta}" }
                    }
                }
            }
            match action {
                ItemAction::Open(c) => rsx! {
                    button { class: "icon-btn", onclick: move |_| on_open.call(c.clone()), Icon { name: "chevron_right", size: 28 } }
                },
                ItemAction::Download(req) => rsx! {
                    button {
                        class: if added() || queued { "icon-btn on" } else { "icon-btn" },
                        onclick: move |_| {
                            services::get().downloads.enqueue(vec![(*req).clone()]);
                            added.set(true);
                            st.toast("Added to downloads");
                        },
                        Icon { name: if added() || queued { "check" } else { "download" } }
                    }
                },
            }
        }
    }
}
