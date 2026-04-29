use super::*;

pub(super) fn upstream_http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("RsLiteyukiBot-WebHost/0.1")
            .build()
            .expect("upstream web host http client should build")
    })
}

pub(super) fn sanitize_repo_component(raw: Option<&String>, label: &str) -> Result<String, String> {
    let value = raw
        .map(String::as_str)
        .unwrap_or_default()
        .trim()
        .trim_matches('/');
    if value.is_empty() {
        return Err(format!("missing github {label}"));
    }
    if value.contains('/')
        || value.contains('\\')
        || value.contains('?')
        || value.contains('#')
        || value.contains(':')
    {
        return Err(format!("invalid github {label}"));
    }
    Ok(value.trim_end_matches(".git").to_string())
}

fn summarize_upstream_error(status: reqwest::StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| body.trim().chars().take(160).collect::<String>());
    format!("upstream returned {}: {}", status.as_u16(), message)
}

pub(super) async fn fetch_upstream_json(
    url: &str,
    accept: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut request = upstream_http_client().get(url);
    if let Some(accept) = accept {
        request = request.header(reqwest::header::ACCEPT, accept);
    }
    let response = request
        .send()
        .await
        .map_err(|err| format!("failed to fetch upstream json: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed to read upstream response: {err}"))?;
    if !status.is_success() {
        return Err(summarize_upstream_error(status, body.as_str()));
    }
    serde_json::from_str(body.as_str()).map_err(|err| format!("invalid upstream json: {err}"))
}

pub(super) async fn fetch_upstream_text(url: &str, accept: Option<&str>) -> Result<String, String> {
    let mut request = upstream_http_client().get(url);
    if let Some(accept) = accept {
        request = request.header(reqwest::header::ACCEPT, accept);
    }
    let response = request
        .send()
        .await
        .map_err(|err| format!("failed to fetch upstream text: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed to read upstream response: {err}"))?;
    if !status.is_success() {
        return Err(summarize_upstream_error(status, body.as_str()));
    }
    Ok(body)
}
