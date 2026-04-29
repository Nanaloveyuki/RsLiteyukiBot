use std::io::Cursor;

use super::*;
use zip::ZipArchive;

fn unique_web_host_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

pub(super) fn install_local_plugin_archive(
    request: &[u8],
    runtime_host: Option<&EmbeddedAppHost>,
) -> Result<Value, String> {
    let upload = parse_multipart_form_data(request)?
        .into_iter()
        .find(|field| field.name == "plugin")
        .ok_or_else(|| "missing plugin upload field".to_string())?;
    let filename = upload
        .filename
        .as_deref()
        .and_then(|value| {
            Path::new(value)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::trim)
        })
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "plugin filename is required".to_string())?
        .to_string();
    if !filename.to_ascii_lowercase().ends_with(".zip") {
        return Err("plugin package must be a .zip archive".to_string());
    }
    if upload.data.is_empty() {
        return Err("plugin package is empty".to_string());
    }

    let plugin_root = resolve_local_plugin_dir();
    fs::create_dir_all(&plugin_root)
        .map_err(|err| format!("failed to create plugin directory: {err}"))?;

    let nonce = unique_web_host_nonce();
    let stage_root = plugin_root.join(format!(".plugin-import-{nonce}"));
    let extracted_root = stage_root.join("extract");
    let install_root = stage_root.join("install");
    fs::create_dir_all(&extracted_root)
        .map_err(|err| format!("failed to create staging directory: {err}"))?;
    fs::create_dir_all(&install_root)
        .map_err(|err| format!("failed to create install staging directory: {err}"))?;

    let install_result = (|| {
        unpack_zip_archive(upload.data.as_slice(), extracted_root.as_path())?;
        let install_summary = match find_plugin_root_in_extracted_dir(extracted_root.as_path())? {
            ExtractedPluginArchive::NativeManifest { root } => install_native_plugin_archive(
                plugin_root.as_path(),
                install_root.as_path(),
                &root,
                runtime_host,
            )?,
            ExtractedPluginArchive::SourceAdapterBundle { root } => install_source_adapter_bundle(
                plugin_root.as_path(),
                install_root.as_path(),
                &root,
                runtime_host,
            )?,
        };

        if let Some(runtime_host) = runtime_host {
            let previous_disabled =
                run_async_for_web_host(runtime_host.plugin_catalog_snapshot()).disabled_plugin_ids;
            if let Err(err) = run_async_for_web_host(
                runtime_host.apply_disabled_plugins(previous_disabled.clone()),
            ) {
                rollback_installed_plugin_paths(install_summary.install_targets.as_slice());
                let _ =
                    run_async_for_web_host(runtime_host.apply_disabled_plugins(previous_disabled));
                return Err(format!(
                    "plugin installed but runtime reload failed, rolled back: {err}"
                ));
            }
        }

        Ok(install_summary)
    })();

    let _ = fs::remove_dir_all(&stage_root);

    let install_summary = install_result?;
    let primary_plugin_id = install_summary.plugin_ids.first().cloned();
    let primary_install_path = (install_summary.install_targets.len() == 1)
        .then(|| install_summary.install_targets[0].display().to_string());
    Ok(serde_json::json!({
        "message": if install_summary.plugin_ids.len() == 1 {
            format!("插件 {} 已安装", install_summary.plugin_ids[0])
        } else {
            format!("已安装 {} 个插件", install_summary.plugin_ids.len())
        },
        "pluginId": primary_plugin_id,
        "pluginIds": install_summary.plugin_ids,
        "installPath": primary_install_path,
        "installPaths": install_summary
            .install_targets
            .into_iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "installRoot": plugin_root.display().to_string(),
    }))
}

#[derive(Debug)]
enum ExtractedPluginArchive {
    NativeManifest { root: PathBuf },
    SourceAdapterBundle { root: PathBuf },
}

#[derive(Debug)]
struct InstalledPluginArchive {
    plugin_ids: Vec<String>,
    install_targets: Vec<PathBuf>,
}

