use std::collections::HashSet;
use std::path::Path;

use crate::plugin::loader::{PluginManifest, PluginManifestError};

use super::super::model::{OVERRIDE_MANIFEST_DIR, OVERRIDE_MANIFEST_SUFFIX};
use super::synthesis::load_override_manifest;

pub fn discover_plugin_manifests_in_dirs<I, P>(
    dirs: I,
) -> Result<Vec<PluginManifest>, PluginManifestError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut manifests = Vec::new();
    let mut seen_ids = HashSet::new();
    for dir in dirs {
        let dir = dir.as_ref();
        if !dir.exists() {
            continue;
        }

        discover_native_manifests_in_dir(dir, &mut manifests, &mut seen_ids)?;
        discover_override_manifests_in_dir(dir, &mut manifests, &mut seen_ids)?;
    }
    Ok(manifests)
}

fn discover_native_manifests_in_dir(
    dir: &Path,
    manifests: &mut Vec<PluginManifest>,
    seen_ids: &mut HashSet<String>,
) -> Result<(), PluginManifestError> {
    if dir.is_file() {
        if dir.file_name().and_then(|name| name.to_str()) == Some("plugin.json") {
            push_unique_manifest(
                crate::plugin::PluginManifestLoader::load_manifest(dir)?,
                manifests,
                seen_ids,
            );
        }
        return Ok(());
    }

    let root_manifest = dir.join("plugin.json");
    if root_manifest.is_file() {
        push_unique_manifest(
            crate::plugin::PluginManifestLoader::load_manifest(&root_manifest)?,
            manifests,
            seen_ids,
        );
    }

    let entries = std::fs::read_dir(dir).map_err(|err| {
        PluginManifestError::Io(format!("read_dir failed for {}: {}", dir.display(), err))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            PluginManifestError::Io(format!(
                "read_dir entry failed for {}: {}",
                dir.display(),
                err
            ))
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = path.join("plugin.json");
        if manifest.is_file() {
            push_unique_manifest(
                crate::plugin::PluginManifestLoader::load_manifest(&manifest)?,
                manifests,
                seen_ids,
            );
        }
    }
    Ok(())
}

fn discover_override_manifests_in_dir(
    dir: &Path,
    manifests: &mut Vec<PluginManifest>,
    seen_ids: &mut HashSet<String>,
) -> Result<(), PluginManifestError> {
    if !dir.is_dir() {
        return Ok(());
    }
    let manifest_dir = dir.join(OVERRIDE_MANIFEST_DIR);
    if !manifest_dir.is_dir() {
        return Ok(());
    }

    let mut files = std::fs::read_dir(&manifest_dir)
        .map_err(|err| {
            PluginManifestError::Io(format!(
                "read_dir failed for {}: {}",
                manifest_dir.display(),
                err
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            PluginManifestError::Io(format!(
                "read_dir entry failed for {}: {}",
                manifest_dir.display(),
                err
            ))
        })?;
    files.sort_by_key(|entry| entry.file_name());

    for entry in files {
        let path = entry.path();
        let Some(filename) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !path.is_file() || !filename.ends_with(OVERRIDE_MANIFEST_SUFFIX) {
            continue;
        }
        let manifest = load_override_manifest(dir, &path)?;
        push_unique_manifest(manifest, manifests, seen_ids);
    }
    Ok(())
}

fn push_unique_manifest(
    manifest: PluginManifest,
    manifests: &mut Vec<PluginManifest>,
    seen_ids: &mut HashSet<String>,
) {
    let id = manifest.descriptor.metadata.id.clone();
    if seen_ids.insert(id) {
        manifests.push(manifest);
    }
}
