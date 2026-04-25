use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

const DEFAULT_SKILLS_DIR_NAME: &str = "skills";
const SKILL_ENTRY_FILE: &str = "SKILL.md";
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
    skills_root: PathBuf,
}

impl SkillManager {
    pub(crate) fn for_workspace(workspace_root: &Path) -> Self {
        let skills_root = std::env::var_os(SKILL_DIR_ENV)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace_root.join(DEFAULT_SKILLS_DIR_NAME));
        Self {
            workspace_root: workspace_root.to_path_buf(),
            skills_root,
        }
    }

    pub(crate) fn list_skills(&self) -> Result<Vec<SkillInfo>, String> {
        if !self.skills_root.exists() {
            return Ok(Vec::new());
        }
        if !self.skills_root.is_dir() {
            return Err(format!(
                "skills root {} is not a directory",
                self.skills_root.display()
            ));
        }

        let mut skills = Vec::new();
        for entry in fs::read_dir(&self.skills_root).map_err(|err| {
            format!(
                "failed to read skills directory {}: {err}",
                self.skills_root.display()
            )
        })? {
            let entry = entry.map_err(|err| {
                format!(
                    "failed to read skill directory entry under {}: {err}",
                    self.skills_root.display()
                )
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let entry_path = path.join(SKILL_ENTRY_FILE);
            if !entry_path.is_file() {
                continue;
            }

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

            skills.push(SkillInfo {
                name,
                description: extract_skill_description(content.as_str()),
                root: path,
                entry_path,
            });
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
        if key.trim() != "description" {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn display_path(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
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
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("liteyuki-skill-test-{label}-{unique}"))
    }

    #[test]
    fn extract_skill_description_reads_frontmatter() {
        let content = "---\ndescription: Example skill\n---\n# Skill";
        assert_eq!(
            extract_skill_description(content).as_deref(),
            Some("Example skill")
        );
    }

    #[test]
    fn extract_skill_description_handles_crlf_frontmatter() {
        let content = "---\r\ndescription: Windows skill\r\n---\r\n# Skill";
        assert_eq!(
            extract_skill_description(content).as_deref(),
            Some("Windows skill")
        );
    }

    #[test]
    fn list_skills_scans_repo_local_layout() {
        let root = temp_path("scan");
        let skill_root = root.join("skills").join("demo");
        fs::create_dir_all(&skill_root).expect("skill dir should be created");
        fs::write(
            skill_root.join("SKILL.md"),
            "---\ndescription: Demo skill\n---\n# Demo",
        )
        .expect("skill file should be written");

        let manager = SkillManager::for_workspace(root.as_path());
        let skills = manager.list_skills().expect("skills should scan");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "demo");
        assert_eq!(skills[0].description.as_deref(), Some("Demo skill"));

        let _ = fs::remove_file(skill_root.join("SKILL.md"));
        let _ = fs::remove_dir_all(root);
    }
}
