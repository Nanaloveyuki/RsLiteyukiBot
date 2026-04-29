use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::plugin::{
    PluginRuntimeKind, PluginRuntimeSpec, PluginSdkSpec, PluginType, source_adapter::model::*,
};

pub(crate) fn build_family_seed(
    family: SourcePluginFamily,
    plugin_id: &str,
    source_root: &Path,
) -> Result<FamilyDescriptorSeed, String> {
    match family {
        SourcePluginFamily::Native => build_native_seed(source_root),
        SourcePluginFamily::LiteyukiPy => build_liteyuki_py_seed(plugin_id, source_root),
        SourcePluginFamily::Astrbot => build_astrbot_seed(plugin_id, source_root),
        SourcePluginFamily::Neomofox => build_neomofox_seed(plugin_id, source_root),
        SourcePluginFamily::Nonebot => Ok(build_nonebot_seed(plugin_id, source_root)),
    }
}

#[derive(Debug, Deserialize, Default)]
struct AstrbotMetadataDoc {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    support_platforms: Option<Vec<String>>,
    #[serde(default)]
    astrbot_version: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct NeomofoxManifestDoc {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    entry_point: Option<String>,
    #[serde(default)]
    min_core_version: Option<String>,
    #[serde(default)]
    python_dependencies: Option<Vec<String>>,
    #[serde(default)]
    include: Option<Vec<Value>>,
}

fn build_native_seed(source_root: &Path) -> Result<FamilyDescriptorSeed, String> {
    let manifest_path = source_root.join("plugin.json");
    let manifest = crate::plugin::PluginManifestLoader::load_manifest(&manifest_path)
        .map_err(|err| err.to_string())?;
    Ok(FamilyDescriptorSeed {
        plugin_id_hint: Some(manifest.descriptor.metadata.id.clone()),
        name: Some(manifest.descriptor.metadata.name),
        description: Some(manifest.descriptor.metadata.description),
        plugin_type: Some(manifest.descriptor.metadata.plugin_type),
        author: Some(manifest.descriptor.metadata.author),
        homepage: Some(manifest.descriptor.metadata.homepage),
        runtime: manifest.descriptor.runtime,
        sdk: manifest.descriptor.sdk,
        permissions: manifest.descriptor.permissions,
        commands: manifest.descriptor.commands,
        extra: manifest.descriptor.metadata.extra,
        source_manifest_name: Some("plugin.json".to_string()),
    })
}

fn build_liteyuki_py_seed(
    plugin_id: &str,
    source_root: &Path,
) -> Result<FamilyDescriptorSeed, String> {
    let metadata_file = source_root.join("__init__.py");
    let content = std::fs::read_to_string(&metadata_file).map_err(|err| {
        format!(
            "failed to read liteyuki plugin metadata {}: {err}",
            metadata_file.display()
        )
    })?;
    let meta_block = extract_python_metadata_call(content.as_str()).ok_or_else(|| {
        format!(
            "failed to find __plugin_meta__ in {}",
            metadata_file.display()
        )
    })?;
    let parsed = parse_liteyuki_metadata(meta_block.as_str());

    let mut runtime = PluginRuntimeSpec {
        kind: PluginRuntimeKind::Python,
        entrypoint: module_name_from_source_root(source_root),
        abi: "liteyuki-python-bridge".to_string(),
        ..PluginRuntimeSpec::default()
    };
    runtime.options.insert(
        "compat_family".to_string(),
        Value::String("liteyuki_py".to_string()),
    );

    Ok(FamilyDescriptorSeed {
        plugin_id_hint: source_root
            .file_name()
            .and_then(|value| value.to_str())
            .map(host_plugin_id_hint),
        name: parsed
            .name
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some(plugin_id.to_string())),
        description: parsed.description,
        plugin_type: parsed.plugin_type,
        author: parsed.author,
        homepage: parsed.homepage,
        runtime,
        sdk: PluginSdkSpec::default(),
        permissions: Vec::new(),
        commands: Vec::new(),
        extra: HashMap::new(),
        source_manifest_name: Some("__init__.py".to_string()),
    })
}

