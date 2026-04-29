use super::*;

use std::process::Command;

use serde::Serialize;

use crate::llm::skills::{SkillCatalogEntry, SkillManager, extract_skill_frontmatter_value};

const MAX_SKILL_SEARCH_DEPTH: usize = 6;
const SKILL_UPLOAD_FIELD: &str = "skill";
const SKILL_OVERWRITE_FIELD: &str = "overwrite";
const SKILL_ENTRY_FILE: &str = "SKILL.md";
const LEGACY_SKILL_ENTRY_FILES: &[&str] = &["Skill.md", "skill.md"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillImportResponse {
    skills: Vec<SkillCatalogEntry>,
    count: usize,
    managed_root: String,
}

pub(super) fn import_skills_payload(request: &[u8]) -> Result<serde_json::Value, String> {
    let fields = parse_multipart_form_data(request)?;
    let overwrite = fields
        .iter()
        .find(|field| field.name == SKILL_OVERWRITE_FIELD)
        .map(parse_bool_field)
        .transpose()?
        .unwrap_or(false);
    let uploads = fields
        .into_iter()
        .filter(|field| field.name == SKILL_UPLOAD_FIELD)
        .collect::<Vec<_>>();
    if uploads.is_empty() {
        return Err("skills/import requires at least one uploaded file".to_string());
    }

    let workspace_root = capability_workspace_root();
    let manager = SkillManager::for_workspace(workspace_root.as_path());
    let stage_root = temp_stage_root("skills-import");
    fs::create_dir_all(&stage_root).map_err(|err| {
        format!(
            "failed to create temporary skill import directory {}: {err}",
            stage_root.display()
        )
    })?;

    let result = import_uploaded_fields(&manager, uploads.as_slice(), overwrite, &stage_root).map(
        |skills| {
            serde_json::to_value(SkillImportResponse {
                count: skills.len(),
                skills,
                managed_root: display_skill_path(manager.managed_root(), workspace_root.as_path()),
            })
            .map_err(|err| format!("failed to serialize skill import response: {err}"))
        },
    );
    let _ = fs::remove_dir_all(&stage_root);
    result?
}

pub(super) fn normalize_skill_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("skill name should not be empty".to_string());
    }

    let mut normalized = String::with_capacity(trimmed.len());
    let mut last_was_separator = false;
    for ch in trimmed.chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control() {
            return Err("skill name contains unsupported filesystem characters".to_string());
        }
        if ch.is_whitespace() {
            if !last_was_separator {
                normalized.push('-');
                last_was_separator = true;
            }
            continue;
        }
        normalized.push(ch);
        last_was_separator = false;
    }

    let normalized = normalized
        .trim_matches('.')
        .trim_matches('-')
        .trim()
        .to_string();
    if normalized.is_empty() {
        return Err("skill name should not be empty".to_string());
    }
    Ok(normalized)
}

fn import_uploaded_fields(
    manager: &SkillManager,
    uploads: &[MultipartField],
    overwrite: bool,
    stage_root: &Path,
) -> Result<Vec<SkillCatalogEntry>, String> {
    let mut installed = Vec::new();
    let mut loose_fields = Vec::new();
    let mut archive_index = 0;

    for upload in uploads {
        let filename = upload
            .filename
            .as_deref()
            .ok_or_else(|| "uploaded skill file is missing a filename".to_string())?;
        if is_archive_filename(filename) {
            let extract_root = stage_root.join(format!("archive-{archive_index}"));
            archive_index += 1;
            fs::create_dir_all(&extract_root)
                .map_err(|err| format!("failed to create archive staging directory: {err}"))?;
            extract_skill_archive(filename, upload.data.as_slice(), extract_root.as_path())?;
            let candidates = collect_skill_roots(extract_root.as_path())?;
            if candidates.is_empty() {
                return Err(format!(
                    "archive '{filename}' does not contain any skill package"
                ));
            }
            for candidate in candidates {
                installed.push(install_skill_root(manager, candidate.as_path(), overwrite)?);
            }
        } else {
            loose_fields.push(upload.clone());
        }
    }

    if !loose_fields.is_empty() {
        let loose_root = stage_root.join("loose-files");
        fs::create_dir_all(&loose_root)
            .map_err(|err| format!("failed to create loose file staging directory: {err}"))?;
        for upload in &loose_fields {
            write_uploaded_file(upload, loose_root.as_path())?;
        }
        let candidates = collect_skill_roots(loose_root.as_path())?;
        if candidates.is_empty() {
            return Err("uploaded files do not contain any skill package".to_string());
        }
        for candidate in candidates {
            installed.push(install_skill_root(manager, candidate.as_path(), overwrite)?);
        }
    }

    installed.sort_by(|left, right| left.name.cmp(&right.name));
    installed.dedup_by(|left, right| left.name == right.name);
    Ok(installed)
}

