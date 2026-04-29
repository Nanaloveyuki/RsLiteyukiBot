use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

const SKILL_ENTRY_FILE: &str = "SKILL.md";
const LEGACY_SKILL_ENTRY_FILES: &[&str] = &["Skill.md", "skill.md"];
const DEFAULT_SKILL_READ_MAX_CHARS: usize = 16_000;
const SKILL_DIR_ENV: &str = "LY_SKILLS_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillInfo {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) root: PathBuf,
    pub(crate) entry_path: PathBuf,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkillCatalogEntry {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SkillManager {
    workspace_root: PathBuf,
    scan_roots: Vec<PathBuf>,
}

impl SkillManager {
    pub(crate) fn for_workspace(workspace_root: &Path) -> Self {
        let managed_root = std::env::var_os(SKILL_DIR_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(crate::utils::config_path::resolve_user_skills_dir);
        let legacy_root = workspace_root.join(crate::hardcode_data::config_path::SKILLS_DIR_NAME);
        let scan_roots = if std::env::var_os(SKILL_DIR_ENV).is_some() {
            vec![managed_root.clone()]
        } else if managed_root == legacy_root {
            vec![managed_root.clone()]
        } else {
            vec![managed_root.clone(), legacy_root]
        };
        Self::from_roots(workspace_root.to_path_buf(), managed_root, scan_roots)
    }

    fn from_roots(
        workspace_root: PathBuf,
        managed_root: PathBuf,
        scan_roots: Vec<PathBuf>,
    ) -> Self {
        let _ = managed_root;
        Self {
            workspace_root,
            scan_roots,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn managed_root(&self) -> &Path {
        self.scan_roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or(self.workspace_root.as_path())
    }

    pub(crate) fn list_skills(&self) -> Result<Vec<SkillInfo>, String> {
        let mut skills = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for root in &self.scan_roots {
            if !root.exists() {
                continue;
            }
            if !root.is_dir() {
                return Err(format!("skills root {} is not a directory", root.display()));
            }

            let entry_iter = fs::read_dir(root).map_err(|err| {
                format!("failed to read skills directory {}: {err}", root.display())
            })?;
            for entry in entry_iter {
                let entry = entry.map_err(|err| {
                    format!(
                        "failed to read skill directory entry under {}: {err}",
                        root.display()
                    )
                })?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let Some(entry_path) = find_skill_entry_path(path.as_path()) else {
                    continue;
                };

                let content = fs::read_to_string(&entry_path).map_err(|err| {
                    format!("failed to read skill file {}: {err}", entry_path.display())
                })?;
                let Some(name) = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                else {
                    continue;
                };
                if !seen.insert(name.clone()) {
                    continue;
                }

                skills.push(SkillInfo {
                    name,
                    description: extract_skill_description(content.as_str()),
                    root: path,
                    entry_path,
                });
            }
        }

        skills.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(skills)
    }

    pub(crate) fn build_inventory_prompt(&self, skills: &[SkillInfo]) -> Option<String> {
        if skills.is_empty() {
            return None;
        }

        let mut lines = Vec::with_capacity(skills.len() + 4);
        lines.push("Repo-local skills are task instructions, not executable tools.".to_string());
        lines.push(
            "If a skill matches the request, read its SKILL.md with `read_skill_document` before using it."
                .to_string(),
        );
        lines.push("Available skills:".to_string());
        for skill in skills {
            let path = display_path(skill.entry_path.as_path(), self.workspace_root.as_path());
            let description = skill
                .description
                .as_deref()
                .unwrap_or("No description provided");
            lines.push(format!("- {}: {} ({path})", skill.name, description));
        }
        Some(lines.join("\n"))
    }

    pub(crate) fn read_skill_document(
        &self,
        skill_name: &str,
        max_chars: Option<usize>,
    ) -> Result<String, String> {
        let normalized = skill_name.trim();
        if normalized.is_empty() {
            return Err("skill_name cannot be empty".to_string());
        }

        let skills = self.list_skills()?;
        let skill = skills
            .into_iter()
            .find(|skill| skill.name == normalized)
            .ok_or_else(|| format!("skill '{normalized}' was not found"))?;
        let content = fs::read_to_string(&skill.entry_path)
            .map_err(|err| format!("failed to read {}: {err}", skill.entry_path.display()))?;
        let limit = max_chars
            .unwrap_or(DEFAULT_SKILL_READ_MAX_CHARS)
            .clamp(256, 64_000);
        let (content, truncated) = truncate_string(content.as_str(), limit);
        let display = display_path(skill.entry_path.as_path(), self.workspace_root.as_path());

        let mut rendered = format!("Skill: {}\nPath: {}\n\n{}", skill.name, display, content);
        if truncated {
            rendered.push_str("\n\n[truncated]");
        }
        Ok(rendered)
    }

    #[allow(dead_code)]
    pub(crate) fn build_catalog(&self, skills: &[SkillInfo]) -> Vec<SkillCatalogEntry> {
        skills
            .iter()
            .map(|skill| SkillCatalogEntry {
                name: skill.name.clone(),
                description: skill.description.clone(),
                path: display_path(skill.entry_path.as_path(), self.workspace_root.as_path()),
            })
            .collect()
    }
}

fn extract_skill_description(content: &str) -> Option<String> {
    extract_skill_frontmatter_value(content, "description")
}

pub(crate) fn extract_skill_frontmatter_value(content: &str, target_key: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }

    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.trim() != target_key {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
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

fn display_path(path: &Path, workspace_root: &Path) -> String {
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

fn truncate_string(value: &str, max_chars: usize) -> (&str, bool) {
    if value.chars().count() <= max_chars {
        return (value, false);
    }

    let mut end = value.len();
    let mut count = 0;
    for (index, ch) in value.char_indices() {
        count += 1;
        if count > max_chars {
            end = index;
            break;
        }
        end = index + ch.len_utf8();
    }
    (&value[..end], true)
}

#[cfg(test)]
#[path = "skills/tests.rs"]
mod tests;
