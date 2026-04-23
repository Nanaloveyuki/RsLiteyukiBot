use super::*;

pub(super) fn route_mirror_api(
    api_path: &str,
    raw_path: &str,
    request: &[u8],
    is_head: bool,
) -> Option<Vec<u8>> {
    if api_path == "/Mirror/List" {
        let body = napcat_ok(&load_mirror_config());
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Mirror/SetCustom" {
        let payload = parse_json_body(request);
        let mut config = load_mirror_config();
        config.custom_mirror = payload
            .get("mirror")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let body = match save_mirror_config(&config) {
            Ok(()) => napcat_ok(&serde_json::Value::Null),
            Err(err) => napcat_err(-1, err.as_str()),
        };
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Mirror/Test" {
        let payload = parse_json_body(request);
        let test_type = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("file");
        let mirror = payload
            .get("mirror")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        let config = load_mirror_config();
        let result = run_async_for_web_host(test_mirror_candidate(
            if mirror.is_empty() {
                "自动选择"
            } else {
                mirror
            },
            (!mirror.is_empty()).then_some(mirror),
            test_type,
            config.timeout,
        ));
        let body = napcat_ok(&serde_json::json!({
            "mirror": result.mirror,
            "latency": result.latency,
            "success": result.success,
            "error": result.error
        }));
        return Some(napcat_response(body, is_head));
    }

    if api_path == "/Mirror/Test/SSE" {
        let query = parse_query_string(raw_path);
        let test_type = query.get("type").map(String::as_str).unwrap_or("file");
        let config = load_mirror_config();
        let mirrors = if test_type.eq_ignore_ascii_case("raw") {
            config.raw_mirrors.clone()
        } else {
            config.file_mirrors.clone()
        };

        let total = mirrors.len() + 1;
        let mut events = vec![
            serde_json::json!({
                "type": "start",
                "total": total,
                "message": "开始测速镜像源"
            })
            .to_string(),
        ];
        let mut results = Vec::new();

        let original_label = if test_type.eq_ignore_ascii_case("raw") {
            "https://raw.githubusercontent.com"
        } else {
            "https://github.com"
        };
        events.push(
            serde_json::json!({
                "type": "testing",
                "index": 0,
                "total": total,
                "mirror": original_label,
                "message": format!("测试 {}", original_label)
            })
            .to_string(),
        );
        let original_result = run_async_for_web_host(test_mirror_candidate(
            original_label,
            None,
            test_type,
            config.timeout,
        ));
        events.push(
            serde_json::json!({
                "type": "result",
                "index": 0,
                "total": total,
                "result": {
                    "mirror": original_result.mirror,
                    "latency": original_result.latency,
                    "success": original_result.success,
                    "error": original_result.error
                }
            })
            .to_string(),
        );
        results.push(original_result);

        for (index, mirror) in mirrors.iter().enumerate() {
            events.push(
                serde_json::json!({
                    "type": "testing",
                    "index": index + 1,
                    "total": total,
                    "mirror": mirror,
                    "message": format!("测试 {}", mirror)
                })
                .to_string(),
            );
            let result = run_async_for_web_host(test_mirror_candidate(
                mirror,
                Some(mirror.as_str()),
                test_type,
                config.timeout,
            ));
            events.push(
                serde_json::json!({
                    "type": "result",
                    "index": index + 1,
                    "total": total,
                    "result": {
                        "mirror": result.mirror,
                        "latency": result.latency,
                        "success": result.success,
                        "error": result.error
                    }
                })
                .to_string(),
            );
            results.push(result);
        }

        let successful_results = results
            .iter()
            .filter(|result| result.success)
            .cloned()
            .collect::<Vec<_>>();
        let failed_results = results
            .iter()
            .filter(|result| !result.success)
            .cloned()
            .collect::<Vec<_>>();
        let fastest = successful_results
            .iter()
            .min_by_key(|result| result.latency);
        events.push(
            serde_json::json!({
                "type": "complete",
                "results": successful_results.iter().map(|result| serde_json::json!({
                    "mirror": result.mirror,
                    "latency": result.latency,
                    "success": result.success,
                    "error": result.error
                })).collect::<Vec<_>>(),
                "failed": failed_results.iter().map(|result| serde_json::json!({
                    "mirror": result.mirror,
                    "latency": result.latency,
                    "success": result.success,
                    "error": result.error
                })).collect::<Vec<_>>(),
                "fastest": fastest.map(|result| serde_json::json!({
                    "mirror": result.mirror,
                    "latency": result.latency,
                    "success": result.success,
                    "error": result.error
                })),
                "message": "测速完成"
            })
            .to_string(),
        );
        return Some(sse_batch_response(&events, is_head));
    }

    None
}
