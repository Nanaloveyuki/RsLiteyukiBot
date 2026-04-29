use crate::app_host::EmbeddedAppHost;

use super::{
    WebHostAsset, build_redirect_response, build_response, plugin_declared_page_paths,
    resolve_plugin_descriptor,
};

pub(super) fn route_plugin_page(
    runtime_host: Option<&EmbeddedAppHost>,
    path: &str,
    is_head: bool,
) -> Vec<u8> {
    let Some(rest) = path.strip_prefix("/plugin/") else {
        return build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            is_head,
        );
    };
    let Some((plugin_id, page_path)) = rest.split_once("/page/") else {
        return build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            is_head,
        );
    };
    let plugin_id = plugin_id.trim();
    if plugin_id.is_empty() {
        return build_response(
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"missing plugin id",
            is_head,
        );
    }

    let Some(descriptor) = resolve_plugin_descriptor(runtime_host, plugin_id) else {
        return build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin not found",
            is_head,
        );
    };
    let Some(manifest_dir) = descriptor
        .manifest_path
        .as_ref()
        .and_then(|path| path.parent())
    else {
        return build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin page not found",
            is_head,
        );
    };

    let wants_index = page_path.ends_with('/')
        || !page_path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .contains('.');
    if wants_index && !page_path.ends_with('/') {
        let location = format!("{path}/");
        return build_redirect_response("307 Temporary Redirect", location.as_str(), is_head);
    }

    let normalized_request = page_path.trim_matches('/');
    if normalized_request.is_empty() {
        return build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin page not found",
            is_head,
        );
    }

    let Some(request_path) = super::assets::sanitize_request_path(normalized_request) else {
        return build_response(
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"bad plugin path",
            is_head,
        );
    };
    let request_key = request_path
        .iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    let declared = plugin_declared_page_paths(&descriptor)
        .into_iter()
        .any(|declared| {
            request_key == declared || request_key.starts_with(format!("{declared}/").as_str())
        });
    if !declared {
        return build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin page not declared",
            is_head,
        );
    }

    let webui_root = manifest_dir.join("webui");
    let asset_path = if wants_index {
        webui_root.join(&request_path).join("index.html")
    } else {
        webui_root.join(&request_path)
    };
    if !asset_path.starts_with(&webui_root) {
        return build_response(
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"bad plugin path",
            is_head,
        );
    }
    match super::assets::read_asset_file(asset_path).map(maybe_inject_plugin_page_auth_bridge) {
        Some(asset) => build_response("200 OK", asset.content_type(), asset.body(), is_head),
        None => build_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"plugin page not found",
            is_head,
        ),
    }
}

fn maybe_inject_plugin_page_auth_bridge(asset: WebHostAsset) -> WebHostAsset {
    if !asset
        .content_type()
        .to_ascii_lowercase()
        .starts_with("text/html")
    {
        return asset;
    }

    let body = String::from_utf8_lossy(asset.body()).into_owned();
    let injected = inject_plugin_page_auth_bridge(body.as_str());
    WebHostAsset::text(asset.content_type().to_string(), injected)
}

fn inject_plugin_page_auth_bridge(document: &str) -> String {
    const SCRIPT: &str = r#"<script>
(function () {
  function readLiteyukiToken() {
    try {
      const raw = window.localStorage.getItem("token");
      if (!raw) return "";
      try {
        const parsed = JSON.parse(raw);
        return typeof parsed === "string" ? parsed : "";
      } catch (_) {
        return raw;
      }
    } catch (_) {
      return "";
    }
  }

  const originalFetch = window.fetch.bind(window);
  window.fetch = function (input, init) {
    const token = readLiteyukiToken();
    if (!token) {
      return originalFetch(input, init);
    }

    const request = new Request(input, init);
    const headers = new Headers(request.headers);
    if (!headers.has("Authorization")) {
      headers.set("Authorization", "Bearer " + token);
    }

    return originalFetch(new Request(request, { headers }));
  };
})();
</script>"#;

    if let Some(index) = document.rfind("</head>") {
        let mut output = String::with_capacity(document.len() + SCRIPT.len());
        output.push_str(&document[..index]);
        output.push_str(SCRIPT);
        output.push_str(&document[index..]);
        return output;
    }

    if let Some(index) = document.rfind("</body>") {
        let mut output = String::with_capacity(document.len() + SCRIPT.len());
        output.push_str(&document[..index]);
        output.push_str(SCRIPT);
        output.push_str(&document[index..]);
        return output;
    }

    let mut output = String::with_capacity(document.len() + SCRIPT.len());
    output.push_str(SCRIPT);
    output.push_str(document);
    output
}