fn install_native_plugin_archive(
    plugin_root: &Path,
    install_root: &Path,
    plugin_source_dir: &Path,
    runtime_host: Option<&EmbeddedAppHost>,
) -> Result<InstalledPluginArchive, String> {
    let manifest = PluginManifestLoader::load_manifest(&plugin_source_dir.join("plugin.json"))
        .map_err(|err| err.to_string())?;
    let plugin_id = manifest.descriptor.metadata.id.trim().to_string();
    if plugin_id.is_empty() {
        return Err("plugin manifest id is empty".to_string());
    }
    if plugin_id_already_exists(plugin_id.as_str(), runtime_host) {
        return Err(format!("plugin '{plugin_id}' already exists"));
    }

    let final_dir = plugin_root.join(&plugin_id);
    if final_dir.exists() {
        return Err(format!("plugin '{plugin_id}' already exists"));
    }

    let staged_plugin_dir = install_root.join(&plugin_id);
    copy_directory_recursive(plugin_source_dir, staged_plugin_dir.as_path())?;
    PluginManifestLoader::load_manifest(&staged_plugin_dir.join("plugin.json"))
        .map_err(|err| err.to_string())?;
    fs::rename(&staged_plugin_dir, &final_dir)
        .map_err(|err| format!("failed to finalize plugin install: {err}"))?;

    Ok(InstalledPluginArchive {
        plugin_ids: vec![plugin_id],
        install_targets: vec![final_dir],
    })
}

fn install_source_adapter_bundle(
    plugin_root: &Path,
    install_root: &Path,
    bundle_root: &Path,
    runtime_host: Option<&EmbeddedAppHost>,
) -> Result<InstalledPluginArchive, String> {
    let manifests =
        discover_plugin_manifests_in_dirs([bundle_root]).map_err(|err| err.to_string())?;
    if manifests.is_empty() {
        return Err(
            "plugin archive does not contain any source-adapter override manifests".to_string(),
        );
    }

    let mut plugin_ids = manifests
        .iter()
        .map(|manifest| manifest.descriptor.metadata.id.trim().to_string())
        .collect::<Vec<_>>();
    if plugin_ids.iter().any(|plugin_id| plugin_id.is_empty()) {
        return Err("plugin manifest id is empty".to_string());
    }
    for plugin_id in &plugin_ids {
        if plugin_id_already_exists(plugin_id.as_str(), runtime_host) {
            return Err(format!("plugin '{plugin_id}' already exists"));
        }
    }

    let install_relative_paths =
        install_relative_paths_from_source_bundle(bundle_root, manifests.as_slice())?;
    for relative_path in &install_relative_paths {
        let source_path = bundle_root.join(relative_path);
        if !source_path.exists() {
            return Err(format!(
                "plugin archive is missing referenced path {}",
                source_path.display()
            ));
        }

        let final_path = plugin_root.join(relative_path);
        if final_path.exists() {
            return Err(format!(
                "plugin install target already exists: {}",
                final_path.display()
            ));
        }
    }

    for relative_path in &install_relative_paths {
        let source_path = bundle_root.join(relative_path);
        let staged_path = install_root.join(relative_path);
        copy_install_path(source_path.as_path(), staged_path.as_path())?;
    }

    let staged_manifests =
        discover_plugin_manifests_in_dirs([install_root]).map_err(|err| err.to_string())?;
    let staged_plugin_ids = staged_manifests
        .into_iter()
        .map(|manifest| manifest.descriptor.metadata.id)
        .collect::<HashSet<_>>();
    let expected_plugin_ids = plugin_ids.iter().cloned().collect::<HashSet<_>>();
    if staged_plugin_ids != expected_plugin_ids {
        return Err(
            "staged source-adapter plugin bundle did not round-trip through discovery".to_string(),
        );
    }

    let install_targets = finalize_source_adapter_install(
        plugin_root,
        install_root,
        install_relative_paths.as_slice(),
    )?;
    plugin_ids.sort();

    Ok(InstalledPluginArchive {
        plugin_ids,
        install_targets,
    })
}

pub(super) fn unpack_zip_archive(archive_bytes: &[u8], target_dir: &Path) -> Result<(), String> {
    let reader = Cursor::new(archive_bytes);
    let mut archive =
        ZipArchive::new(reader).map_err(|err| format!("failed to open zip archive: {err}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("failed to read zip entry {index}: {err}"))?;
        let Some(relative_path) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
            return Err("zip archive contains an invalid path".to_string());
        };
        if relative_path.as_os_str().is_empty() {
            continue;
        }
        let output_path = target_dir.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|err| format!("failed to create extracted directory: {err}"))?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create extracted parent directory: {err}"))?;
        }
        let mut output = fs::File::create(&output_path)
            .map_err(|err| format!("failed to create extracted file: {err}"))?;
        io::copy(&mut entry, &mut output)
            .map_err(|err| format!("failed to extract archive entry: {err}"))?;
    }
    Ok(())
}

