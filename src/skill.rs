#![allow(dead_code)]

use anyhow::Result;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(test))]
use std::sync::OnceLock;

pub const SKILL_RECALL_SIMILARITY_THRESHOLD: f32 = 0.42;
pub const SKILL_RECALL_MAX_RESULTS: usize = 1;
const SKILL_RECALL_MIN_KEYWORD_OVERLAP: usize = 2;
const SKILL_RECALL_MIN_TERM_LEN: usize = 4;

/// A skill definition from SKILL.md
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub allowed_tools: Option<Vec<String>>,
    pub content: String,
    pub path: PathBuf,
    search_text: String,
}

#[derive(Debug, Clone)]
pub struct SkillRecallPrompt {
    pub skill_names: Vec<String>,
    pub prompt: String,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<String>,
}

/// Registry of available skills
#[derive(Debug, Default)]
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    /// Load a process-wide shared immutable snapshot of skills for startup paths
    /// that only need read access.
    pub fn shared_snapshot() -> Arc<Self> {
        #[cfg(test)]
        {
            Arc::new(Self::load().unwrap_or_default())
        }

        #[cfg(not(test))]
        {
            static SHARED: OnceLock<Arc<SkillRegistry>> = OnceLock::new();
            SHARED
                .get_or_init(|| Arc::new(SkillRegistry::load().unwrap_or_default()))
                .clone()
        }
    }

    /// Import skills from Claude Code and Codex CLI on first run.
    /// Only runs if ~/.jcode/skills/ doesn't exist yet.
    fn import_from_external() {
        let jcode_skills = match crate::storage::jcode_dir() {
            Ok(dir) => dir.join("skills"),
            Err(_) => return,
        };

        if jcode_skills.exists() {
            return; // Not first run
        }

        let mut sources = Vec::new();
        let mut copied = Vec::new();

        // Import from Claude Code (~/.claude/skills/)
        if let Ok(claude_skills) = crate::storage::user_home_path(".claude/skills") {
            if claude_skills.is_dir() {
                let count = Self::copy_skills_dir(&claude_skills, &jcode_skills);
                if count > 0 {
                    sources.push(format!("{} from Claude Code", count));
                    copied.extend(Self::list_skill_names(&jcode_skills));
                }
            }
        }

        // Import from Codex CLI (~/.codex/skills/)
        if let Ok(codex_skills) = crate::storage::user_home_path(".codex/skills") {
            if codex_skills.is_dir() {
                let count = Self::copy_skills_dir(&codex_skills, &jcode_skills);
                if count > 0 {
                    sources.push(format!("{} from Codex CLI", count));
                    copied.extend(Self::list_skill_names(&jcode_skills));
                }
            }
        }

        if !sources.is_empty() {
            // Deduplicate names
            copied.sort();
            copied.dedup();
            crate::logging::info(&format!(
                "Skills: Imported {} ({}) from {}",
                copied.len(),
                copied.join(", "),
                sources.join(" + "),
            ));
        }
    }

    /// Copy skill directories from src to dst. Returns count of skills copied.
    fn copy_skills_dir(src: &Path, dst: &Path) -> usize {
        let entries = match std::fs::read_dir(src) {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Skip Codex system skills
            if name.starts_with('.') {
                continue;
            }

            // Only copy if SKILL.md exists
            if !path.join("SKILL.md").exists() {
                continue;
            }

            let dest = dst.join(&name);
            if let Err(e) = Self::copy_dir_recursive(&path, &dest) {
                crate::logging::error(&format!("Failed to copy skill '{}': {}", name, e));
                continue;
            }
            count += 1;
        }
        count
    }

    /// Recursively copy a directory
    fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                Self::copy_dir_recursive(&src_path, &dst_path)?;
            } else if src_path.is_symlink() {
                // Resolve symlink and copy the target
                let target = std::fs::read_link(&src_path)?;
                // Try to create symlink, fall back to copying the file
                if crate::platform::symlink_or_copy(&target, &dst_path).is_err() {
                    if let Ok(resolved) = std::fs::canonicalize(&src_path) {
                        std::fs::copy(&resolved, &dst_path)?;
                    }
                }
            } else {
                std::fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    /// List skill directory names
    fn list_skill_names(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .ok()
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().to_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Load skills from all standard locations
    pub fn load() -> Result<Self> {
        // First-run import from Claude Code / Codex CLI
        Self::import_from_external();

        let mut registry = Self::default();

        // Load from ~/.jcode/skills/ (jcode's own global skills)
        if let Ok(jcode_dir) = crate::storage::jcode_dir() {
            let jcode_skills = jcode_dir.join("skills");
            if jcode_skills.exists() {
                registry.load_from_dir(&jcode_skills)?;
            }
        }

        // Load from ./.jcode/skills/ (project-local jcode skills)
        let local_jcode = Path::new(".jcode").join("skills");
        if local_jcode.exists() {
            registry.load_from_dir(&local_jcode)?;
        }

        // Fallback: ./.claude/skills/ (project-local Claude skills for compatibility)
        let local_claude = Path::new(".claude").join("skills");
        if local_claude.exists() {
            registry.load_from_dir(&local_claude)?;
        }

        Ok(registry)
    }

    /// Load skills from a directory
    fn load_from_dir(&mut self, dir: &Path) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    if let Ok(skill) = Self::parse_skill(&skill_file) {
                        self.skills.insert(skill.name.clone(), skill);
                    }
                }
            }
        }

        Ok(())
    }

    /// Parse a SKILL.md file
    fn parse_skill(path: &Path) -> Result<Skill> {
        let content = std::fs::read_to_string(path)?;

        // Parse YAML frontmatter
        let (frontmatter, body) = Self::parse_frontmatter(&content)?;

        let SkillFrontmatter {
            name,
            description,
            allowed_tools,
        } = frontmatter;

        let allowed_tools =
            allowed_tools.map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
        let search_text = build_skill_search_text(&name, &description, &body);

        Ok(Skill {
            name,
            description,
            allowed_tools,
            content: body,
            path: path.to_path_buf(),
            search_text,
        })
    }

    /// Parse YAML frontmatter from markdown
    fn parse_frontmatter(content: &str) -> Result<(SkillFrontmatter, String)> {
        let content = content.trim();

        if !content.starts_with("---") {
            anyhow::bail!("Missing YAML frontmatter");
        }

        let rest = &content[3..];
        let end = rest
            .find("---")
            .ok_or_else(|| anyhow::anyhow!("Unclosed frontmatter"))?;

        let yaml = &rest[..end];
        let body = rest[end + 3..].trim().to_string();

        let frontmatter: SkillFrontmatter = serde_yaml::from_str(yaml)?;

        Ok((frontmatter, body))
    }

    /// Get a skill by name
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    /// List all available skills
    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    /// Reload a specific skill by name
    pub fn reload(&mut self, name: &str) -> Result<bool> {
        // Find the skill's path first
        let path = self.skills.get(name).map(|s| s.path.clone());

        if let Some(path) = path {
            if path.exists() {
                let skill = Self::parse_skill(&path)?;
                self.skills.insert(skill.name.clone(), skill);
                Ok(true)
            } else {
                // Skill file was deleted
                self.skills.remove(name);
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    /// Reload all skills from all locations
    pub fn reload_all(&mut self) -> Result<usize> {
        self.skills.clear();

        let mut count = 0;

        // Load from ~/.jcode/skills/ (jcode's own global skills)
        if let Ok(jcode_dir) = crate::storage::jcode_dir() {
            let jcode_skills = jcode_dir.join("skills");
            if jcode_skills.exists() {
                count += self.load_from_dir_count(&jcode_skills)?;
            }
        }

        // Load from ./.jcode/skills/ (project-local jcode skills)
        let local_jcode = Path::new(".jcode").join("skills");
        if local_jcode.exists() {
            count += self.load_from_dir_count(&local_jcode)?;
        }

        // Fallback: ./.claude/skills/ (project-local Claude skills for compatibility)
        let local_claude = Path::new(".claude").join("skills");
        if local_claude.exists() {
            count += self.load_from_dir_count(&local_claude)?;
        }

        Ok(count)
    }

    /// Load skills from a directory and return count
    fn load_from_dir_count(&mut self, dir: &Path) -> Result<usize> {
        if !dir.is_dir() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    if let Ok(skill) = Self::parse_skill(&skill_file) {
                        self.skills.insert(skill.name.clone(), skill);
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Check if a message is a skill invocation (starts with /)
    pub fn parse_invocation(input: &str) -> Option<&str> {
        let trimmed = input.trim();
        if trimmed.starts_with('/') && !trimmed.contains(' ') {
            Some(&trimmed[1..])
        } else {
            None
        }
    }

    pub fn relevant_prompt_for_messages(
        &self,
        messages: &[crate::message::Message],
    ) -> Option<SkillRecallPrompt> {
        let context = crate::memory::format_context_for_relevance(messages);
        self.relevant_prompt_for_context(&context)
    }

    pub fn relevant_prompt_for_context(&self, context: &str) -> Option<SkillRecallPrompt> {
        let context = context.trim();
        if context.is_empty() || self.skills.is_empty() {
            return None;
        }

        #[cfg(test)]
        let matches = self.find_relevant_by_keywords(context);

        #[cfg(not(test))]
        let matches = match self.find_relevant_by_embedding(context) {
            Ok(matches) if !matches.is_empty() => matches,
            Ok(_) => self.find_relevant_by_keywords(context),
            Err(error) => {
                crate::logging::info(&format!(
                    "Skill auto-recall embedding search unavailable, falling back to keywords: {}",
                    error
                ));
                self.find_relevant_by_keywords(context)
            }
        };

        if matches.is_empty() {
            return None;
        }

        let skill_names = matches
            .iter()
            .map(|(skill, _)| skill.name.clone())
            .collect();
        let prompt = format_auto_recalled_skill_prompt(&matches);

        Some(SkillRecallPrompt {
            skill_names,
            prompt,
        })
    }

    fn find_relevant_by_embedding(&self, context: &str) -> Result<Vec<(&Skill, f32)>> {
        let query_embedding = crate::embedding::embed(context)?;

        let mut candidates = Vec::new();
        for skill in self.skills.values() {
            if let Ok(embedding) = crate::embedding::embed(skill.search_text.as_str()) {
                candidates.push((skill, embedding));
            }
        }

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        let candidate_refs: Vec<&[f32]> = candidates
            .iter()
            .map(|(_, embedding)| embedding.as_slice())
            .collect();
        let scores = crate::embedding::batch_cosine_similarity(&query_embedding, &candidate_refs);

        let mut scored: Vec<_> = candidates
            .into_iter()
            .zip(scores)
            .filter_map(|((skill, _), score)| {
                (score >= SKILL_RECALL_SIMILARITY_THRESHOLD).then_some((skill, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(SKILL_RECALL_MAX_RESULTS);
        Ok(scored)
    }

    fn find_relevant_by_keywords(&self, context: &str) -> Vec<(&Skill, f32)> {
        let normalized_context = normalize_skill_search_text(context);
        if normalized_context.is_empty() {
            return Vec::new();
        }

        let query_terms: HashSet<&str> = normalized_context
            .split_whitespace()
            .filter(|term| is_meaningful_skill_term(term))
            .collect();
        let mut scored: Vec<_> = self
            .skills
            .values()
            .filter_map(|skill| {
                let skill_terms: HashSet<&str> = skill
                    .search_text
                    .split_whitespace()
                    .filter(|term| is_meaningful_skill_term(term))
                    .collect();
                let overlap = query_terms
                    .iter()
                    .filter(|term| skill_terms.contains(**term))
                    .count();
                let normalized_name = normalize_skill_search_text(&skill.name);
                let exact_name_match = !normalized_name.is_empty()
                    && normalized_context.contains(normalized_name.as_str());
                if !exact_name_match && overlap < SKILL_RECALL_MIN_KEYWORD_OVERLAP {
                    return None;
                }
                let mut score = overlap as f32;
                if exact_name_match {
                    score += 3.0;
                }
                Some((skill, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored.truncate(SKILL_RECALL_MAX_RESULTS);
        scored
    }
}

impl Skill {
    /// Get the full prompt content for this skill
    pub fn get_prompt(&self) -> String {
        format!(
            "# Skill: {}\n\n{}\n\n{}",
            self.name, self.description, self.content
        )
    }

    /// Load additional files from the skill directory
    pub fn load_file(&self, filename: &str) -> Result<String> {
        let skill_dir = self
            .path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("No parent dir"))?;
        let file_path = skill_dir.join(filename);
        Ok(std::fs::read_to_string(file_path)?)
    }
}

fn build_skill_search_text(name: &str, description: &str, content: &str) -> String {
    normalize_skill_search_text(&format!("{}\n{}\n{}", name, description, content))
}

fn normalize_skill_search_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_meaningful_skill_term(term: &str) -> bool {
    term.len() >= SKILL_RECALL_MIN_TERM_LEN && !term.chars().all(|c| c.is_ascii_digit())
}

fn format_auto_recalled_skill_prompt(matches: &[(&Skill, f32)]) -> String {
    let mut output = String::from(
        "# Auto-Recalled Skill\n\nA skill matched the current conversation context. Use it if it helps complete the task.\n",
    );

    for (idx, (skill, _score)) in matches.iter().enumerate() {
        output.push_str(&format!(
            "\n\n## Suggested Skill {}: /{}\n\n{}",
            idx + 1,
            skill.name,
            skill.get_prompt()
        ));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_skill(name: &str, description: &str, content: &str) -> Skill {
        Skill {
            name: name.to_string(),
            description: description.to_string(),
            allowed_tools: None,
            content: content.to_string(),
            path: PathBuf::from(format!("/tmp/{name}/SKILL.md")),
            search_text: build_skill_search_text(name, description, content),
        }
    }

    #[test]
    fn relevant_prompt_for_context_keyword_matches_skill() {
        let mut registry = SkillRegistry::default();
        let skill = test_skill(
            "firefox-browser",
            "Control Firefox browser sessions and logged-in pages",
            "Use this skill when you need to open websites, click buttons, or interact with browser pages.",
        );
        registry.skills.insert(skill.name.clone(), skill);

        let recalled = registry
            .relevant_prompt_for_context("Open Firefox and click the login button on the website")
            .expect("expected a recalled skill");

        assert_eq!(recalled.skill_names, vec!["firefox-browser".to_string()]);
        assert!(recalled.prompt.contains("# Auto-Recalled Skill"));
        assert!(recalled.prompt.contains("/firefox-browser"));
    }

    #[test]
    fn relevant_prompt_for_context_returns_none_when_no_skill_matches() {
        let mut registry = SkillRegistry::default();
        let skill = test_skill(
            "firefox-browser",
            "Control Firefox browser sessions and logged-in pages",
            "Use this skill when you need to open websites, click buttons, or interact with browser pages.",
        );
        registry.skills.insert(skill.name.clone(), skill);

        let recalled = registry.relevant_prompt_for_context(
            "Refactor the Rust parser and improve error handling in the compiler pipeline",
        );

        assert!(recalled.is_none());
    }
}