fn install_skill_root(
    manager: &SkillManager,
    source_root: &Path,
    overwrite: bool,
) -> Result<SkillCatalogEntry, String> {
    let entry_path = find_skill_entry_path(source_root)
        .ok_or_else(|| format!("skill root {} is missing SKILL.md", source_root.display()))?;
    let entry_bytes = fs::read(&entry_path)
        .map_err(|err| format!("failed to read skill file {}: {err}", entry_path.display()))?;
    let entry_content = String::from_utf8_lossy(&entry_bytes).into_owned();
    let skill_name = derive_skill_name(source_root, entry_path.as_path(), entry_content.as_str())?;
    let target_root = manager.managed_root().join(skill_name.as_str());

    if target_root.exists() && !overwrite {
        return Err(format!("skill '{}' already exists", skill_name));
    }
    replace_skill_tree(source_root, target_root.as_path(), overwrite)?;

    let target_entry = target_root.join(SKILL_ENTRY_FILE);
    fs::write(&target_entry, entry_content.as_bytes()).map_err(|err| {
        format!(
            "failed to normalize skill file {}: {err}",
            target_entry.display()
        )
    })?;
    if let Some(source_name) = entry_path.file_name().and_then(|value| value.to_str())
        && source_name != SKILL_ENTRY_FILE
    {
        let legacy_target = target_root.join(source_name);
        if legacy_target.exists() {
            let _ = fs::remove_file(legacy_target);
        }
    }

    resolve_installed_skill(manager, skill_name.as_str())
}

fn resolve_installed_skill(
    manager: &SkillManager,
    skill_name: &str,
) -> Result<SkillCatalogEntry, String> {
    let skill = manager
        .list_skills()?
        .into_iter()
        .find(|skill| skill.name == skill_name)
        .ok_or_else(|| format!("skill '{}' was not found after install", skill_name))?;
    manager
        .build_catalog(&[skill])
        .into_iter()
        .next()
        .ok_or_else(|| format!("skill '{}' catalog entry could not be built", skill_name))
}

fn replace_skill_tree(
    source_root: &Path,
    target_root: &Path,
    overwrite: bool,
) -> Result<(), String> {
    if !target_root.exists() {
        return super::plugin_install::copy_directory_recursive(source_root, target_root);
    }
    if !overwrite {
        return Err(format!(
            "skill install target already exists: {}",
            target_root.display()
        ));
    }

    let backup_root = target_root.with_extension(format!(
        "backup-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::rename(target_root, &backup_root).map_err(|err| {
        format!(
            "failed to move existing skill {} out of the way: {err}",
            target_root.display()
        )
    })?;

    match super::plugin_install::copy_directory_recursive(source_root, target_root) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&backup_root);
            Ok(())
        }
        Err(err) => {
            let _ = fs::remove_dir_all(target_root);
            let _ = fs::rename(&backup_root, target_root);
            Err(err)
        }
    }
}

fn write_uploaded_file(upload: &MultipartField, root: &Path) -> Result<(), String> {
    let filename = upload
        .filename
        .as_deref()
        .ok_or_else(|| "uploaded skill file is missing a filename".to_string())?;
    let relative_path = sanitize_upload_relative_path(filename)?;
    let output_path = root.join(relative_path);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create skill upload directory: {err}"))?;
    }
    fs::write(&output_path, upload.data.as_slice()).map_err(|err| {
        format!(
            "failed to write uploaded file {}: {err}",
            output_path.display()
        )
    })
}

fn collect_skill_roots(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    collect_skill_roots_recursive(root, 0, &mut roots)?;
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn collect_skill_roots_recursive(
    current: &Path,
    depth: usize,
    roots: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if find_skill_entry_path(current).is_some() {
        roots.push(current.to_path_buf());
        return Ok(());
    }
    if depth >= MAX_SKILL_SEARCH_DEPTH {
        return Ok(());
    }
    let entries = fs::read_dir(current).map_err(|err| {
        format!(
            "failed to inspect staged skill directory {}: {err}",
            current.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("failed to inspect staged skill entry: {err}"))?;
        let path = entry.path();
        if path.is_dir() {
            collect_skill_roots_recursive(path.as_path(), depth + 1, roots)?;
        }
    }
    Ok(())
}

fn derive_skill_name(
    source_root: &Path,
    entry_path: &Path,
    content: &str,
) -> Result<String, String> {
    let mut candidates = Vec::new();
    if let Some(frontmatter_name) = extract_skill_frontmatter_value(content, "name") {
        candidates.push(frontmatter_name);
    }
    if let Some(folder_name) = source_root.file_name().and_then(|value| value.to_str())
        && !folder_name.starts_with("archive-")
        && folder_name != "loose-files"
    {
        candidates.push(folder_name.to_string());
    }
    if let Some(stem) = entry_path.file_stem().and_then(|value| value.to_str())
        && !stem.eq_ignore_ascii_case("skill")
    {
        candidates.push(stem.to_string());
    }
    if let Some(first_heading) = first_markdown_heading(content) {
        candidates.push(first_heading);
    }

    for candidate in candidates {
        if let Ok(name) = normalize_skill_name(candidate.as_str()) {
            return Ok(name);
        }
    }
    Err(format!(
        "could not derive a valid skill name from {}",
        entry_path.display()
    ))
}

fn first_markdown_heading(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let heading = line
            .trim()
            .strip_prefix('#')?
            .trim()
            .trim_start_matches('#')
            .trim();
        if heading.is_empty() {
            None
        } else {
            Some(heading.to_string())
        }
    })
}

