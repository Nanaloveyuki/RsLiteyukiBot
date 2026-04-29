use serde_json::Value;

pub(super) fn namespace_tool_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp__{}__{}",
        sanitize_identifier(server_name),
        sanitize_identifier(tool_name)
    )
}

pub(super) fn sanitize_identifier(raw: &str) -> String {
    let mut output = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == '-' {
            output.push('_');
        }
    }

    let trimmed = output.trim_matches('_');
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn normalize_mcp_input_schema(schema: Value) -> Value {
    fn normalize(node: &Value) -> Value {
        match node {
            Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
            Value::Object(object) => {
                let mut normalized = serde_json::Map::new();
                for (key, value) in object {
                    normalized.insert(key.clone(), normalize(value));
                }

                let original_properties = object
                    .get("properties")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let mut required = normalized
                    .get("required")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if let Some(properties) = normalized
                    .get_mut("properties")
                    .and_then(Value::as_object_mut)
                {
                    for (name, property) in properties.iter_mut() {
                        let Some(original_property) =
                            original_properties.get(name).and_then(Value::as_object)
                        else {
                            continue;
                        };
                        let Some(required_flag) =
                            original_property.get("required").and_then(Value::as_bool)
                        else {
                            continue;
                        };
                        if let Some(property_object) = property.as_object_mut() {
                            property_object.remove("required");
                        }
                        if required_flag {
                            required.push(Value::String(name.clone()));
                        }
                    }

                    if required.is_empty() {
                        normalized.remove("required");
                    } else {
                        let mut unique = Vec::new();
                        for value in required {
                            if unique.iter().all(|existing| existing != &value) {
                                unique.push(value);
                            }
                        }
                        normalized.insert("required".to_string(), Value::Array(unique));
                    }
                }

                Value::Object(normalized)
            }
            _ => node.clone(),
        }
    }

    normalize(&schema)
}
