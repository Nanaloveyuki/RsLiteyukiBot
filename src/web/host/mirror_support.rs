use super::*;

pub(super) fn default_file_mirrors() -> Vec<String> {
    [
        "https://github.chenc.dev/",
        "https://ghproxy.cfd/",
        "https://ghproxy.cc/",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

pub(super) fn default_raw_mirrors() -> Vec<String> {
    [
        "https://raw.githubusercontent.com",
        "https://github.chenc.dev/https://raw.githubusercontent.com",
        "https://ghproxy.cfd/https://raw.githubusercontent.com",
    ]
    .into_iter()
    .map(ToString::to_string)
    .collect()
}

#[derive(Debug, Clone)]
pub(super) struct MirrorTestResult {
    pub(super) mirror: String,
    pub(super) latency: u64,
    pub(super) success: bool,
    pub(super) error: Option<String>,
}

pub(super) fn resolve_mirrored_url(original_url: &str, mirror: &str) -> String {
    let mirror = mirror.trim().trim_end_matches('/');
    if mirror.is_empty() {
        return original_url.to_string();
    }
    if original_url.starts_with("https://github.com/") {
        if mirror.eq_ignore_ascii_case("https://github.com") {
            return original_url.to_string();
        }
        let suffix = original_url.trim_start_matches("https://github.com/");
        if mirror.ends_with("github.com") {
            return format!("{mirror}/{suffix}");
        }
        return format!("{mirror}/{original_url}");
    }
    if original_url.starts_with("https://raw.githubusercontent.com/") {
        if mirror.eq_ignore_ascii_case("https://raw.githubusercontent.com") {
            return original_url.to_string();
        }
        let suffix = original_url.trim_start_matches("https://raw.githubusercontent.com/");
        if mirror.ends_with("raw.githubusercontent.com") {
            return format!("{mirror}/{suffix}");
        }
        return format!("{mirror}/{original_url}");
    }
    format!("{mirror}/{original_url}")
}

pub(super) async fn test_mirror_candidate(
    mirror_label: &str,
    mirror_url: Option<&str>,
    test_type: &str,
    timeout_ms: u64,
) -> MirrorTestResult {
    let (original_url, accept) = if test_type.eq_ignore_ascii_case("raw") {
        (
            "https://raw.githubusercontent.com/Nanaloveyuki/RsLiteyukiBot/main/README.md",
            Some("text/plain"),
        )
    } else {
        ("https://github.com/Nanaloveyuki/RsLiteyukiBot", None)
    };
    let request_url = mirror_url
        .map(|mirror| resolve_mirrored_url(original_url, mirror))
        .unwrap_or_else(|| original_url.to_string());
    let started_at = std::time::Instant::now();

    let response = super::upstream::upstream_http_client()
        .get(request_url.as_str())
        .timeout(Duration::from_millis(timeout_ms.max(250)))
        .header(
            reqwest::header::ACCEPT,
            accept.unwrap_or("text/html,application/xhtml+xml"),
        )
        .send()
        .await;

    match response {
        Ok(response) => {
            let status = response.status();
            let _ = response.bytes().await;
            if status.is_success() || status.is_redirection() {
                MirrorTestResult {
                    mirror: mirror_label.to_string(),
                    latency: started_at.elapsed().as_millis() as u64,
                    success: true,
                    error: None,
                }
            } else {
                MirrorTestResult {
                    mirror: mirror_label.to_string(),
                    latency: started_at.elapsed().as_millis() as u64,
                    success: false,
                    error: Some(format!("HTTP {}", status.as_u16())),
                }
            }
        }
        Err(err) => MirrorTestResult {
            mirror: mirror_label.to_string(),
            latency: started_at.elapsed().as_millis() as u64,
            success: false,
            error: Some(err.to_string()),
        },
    }
}