fn build_astrbot_seed(plugin_id: &str, source_root: &Path) -> Result<FamilyDescriptorSeed, String> {
    let metadata_file = source_root.join("metadata.yaml");
    let content = std::fs::read_to_string(&metadata_file).map_err(|err| {
        format!(
            "failed to read astrbot metadata {}: {err}",
            metadata_file.display()
        )
    })?;
    let metadata: AstrbotMetadataDoc = serde_yaml::from_str(content.as_str()).map_err(|err| {
        format!(
            "failed to parse astrbot metadata {}: {err}",
            metadata_file.display()
        )
    })?;

    let mut runtime = PluginRuntimeSpec {
        kind: PluginRuntimeKind::Python,
        entrypoint: "main".to_string(),
        abi: "liteyuki-python-bridge".to_string(),
        ..PluginRuntimeSpec::default()
    };
    runtime.options.insert(
        "compat_family".to_string(),
        Value::String("astrbot".to_string()),
    );

    let mut permissions = Vec::new();
    let schema_path = source_root.join("_conf_schema.json");
    let mut extra = HashMap::new();
    if schema_path.is_file() {
        permissions.push("config.read".to_string());
        permissions.push("config.write".to_string());
        extra.insert(
            "configSchemaPath".to_string(),
            Value::String(schema_path.display().to_string()),
        );
    }
    if let Some(version) = metadata
        .version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        extra.insert("version".to_string(), Value::String(version.to_string()));
    }
    if let Some(repo) = metadata
        .repo
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        extra.insert("repository".to_string(), Value::String(repo.to_string()));
    }
    if let Some(platforms) = metadata.support_platforms.filter(|items| !items.is_empty()) {
        extra.insert(
            "supportPlatforms".to_string(),
            Value::Array(platforms.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(version_range) = metadata
        .astrbot_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        extra.insert(
            "astrbotVersion".to_string(),
            Value::String(version_range.to_string()),
        );
    }

    Ok(FamilyDescriptorSeed {
        plugin_id_hint: metadata
            .name
            .as_deref()
            .map(host_plugin_id_hint)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                source_root
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(host_plugin_id_hint)
            }),
        name: metadata
            .display_name
            .filter(|value| !value.trim().is_empty())
            .or(metadata.name.filter(|value| !value.trim().is_empty()))
            .or_else(|| Some(plugin_id.to_string())),
        description: metadata
            .description
            .filter(|value| !value.trim().is_empty())
            .or(metadata.desc.filter(|value| !value.trim().is_empty())),
        plugin_type: Some(PluginType::Service),
        author: metadata.author.filter(|value| !value.trim().is_empty()),
        homepage: metadata
            .homepage
            .filter(|value| !value.trim().is_empty())
            .or(metadata.repo.filter(|value| !value.trim().is_empty())),
        runtime,
        sdk: PluginSdkSpec::default(),
        permissions,
        commands: Vec::new(),
        extra,
        source_manifest_name: Some("metadata.yaml".to_string()),
    })
}

fn build_neomofox_seed(
    plugin_id: &str,
    source_root: &Path,
) -> Result<FamilyDescriptorSeed, String> {
    let manifest_file = source_root.join("manifest.json");
    let content = std::fs::read_to_string(&manifest_file).map_err(|err| {
        format!(
            "failed to read Neo-MoFox manifest {}: {err}",
            manifest_file.display()
        )
    })?;
    let metadata: NeomofoxManifestDoc = serde_json::from_str(content.as_str()).map_err(|err| {
        format!(
            "failed to parse Neo-MoFox manifest {}: {err}",
            manifest_file.display()
        )
    })?;

    let entrypoint =
        neomofox_entrypoint_from_manifest(source_root, metadata.entry_point.as_deref());
    let mut runtime = PluginRuntimeSpec {
        kind: PluginRuntimeKind::Python,
        entrypoint,
        abi: "liteyuki-python-bridge".to_string(),
        ..PluginRuntimeSpec::default()
    };
    runtime.options.insert(
        "compat_family".to_string(),
        Value::String("neomofox".to_string()),
    );

    let mut extra = HashMap::new();
    if let Some(version) = metadata
        .version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        extra.insert("version".to_string(), Value::String(version.to_string()));
    }
    if let Some(version_range) = metadata
        .min_core_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        extra.insert(
            "neomofoxMinCoreVersion".to_string(),
            Value::String(version_range.to_string()),
        );
    }
    if let Some(dependencies) = metadata
        .python_dependencies
        .filter(|items| !items.is_empty())
    {
        extra.insert(
            "pythonDependencies".to_string(),
            Value::Array(dependencies.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(include) = metadata.include.filter(|items| !items.is_empty()) {
        extra.insert("neomofoxComponents".to_string(), Value::Array(include));
    }

    Ok(FamilyDescriptorSeed {
        plugin_id_hint: metadata
            .name
            .as_deref()
            .map(host_plugin_id_hint)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                source_root
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(host_plugin_id_hint)
            }),
        name: metadata
            .name
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some(plugin_id.to_string())),
        description: metadata
            .description
            .filter(|value| !value.trim().is_empty()),
        plugin_type: Some(PluginType::Service),
        author: metadata.author.filter(|value| !value.trim().is_empty()),
        homepage: None,
        runtime,
        sdk: PluginSdkSpec::default(),
        permissions: Vec::new(),
        commands: Vec::new(),
        extra,
        source_manifest_name: Some("manifest.json".to_string()),
    })
}

