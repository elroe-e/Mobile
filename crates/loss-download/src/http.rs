//! Shared HTTP client.

use std::time::Duration;

pub const USER_AGENT: &str = concat!("Loss/", env!("CARGO_PKG_VERSION"));

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("http client")
}

/// GET a URL and parse JSON, with a clear error for non-2xx answers.
pub async fn get_json(client: &reqwest::Client, url: &str) -> crate::Result<serde_json::Value> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("server answered {status}").into());
    }
    Ok(resp.json().await?)
}

pub async fn get_bytes(client: &reqwest::Client, url: &str) -> crate::Result<Vec<u8>> {
    let resp = client
        .get(url)
        .timeout(Duration::from_secs(60))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("server answered {status}").into());
    }
    Ok(resp.bytes().await?.to_vec())
}

/// Read a JSON value as a string even if the API returns a number or an
/// array of strings (the Internet Archive does both).
pub fn as_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Array(a) => a.first().and_then(as_string),
        _ => None,
    }
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

pub fn as_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => {
            // "3:25" or "205.3"
            if let Some((m, sec)) = s.split_once(':') {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() == 3 {
                    let h: f64 = parts[0].parse().ok()?;
                    let m: f64 = parts[1].parse().ok()?;
                    let sec: f64 = parts[2].parse().ok()?;
                    return Some(h * 3600.0 + m * 60.0 + sec);
                }
                Some(m.parse::<f64>().ok()? * 60.0 + sec.parse::<f64>().ok()?)
            } else {
                s.parse().ok()
            }
        }
        _ => None,
    }
}

pub fn as_u32(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().map(|n| n as u32),
        serde_json::Value::String(s) => s
            .split(['/', '-', ' '])
            .next()
            .and_then(|x| x.trim().parse().ok()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_helpers() {
        assert_eq!(as_string(&json!(["a", "b"])), Some("a".into()));
        assert_eq!(as_f64(&json!("3:25")), Some(205.0));
        assert_eq!(as_f64(&json!("1:00:05")), Some(3605.0));
        assert_eq!(as_u32(&json!("4/12")), Some(4));
        assert_eq!(as_u32(&json!("1998-01-01")), Some(1998));
    }
}