fn plugin_id_already_exists(plugin_id: &str, runtime_host: Option<&EmbeddedAppHost>) -> bool {
    runtime_host
        .map(|host| {
            run_async_for_web_host(host.plugin_catalog_snapshot())
                .entries
                .into_iter()
                .any(|entry| entry.descriptor.metadata.id == plugin_id)
        })
        .unwrap_or_else(|| {
            let plugin_dirs = resolve_builtin_plugin_dirs();
            discover_plugin_manifests_in_dirs(plugin_dirs.iter())
                .map(|manifests| {
                    manifests
                        .into_iter()
                        .any(|manifest| manifest.descriptor.metadata.id == plugin_id)
                })
                .unwrap_or(false)
        })
}

fn find_plugin_root_in_extracted_dir(root: &Path) -> Result<ExtractedPluginArchive, String> {
    let mut native_candidates = Vec::new();
    let mut source_bundle_candidates = Vec::new();
    for candidate in extracted_archive_root_candidates(root)? {
        if candidate.join("plugin.json").is_file() {
            native_candidates.push(candidate.clone());
            continue;
        }
        if is_source_adapter_bundle_root(candidate.as_path())? {
            source_bundle_candidates.push(candidate);
        }
    }

    match (native_candidates.len(), source_bundle_candidates.len()) {
        (1, 0) => Ok(ExtractedPluginArchive::NativeManifest {
            root: native_candidates.remove(0),
        }),
        (0, 1) => Ok(ExtractedPluginArchive::SourceAdapterBundle {
            root: source_bundle_candidates.remove(0),
        }),
        (0, 0) => Err(
            "plugin.json or source-adapter manifests were not found in the archive root"
                .to_string(),
        ),
        _ => Err("plugin archive contains multiple plugin roots".to_string()),
    }
}

fn extracted_archive_root_candidates(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut candidates = vec![root.to_path_buf()];
    let entries =
        fs::read_dir(root).map_err(|err| format!("failed to inspect extracted plugin: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to inspect extracted plugin: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            candidates.push(path);
        }
    }
    Ok(candidates)
}

fn is_source_adapter_bundle_root(root: &Path) -> Result<bool, String> {
    let manifest_dir = root.join(SOURCE_ADAPTER_MANIFEST_DIR);
    if !manifest_dir.is_dir() {
        return Ok(false);
    }

    let mut has_override_manifest = false;
    let entries = fs::read_dir(&manifest_dir)
        .map_err(|err| format!("failed to inspect source-adapter manifests: {err}"))?;
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("failed to inspect source-adapter manifests: {err}"))?;
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with(SOURCE_ADAPTER_OVERRIDE_SUFFIX))
        {
            has_override_manifest = true;
            break;
        }
    }
    if !has_override_manifest {
        return Ok(false);
    }

    let manifests = discover_plugin_manifests_in_dirs([root]).map_err(|err| err.to_string())?;
    Ok(!manifests.is_empty())
}

pub(super) fn copy_directory_recursive(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target)
        .map_err(|err| format!("failed to create install directory: {err}"))?;
    let entries = fs::read_dir(source).map_err(|err| {
        format!(
            "failed to read plugin directory {}: {err}",
            source.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to read plugin directory entry: {err}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry
            .metadata()
            .map_err(|err| format!("failed to stat plugin entry: {err}"))?;
        if metadata.is_dir() {
            copy_directory_recursive(source_path.as_path(), target_path.as_path())?;
        } else if metadata.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("failed to create plugin parent directory: {err}"))?;
            }
            fs::copy(&source_path, &target_path)
                .map_err(|err| format!("failed to copy plugin file: {err}"))?;
        }
    }
    Ok(())
}