fn build_nonebot_seed(plugin_id: &str, source_root: &Path) -> FamilyDescriptorSeed {
    let mut runtime = PluginRuntimeSpec {
        kind: PluginRuntimeKind::External,
        abi: "nonebot-external".to_string(),
        ..PluginRuntimeSpec::default()
    };
    runtime.options.insert(
        "compat_family".to_string(),
        Value::String("nonebot".to_string()),
    );
    runtime.options.insert(
        "source_root".to_string(),
        Value::String(source_root.display().to_string()),
    );

    let mut extra = HashMap::new();
    extra.insert(
        "runtimeSupport".to_string(),
        Value::String("metadata_only".to_string()),
    );

    FamilyDescriptorSeed {
        plugin_id_hint: source_root
            .file_name()
            .and_then(|value| value.to_str())
            .map(host_plugin_id_hint),
        name: Some(plugin_id.to_string()),
        description: Some("NoneBot plugin mapped through host metadata only.".to_string()),
        plugin_type: Some(PluginType::Module),
        author: None,
        homepage: None,
        runtime,
        sdk: PluginSdkSpec::default(),
        permissions: Vec::new(),
        commands: Vec::new(),
        extra,
        source_manifest_name: None,
    }
}

fn neomofox_entrypoint_from_manifest(source_root: &Path, entry_point: Option<&str>) -> String {
    let package = source_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("plugin");
    let raw = entry_point
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("plugin.py")
        .replace('\\', "/");
    let module = raw
        .trim_end_matches(".py")
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    if module.is_empty() || module == "__init__" {
        package.to_string()
    } else {
        format!("{package}.{module}")
    }
}

fn module_name_from_source_root(source_root: &Path) -> String {
    source_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| {
            value
                .trim()
                .chars()
                .map(|ch| match ch {
                    '-' | ' ' => '_',
                    _ if ch.is_ascii_alphanumeric() || ch == '_' => ch,
                    _ => '_',
                })
                .collect::<String>()
                .trim_matches('_')
                .to_string()
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "plugin".to_string())
}

fn host_plugin_id_hint(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|ch| {
            let normalized = ch.to_ascii_lowercase();
            if normalized.is_ascii_alphanumeric() {
                normalized
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[derive(Debug, Default)]
struct LiteyukiMetadataDoc {
    name: Option<String>,
    description: Option<String>,
    plugin_type: Option<PluginType>,
    author: Option<String>,
    homepage: Option<String>,
}

fn extract_python_metadata_call(content: &str) -> Option<String> {
    let marker_idx = content.find("__plugin_meta__")?;
    let metadata_idx = content[marker_idx..].find("PluginMetadata(")? + marker_idx;
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut started = false;
    let mut output = String::new();
    for &byte in &bytes[metadata_idx + "PluginMetadata".len()..] {
        let ch = byte as char;
        if ch == '(' {
            depth += 1;
            started = true;
            if depth == 1 {
                continue;
            }
        } else if ch == ')' {
            if depth == 0 {
                return None;
            }
            depth -= 1;
            if depth == 0 {
                break;
            }
        }

        if started && depth >= 1 {
            output.push(ch);
        }
    }
    (!output.trim().is_empty()).then_some(output)
}

fn parse_liteyuki_metadata(block: &str) -> LiteyukiMetadataDoc {
    let mut doc = LiteyukiMetadataDoc::default();
    for line in block.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "name" => doc.name = parse_python_string_literal(value),
                "description" => doc.description = parse_python_string_literal(value),
                "author" => doc.author = parse_python_string_literal(value),
                "homepage" => doc.homepage = parse_python_string_literal(value),
                "type" => doc.plugin_type = parse_plugin_type(value),
                _ => {}
            }
        }
    }
    doc
}

fn parse_python_string_literal(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let first = trimmed.chars().next()?;
    let last = trimmed.chars().last()?;
    if !matches!(first, '"' | '\'') || first != last {
        return None;
    }
    Some(trimmed[1..trimmed.len().saturating_sub(1)].to_string())
}

fn parse_plugin_type(raw: &str) -> Option<PluginType> {
    let compact = raw.trim().to_ascii_lowercase().replace([' ', '_'], "");
    match compact.as_str() {
        "plugintype.application" | "application" => Some(PluginType::Application),
        "plugintype.service" | "service" => Some(PluginType::Service),
        "plugintype.module" | "module" => Some(PluginType::Module),
        "plugintype.test" | "test" => Some(PluginType::Test),
        "plugintype.unclassified" | "unclassified" => Some(PluginType::Unclassified),
        _ => None,
    }
}