fn extract_skill_archive(
    filename: &str,
    archive_bytes: &[u8],
    target_dir: &Path,
) -> Result<(), String> {
    let lowercase = filename.to_ascii_lowercase();
    if lowercase.ends_with(".zip") {
        return super::plugin_install::unpack_zip_archive(archive_bytes, target_dir);
    }
    if lowercase.ends_with(".rar") || lowercase.ends_with(".7z") {
        return unpack_with_7z(filename, archive_bytes, target_dir);
    }
    Err(format!(
        "unsupported skill archive '{}'; only .zip, .rar and .7z are supported",
        filename
    ))
}

fn unpack_with_7z(filename: &str, archive_bytes: &[u8], target_dir: &Path) -> Result<(), String> {
    let executable = find_7z_executable()
        .ok_or_else(|| "importing .rar/.7z skills requires a 7z executable in PATH".to_string())?;
    let archive_path = target_dir.join(filename);
    fs::write(&archive_path, archive_bytes).map_err(|err| {
        format!(
            "failed to write staged archive {}: {err}",
            archive_path.display()
        )
    })?;
    let result = validate_7z_paths(executable.as_str(), archive_path.as_path()).and_then(|()| {
        let output = Command::new(executable.as_str())
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", target_dir.display()))
            .arg(archive_path.as_os_str())
            .output()
            .map_err(|err| format!("failed to start 7z extraction: {err}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(render_7z_failure(
                "failed to extract skill archive with 7z",
                &output,
            ))
        }
    });
    let _ = fs::remove_file(&archive_path);
    result
}

fn validate_7z_paths(executable: &str, archive_path: &Path) -> Result<(), String> {
    let output = Command::new(executable)
        .arg("l")
        .arg("-slt")
        .arg(archive_path.as_os_str())
        .output()
        .map_err(|err| format!("failed to inspect skill archive with 7z: {err}"))?;
    if !output.status.success() {
        return Err(render_7z_failure(
            "failed to inspect skill archive with 7z",
            &output,
        ));
    }

    let listing = String::from_utf8_lossy(&output.stdout);
    let mut in_entries = false;
    for line in listing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("----------") {
            in_entries = true;
            continue;
        }
        if !in_entries {
            continue;
        }
        let Some(raw_path) = trimmed.strip_prefix("Path = ") else {
            continue;
        };
        let _ = sanitize_upload_relative_path(raw_path)?;
    }
    Ok(())
}

fn render_7z_failure(prefix: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        format!("{prefix}: {stderr}")
    } else if !stdout.is_empty() {
        format!("{prefix}: {stdout}")
    } else {
        format!("{prefix}: exit status {}", output.status)
    }
}

fn find_7z_executable() -> Option<String> {
    ["7z", "7za", "7zr"].iter().find_map(|candidate| {
        Command::new(candidate)
            .arg("--help")
            .output()
            .ok()
            .map(|_| (*candidate).to_string())
    })
}

fn find_skill_entry_path(root: &Path) -> Option<PathBuf> {
    let canonical = root.join(SKILL_ENTRY_FILE);
    if canonical.is_file() {
        return Some(canonical);
    }
    LEGACY_SKILL_ENTRY_FILES
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
}

fn sanitize_upload_relative_path(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_start_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err("uploaded file path should not be empty".to_string());
    }

    let mut output = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(segment) => output.push(segment),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("uploaded file path escapes the skill root".to_string());
            }
        }
    }
    if output.as_os_str().is_empty() {
        return Err("uploaded file path should not be empty".to_string());
    }
    Ok(output)
}

fn parse_bool_field(field: &MultipartField) -> Result<bool, String> {
    let text = String::from_utf8_lossy(field.data.as_slice());
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" | "" => Ok(false),
        _ => Err("skills/import received an invalid overwrite flag".to_string()),
    }
}

fn is_archive_filename(filename: &str) -> bool {
    let lowercase = filename.to_ascii_lowercase();
    lowercase.ends_with(".zip") || lowercase.ends_with(".rar") || lowercase.ends_with(".7z")
}

fn temp_stage_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "liteyuki-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

fn capability_workspace_root() -> PathBuf {
    std::env::var_os("LY_WORKSPACE_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .unwrap_or_else(workspace_root)
}

fn display_skill_path(path: &Path, workspace_root: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(workspace_root) {
        return relative.display().to_string().replace('\\', "/");
    }
    let liteyuki_root = crate::utils::config_path::resolve_liteyuki_root_dir();
    if let Ok(relative) = path.strip_prefix(&liteyuki_root) {
        return Path::new(".liteyuki")
            .join(relative)
            .display()
            .to_string()
            .replace('\\', "/");
    }
    path.display().to_string().replace('\\', "/")
}