fn copy_install_path(source: &Path, target: &Path) -> Result<(), String> {
    if source.is_dir() {
        return copy_directory_recursive(source, target);
    }
    if !source.is_file() {
        return Err(format!(
            "plugin archive install source does not exist: {}",
            source.display()
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create plugin parent directory: {err}"))?;
    }
    fs::copy(source, target).map_err(|err| format!("failed to copy plugin file: {err}"))?;
    Ok(())
}

fn install_relative_paths_from_source_bundle(
    bundle_root: &Path,
    manifests: &[crate::PluginManifest],
) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for manifest in manifests {
        let source_path = descriptor_relative_install_path(
            &manifest.descriptor,
            "sourcePath",
            manifest.path.as_path(),
        )?;
        for key in ["sourcePath", "overrideManifestPath"] {
            let relative_path = descriptor_relative_install_path(
                &manifest.descriptor,
                key,
                manifest.path.as_path(),
            )?;
            if seen.insert(relative_path.clone()) {
                paths.push(relative_path);
            }
        }
        for metadata_path in descriptor_metadata_install_paths(
            bundle_root,
            &manifest.descriptor,
            source_path.as_path(),
            manifest.path.as_path(),
        )? {
            if seen.insert(metadata_path.clone()) {
                paths.push(metadata_path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn descriptor_relative_install_path(
    descriptor: &crate::PluginDescriptor,
    key: &str,
    manifest_path: &Path,
) -> Result<PathBuf, String> {
    let raw = descriptor_family_value(descriptor, key)
        .and_then(|value| value.as_str().map(ToString::to_string))
        .ok_or_else(|| {
            format!(
                "manifest {} is missing descriptor extra '{}'",
                manifest_path.display(),
                key
            )
        })?;
    sanitize_plugin_archive_relative_path(raw.as_str()).map_err(|err| {
        format!(
            "manifest {} has invalid descriptor extra '{}': {}",
            manifest_path.display(),
            key,
            err
        )
    })
}

fn sanitize_plugin_archive_relative_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_start_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err("path should not be empty".to_string());
    }

    let mut output = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(segment) => output.push(segment),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("path escapes plugin root".to_string());
            }
        }
    }
    if output.as_os_str().is_empty() {
        return Err("path should not be empty".to_string());
    }
    Ok(output)
}

fn descriptor_metadata_install_paths(
    bundle_root: &Path,
    descriptor: &crate::PluginDescriptor,
    source_path: &Path,
    manifest_path: &Path,
) -> Result<Vec<PathBuf>, String> {
    let Some(value) = descriptor_family_value(descriptor, "metadataFiles") else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(format!(
            "manifest {} has invalid descriptor extra 'metadataFiles'",
            manifest_path.display()
        ));
    };

    let mut metadata_paths = Vec::new();
    for item in items {
        let raw = item.as_str().ok_or_else(|| {
            format!(
                "manifest {} has non-string metadataFiles entry",
                manifest_path.display()
            )
        })?;
        let candidate = sanitize_plugin_archive_relative_path(raw).map_err(|err| {
            format!(
                "manifest {} has invalid metadataFiles entry '{}': {}",
                manifest_path.display(),
                raw,
                err
            )
        })?;

        let resolved_path = if bundle_root.join(&candidate).exists() {
            candidate
        } else {
            source_path.join(candidate)
        };
        if resolved_path.starts_with(source_path) {
            continue;
        }
        metadata_paths.push(resolved_path);
    }
    Ok(metadata_paths)
}

pub(super) fn finalize_source_adapter_install(
    plugin_root: &Path,
    install_root: &Path,
    relative_paths: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    let mut created_targets = Vec::new();
    for relative_path in relative_paths {
        let staged_path = install_root.join(relative_path);
        let final_path = plugin_root.join(relative_path);
        if final_path.exists() {
            rollback_installed_plugin_paths(created_targets.as_slice());
            return Err(format!(
                "plugin install target already exists: {}",
                final_path.display()
            ));
        }
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create plugin parent directory: {err}"))?;
        }
        if let Err(err) = fs::rename(&staged_path, &final_path) {
            rollback_installed_plugin_paths(created_targets.as_slice());
            return Err(format!("failed to finalize plugin install: {err}"));
        }
        created_targets.push(final_path);
    }
    Ok(created_targets)
}

fn rollback_installed_plugin_paths(paths: &[PathBuf]) {
    let mut ordered_paths = paths.to_vec();
    ordered_paths.sort_by(|left, right| {
        right
            .components()
            .count()
            .cmp(&left.components().count())
            .then_with(|| right.cmp(left))
    });
    for path in ordered_paths {
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }
}
