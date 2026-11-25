//! Downloads an AutoEq headphone profile (M2-11; parsing is `loss_core::autoeq`).

/// The profile text at `url` (AutoEq's raw file on GitHub), at most 64 KB.
pub async fn fetch(client: &reqwest::Client, url: &str) -> crate::Result<String> {
    let bytes = crate::http::get_bytes(client, url).await?;
    if bytes.len() > 64 * 1024 {
        return Err("profile too large".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
