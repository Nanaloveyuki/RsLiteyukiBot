use std::fs;

use serde_json::Value;

use super::{
    CUSTOM_FONT_FILE, WORKSPACE_FILE_DOWNLOAD_NAME, build_response, build_workspace_file_info,
    ensure_path_within_workspace, napcat_err, napcat_ok, napcat_response, parent_or_self,
    parse_json_body, parse_query_string, resolve_workspace_path, state_path,
};

pub(super) fn route_file_api(
    method: &str,
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Vec<u8> {
    let query = parse_query_string(raw_path);

    if method.eq_ignore_ascii_case("GET") {
        if api_path == "/File/list" {
            let target = query.get("path").map(String::as_str).unwrap_or("/");
            let only_directory = query
                .get("onlyDirectory")
                .map(|value| value.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let body = match resolve_workspace_path(target).and_then(|path| {
                ensure_path_within_workspace(path.as_path())?;
                let mut items = fs::read_dir(path)
                    .map_err(|err| format!("failed to read workspace directory: {err}"))?
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter_map(|entry| build_workspace_file_info(entry.as_path()).ok())
                    .filter(|entry| !only_directory || entry.is_directory)
                    .collect::<Vec<_>>();
                items.sort_by(|left, right| {
                    left.is_directory
                        .cmp(&right.is_directory)
                        .reverse()
                        .then_with(|| left.name.cmp(&right.name))
                });
                Ok(items)
            }) {
                Ok(items) => napcat_ok(&items),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/read" {
            let target = query.get("path").map(String::as_str).unwrap_or("/");
            let body = match resolve_workspace_path(target).and_then(|path| {
                ensure_path_within_workspace(path.as_path())?;
                fs::read_to_string(path).map_err(|err| format!("failed to read file: {err}"))
            }) {
                Ok(content) => napcat_ok(&content),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/font/exists/webui" {
            let body = napcat_ok(&state_path(CUSTOM_FONT_FILE).is_file());
            return napcat_response(body, is_head);
        }
        if api_path.starts_with("/File/download") {
            let target = query.get("path").map(String::as_str).unwrap_or("/");
            if let Ok(path) = resolve_workspace_path(target).and_then(|path| {
                ensure_path_within_workspace(path.as_path())?;
                Ok(path)
            }) && let Ok(bytes) = fs::read(path)
            {
                return build_response(
                    "200 OK",
                    "application/octet-stream",
                    bytes.as_slice(),
                    is_head,
                );
            }
            return build_response(
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"file not found",
                is_head,
            );
        }
    }

    if method.eq_ignore_ascii_case("POST") {
        let body = parse_json_body(request);
        if api_path.starts_with("/File/download") {
            let target = query.get("path").map(String::as_str).unwrap_or("/");
            if let Ok(path) = resolve_workspace_path(target).and_then(|path| {
                ensure_path_within_workspace(path.as_path())?;
                Ok(path)
            }) && let Ok(bytes) = fs::read(path)
            {
                return build_response(
                    "200 OK",
                    "application/octet-stream",
                    bytes.as_slice(),
                    is_head,
                );
            }
            return build_response(
                "404 Not Found",
                "text/plain; charset=utf-8",
                b"file not found",
                is_head,
            );
        }
        if api_path == "/File/mkdir" {
            let result = body
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "path is required".to_string())
                .and_then(resolve_workspace_path)
                .and_then(|path| {
                    if path.exists() {
                        return Ok(false);
                    }
                    ensure_path_within_workspace(path.as_path())?;
                    fs::create_dir_all(path)
                        .map_err(|err| format!("failed to create directory: {err}"))?;
                    Ok(true)
                });
            let body = match result {
                Ok(created) => napcat_ok(&created),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/delete" {
            let result = body
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "path is required".to_string())
                .and_then(resolve_workspace_path)
                .and_then(|path| {
                    ensure_path_within_workspace(path.as_path())?;
                    let metadata =
                        fs::metadata(&path).map_err(|err| format!("failed to stat path: {err}"))?;
                    if metadata.is_dir() {
                        fs::remove_dir_all(path)
                            .map_err(|err| format!("failed to remove directory: {err}"))
                    } else {
                        fs::remove_file(path).map_err(|err| format!("failed to remove file: {err}"))
                    }
                });
            let body = match result {
                Ok(()) => napcat_ok(&true),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/write" {
            let result = body
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "path is required".to_string())
                .and_then(|raw_path| {
                    let content = body
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let path = resolve_workspace_path(raw_path)?;
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|err| format!("failed to create parent directory: {err}"))?;
                    }
                    ensure_path_within_workspace(parent_or_self(path.as_path()).as_path())?;
                    fs::write(path, content).map_err(|err| format!("failed to write file: {err}"))
                });
            let body = match result {
                Ok(()) => napcat_ok(&true),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/create" {
            let result = body
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "path is required".to_string())
                .and_then(resolve_workspace_path)
                .and_then(|path| {
                    if path.exists() {
                        return Ok(false);
                    }
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|err| format!("failed to create parent directory: {err}"))?;
                    }
                    ensure_path_within_workspace(parent_or_self(path.as_path()).as_path())?;
                    fs::write(path, "").map_err(|err| format!("failed to create file: {err}"))?;
                    Ok(true)
                });
            let body = match result {
                Ok(created) => napcat_ok(&created),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/batchDelete" {
            let result = body
                .get("paths")
                .and_then(Value::as_array)
                .ok_or_else(|| "paths is required".to_string())
                .and_then(|paths| {
                    for raw in paths.iter().filter_map(Value::as_str) {
                        let path = resolve_workspace_path(raw)?;
                        ensure_path_within_workspace(path.as_path())?;
                        if let Ok(metadata) = fs::metadata(&path) {
                            if metadata.is_dir() {
                                fs::remove_dir_all(&path).map_err(|err| {
                                    format!("failed to remove directory {}: {err}", path.display())
                                })?;
                            } else {
                                fs::remove_file(&path).map_err(|err| {
                                    format!("failed to remove file {}: {err}", path.display())
                                })?;
                            }
                        }
                    }
                    Ok(())
                });
            let body = match result {
                Ok(()) => napcat_ok(&true),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/rename" || api_path == "/File/move" {
            let from_key = if api_path == "/File/rename" {
                "oldPath"
            } else {
                "sourcePath"
            };
            let to_key = if api_path == "/File/rename" {
                "newPath"
            } else {
                "targetPath"
            };
            let result = body
                .get(from_key)
                .and_then(Value::as_str)
                .zip(body.get(to_key).and_then(Value::as_str))
                .ok_or_else(|| "source and target path are required".to_string())
                .and_then(|(from, to)| {
                    let from_path = resolve_workspace_path(from)?;
                    let to_path = resolve_workspace_path(to)?;
                    ensure_path_within_workspace(from_path.as_path())?;
                    ensure_path_within_workspace(parent_or_self(to_path.as_path()).as_path())?;
                    if let Some(parent) = to_path.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|err| format!("failed to create parent directory: {err}"))?;
                    }
                    fs::rename(from_path, to_path)
                        .map_err(|err| format!("failed to move path: {err}"))
                });
            let body = match result {
                Ok(()) => napcat_ok(&true),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/batchMove" {
            let result = body
                .get("items")
                .and_then(Value::as_array)
                .ok_or_else(|| "items is required".to_string())
                .and_then(|items| {
                    for item in items {
                        let from = item
                            .get("sourcePath")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "sourcePath is required".to_string())?;
                        let to = item
                            .get("targetPath")
                            .and_then(Value::as_str)
                            .ok_or_else(|| "targetPath is required".to_string())?;
                        let from_path = resolve_workspace_path(from)?;
                        let to_path = resolve_workspace_path(to)?;
                        ensure_path_within_workspace(from_path.as_path())?;
                        ensure_path_within_workspace(parent_or_self(to_path.as_path()).as_path())?;
                        if let Some(parent) = to_path.parent() {
                            fs::create_dir_all(parent).map_err(|err| {
                                format!("failed to create parent directory: {err}")
                            })?;
                        }
                        fs::rename(from_path, to_path)
                            .map_err(|err| format!("failed to move path: {err}"))?;
                    }
                    Ok(())
                });
            let body = match result {
                Ok(()) => napcat_ok(&true),
                Err(err) => napcat_err(-1, err.as_str()),
            };
            return napcat_response(body, is_head);
        }
        if api_path == "/File/font/delete/webui" {
            let _ = fs::remove_file(state_path(CUSTOM_FONT_FILE));
            let body = napcat_ok(&true);
            return napcat_response(body, is_head);
        }
        if api_path.starts_with("/File/upload") || api_path == "/File/font/upload/webui" {
            let body = napcat_err(-1, "multipart upload is not implemented yet");
            return napcat_response(body, is_head);
        }
        if api_path == "/File/batchDownload" {
            return build_response(
                "200 OK",
                "application/octet-stream",
                WORKSPACE_FILE_DOWNLOAD_NAME.as_bytes(),
                is_head,
            );
        }
    }

    let body = napcat_err(-1, "not found");
    napcat_response(body, is_head)
}
