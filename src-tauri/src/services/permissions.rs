//! Desktop permission model V2 (Phase 9.0.4).
//!
//! Defines the permission policy that guardrails the Agent's automatic
//! surface — chiefly the Auto-mode diff auto-apply. The policy is persisted
//! per config dir and shared by IDE commands and the CLI.
//!
//! Semantics (aligned with `docs/agent_cli_design.md` §3.3):
//! - Presets `suggest` / `edit` keep diffs in the waiting-user review flow;
//!   only `auto` lets the Agent apply diffs without confirmation.
//! - Granular flags (`allow_create`, `allow_edit`, `allow_delete`,
//!   `allow_commands`, `allow_git`) constrain what the Agent may do when it
//!   is allowed to act. Command/git mutation enforcement lands with agent
//!   initiated command execution (CLI Phase CLI-4); the flags and the shared
//!   command matcher are provided here so both surfaces stay consistent.
//! - `deny_paths` blocks the Agent from touching matched workspace paths.
//! - Per-run caps bound how much an unattended run may apply.

use crate::services::workspace;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionPreset {
    Ask,
    Suggest,
    Auto,
}

impl Default for PermissionPreset {
    fn default() -> Self {
        PermissionPreset::Auto
    }
}

impl PermissionPreset {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionPreset::Ask => "ask",
            PermissionPreset::Suggest => "suggest",
            PermissionPreset::Auto => "auto",
        }
    }

    /// Parse a preset name. `edit` is accepted as an alias of `suggest` to
    /// stay compatible with the CLI design's `suggest|edit|auto` spelling.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ask" => Some(PermissionPreset::Ask),
            "suggest" | "edit" => Some(PermissionPreset::Suggest),
            "auto" => Some(PermissionPreset::Auto),
            _ => None,
        }
    }

    /// Only the `auto` preset lets the Agent apply diffs unattended.
    pub fn auto_apply_enabled(&self) -> bool {
        matches!(self, PermissionPreset::Auto)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PermissionPolicy {
    #[serde(default)]
    pub preset: PermissionPreset,
    #[serde(rename = "allowCreate", default = "default_true")]
    pub allow_create: bool,
    #[serde(rename = "allowEdit", default = "default_true")]
    pub allow_edit: bool,
    #[serde(rename = "allowDelete", default)]
    pub allow_delete: bool,
    #[serde(rename = "allowCommands", default)]
    pub allow_commands: bool,
    /// Git mutation permission (stage/unstage/commit/discard style actions).
    #[serde(rename = "allowGit", default = "default_true")]
    pub allow_git: bool,
    #[serde(rename = "allowedCommandPatterns", default)]
    pub allowed_command_patterns: Vec<String>,
    #[serde(rename = "denyPaths", default)]
    pub deny_paths: Vec<String>,
    #[serde(rename = "maxAutoAppliesPerRun", default = "default_max_auto_applies")]
    pub max_auto_applies_per_run: u32,
    #[serde(rename = "maxFilesPerAutoApply", default = "default_max_files")]
    pub max_files_per_auto_apply: u32,
}

fn default_true() -> bool {
    true
}

fn default_max_auto_applies() -> u32 {
    25
}

fn default_max_files() -> u32 {
    50
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        PermissionPolicy::default_auto()
    }
}

impl PermissionPolicy {
    /// The out-of-box policy. It preserves the desktop's historical
    /// Auto-mode behavior (unattended apply of create/edit diffs) while
    /// keeping deletes, commands, and git mutations behind explicit
    /// opt-in. Use [`PermissionPolicy::with_preset`] for the stricter
    /// ask/suggest starting points.
    pub fn default_auto() -> Self {
        Self {
            preset: PermissionPreset::Auto,
            allow_create: true,
            allow_edit: true,
            allow_delete: true,
            allow_commands: true,
            allow_git: true,
            allowed_command_patterns: Vec::new(),
            deny_paths: Vec::new(),
            max_auto_applies_per_run: default_max_auto_applies(),
            max_files_per_auto_apply: default_max_files(),
        }
    }

    /// Restrict the policy to the recommended defaults for a preset.
    /// The flag defaults mirror the frontend preset tables in
    /// `src/types/agent.ts`: `ask` blocks every mutation, `suggest` allows
    /// file creation/editing but nothing destructive, `auto` allows file
    /// mutations and delegates remaining risk to the granular flags.
    /// Command execution and git mutations always require an explicit
    /// opt-in per the CLI design ("--auto must explicitly opt into
    /// command execution").
    pub fn with_preset(mut self, preset: PermissionPreset) -> Self {
        self.preset = preset;
        match preset {
            PermissionPreset::Ask => {
                self.allow_create = false;
                self.allow_edit = false;
                self.allow_delete = false;
                self.allow_commands = false;
                self.allow_git = false;
            }
            PermissionPreset::Suggest => {
                self.allow_create = true;
                self.allow_edit = true;
                self.allow_delete = false;
                self.allow_commands = false;
                self.allow_git = false;
            }
            PermissionPreset::Auto => {
                self.allow_create = true;
                self.allow_edit = true;
                self.allow_delete = true;
                self.allow_commands = true;
                self.allow_git = true;
            }
        }
        self
    }

    pub fn load() -> PermissionPolicy {
        Self::load_from_dir(&workspace::config_dir())
    }

    pub fn load_from_dir(dir: &Path) -> PermissionPolicy {
        let file_path = dir.join("permissions.json");
        let content = match std::fs::read_to_string(&file_path) {
            Ok(content) => content,
            Err(_) => return PermissionPolicy::default(),
        };
        match serde_json::from_str::<PermissionPolicy>(&content) {
            Ok(mut policy) => {
                // Repair invalid values from hand-edited configs.
                if policy.max_auto_applies_per_run == 0 {
                    policy.max_auto_applies_per_run = default_max_auto_applies();
                }
                if policy.max_files_per_auto_apply == 0 {
                    policy.max_files_per_auto_apply = default_max_files();
                }
                policy
            }
            Err(_) => PermissionPolicy::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to_dir(&workspace::config_dir())
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("Create config dir: {}", e))?;
        let file_path = dir.join("permissions.json");
        let json =
            serde_json::to_string_pretty(self).map_err(|e| format!("Serialize policy: {}", e))?;
        std::fs::write(&file_path, json).map_err(|e| format!("Write policy: {}", e))
    }
}

/// Check a command string against `--allow-run` style patterns.
/// Patterns support exact match, `*` for all, and a trailing-`*` prefix
/// match; whitespace is normalized on both sides. Shared by the CLI and
/// the desktop permission model.
pub fn is_command_allowed(command: &str, allow_run: &[String]) -> bool {
    let command = normalize_command_pattern(command);
    allow_run.iter().any(|pattern| {
        let pattern = normalize_command_pattern(pattern);
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return command.starts_with(prefix.trim_end());
        }
        command == pattern
    })
}

pub fn normalize_command_pattern(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decide whether a workspace-relative path is blocked by `deny_paths`.
/// Supported pattern shapes: exact relative path, directory prefix
/// (`assets/` or `assets` matches everything under it), and a trailing
/// `*` wildcard. Matching is separator- and case-insensitive.
pub fn path_denied(relative_path: &str, deny_paths: &[String]) -> bool {
    let normalized = normalize_rel_path(relative_path);
    if normalized.is_empty() {
        return false;
    }
    deny_paths.iter().any(|pattern| {
        let pattern = normalize_rel_path(pattern);
        if pattern.is_empty() {
            return false;
        }
        let pattern = pattern.trim_end_matches('*').to_string();
        if pattern.ends_with('/') || pattern.ends_with('\\') || !pattern.contains('.') {
            let prefix = pattern.trim_end_matches(['/', '\\']);
            return prefix.is_empty()
                || normalized == prefix
                || normalized.starts_with(&format!("{}/", prefix));
        }
        if pattern.contains('*') {
            return match_wildcard(&normalized, &pattern);
        }
        normalized == pattern
    })
}

fn normalize_rel_path(value: &str) -> String {
    value
        .trim()
        .trim_start_matches(['/', '\\'])
        .replace('\\', "/")
        .to_lowercase()
}

fn match_wildcard(text: &str, pattern: &str) -> bool {
    // Only trailing wildcards are supported, e.g. `*.env` or `secrets/*`.
    match pattern.split_once('*') {
        Some((prefix, "")) => text.starts_with(prefix),
        Some((prefix, suffix)) if !suffix.contains('*') => {
            text.starts_with(prefix) && text.ends_with(suffix)
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PermissionViolation {
    #[serde(rename = "diffId")]
    pub diff_id: String,
    pub file: String,
    pub reason: String,
}

/// Evaluate whether an unattended (Auto mode) apply may proceed.
/// `already_applied` counts files this run has already auto-applied so the
/// per-run cap holds across multiple batches.
pub fn evaluate_auto_apply(
    policy: &PermissionPolicy,
    diffs: &[(String, String)],
    already_applied: u32,
    workspace_root: Option<&Path>,
) -> Result<(), Vec<PermissionViolation>> {
    let mut violations: Vec<PermissionViolation> = Vec::new();

    if !policy.preset.auto_apply_enabled() {
        violations.push(PermissionViolation {
            diff_id: String::new(),
            file: String::new(),
            reason: format!(
                "Permission preset '{}' does not allow unattended apply",
                policy.preset.as_str()
            ),
        });
        return Err(violations);
    }

    let batch_files = diffs.len() as u32;
    if batch_files > policy.max_files_per_auto_apply {
        violations.push(PermissionViolation {
            diff_id: String::new(),
            file: String::new(),
            reason: format!(
                "Batch has {} files which exceeds maxFilesPerAutoApply ({})",
                batch_files, policy.max_files_per_auto_apply
            ),
        });
    }
    if already_applied.saturating_add(batch_files) > policy.max_auto_applies_per_run {
        violations.push(PermissionViolation {
            diff_id: String::new(),
            file: String::new(),
            reason: format!(
                "Run already auto-applied {} files; cap maxAutoAppliesPerRun is {}",
                already_applied, policy.max_auto_applies_per_run
            ),
        });
    }

    for (diff_id, file) in diffs {
        if path_denied(file, &policy.deny_paths) {
            violations.push(PermissionViolation {
                diff_id: diff_id.clone(),
                file: file.clone(),
                reason: "Path is denied by the permission policy".to_string(),
            });
            continue;
        }

        match resolve_existing_target(workspace_root, file) {
            Some(true) => {
                if !policy.allow_edit {
                    violations.push(PermissionViolation {
                        diff_id: diff_id.clone(),
                        file: file.clone(),
                        reason: "Editing existing files is not allowed by the permission policy"
                            .to_string(),
                    });
                }
            }
            Some(false) => {
                if !policy.allow_create {
                    violations.push(PermissionViolation {
                        diff_id: diff_id.clone(),
                        file: file.clone(),
                        reason: "Creating new files is not allowed by the permission policy"
                            .to_string(),
                    });
                }
            }
            None => {
                violations.push(PermissionViolation {
                    diff_id: diff_id.clone(),
                    file: file.clone(),
                    reason: "Path is outside the workspace".to_string(),
                });
            }
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

/// Resolve a workspace-relative diff path and report whether the target
/// already exists. `None` means the path resolves outside the workspace.
/// New files in not-yet-existing directories are handled by walking up to
/// the closest existing ancestor before the boundary check.
fn resolve_existing_target(workspace_root: Option<&Path>, file: &str) -> Option<bool> {
    let candidate = match workspace_root {
        Some(root) => root.join(file),
        None => crate::services::workspace::resolve_for_write(file).ok()?,
    };
    let canonical_root = match workspace_root {
        Some(root) => root.canonicalize().ok()?,
        None => crate::services::workspace::load_workspace_path()
            .ok()
            .flatten()?
            .parse::<PathBuf>()
            .ok()?
            .canonicalize()
            .ok()?,
    };

    let mut ancestor: &Path = &candidate;
    let canonical_ancestor = loop {
        match ancestor.canonicalize() {
            Ok(path) => break path,
            Err(_) => ancestor = ancestor.parent()?,
        }
    };
    if !canonical_ancestor.starts_with(&canonical_root) {
        return None;
    }
    Some(candidate.exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace::env_test_guard;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_config_dir() -> PathBuf {
        std::env::temp_dir().join(format!("agent-ide-perm-test-{}", Uuid::new_v4()))
    }

    fn diff(id: &str, file: &str) -> (String, String) {
        (id.to_string(), file.to_string())
    }

    #[test]
    fn default_policy_preserves_auto_behavior() {
        let policy = PermissionPolicy::default();
        assert_eq!(policy.preset, PermissionPreset::Auto);
        assert!(policy.allow_create);
        assert!(policy.allow_edit);
        assert!(policy.allow_delete);
        assert!(policy.allow_commands);
        assert!(policy.allow_git);
        assert!(policy.preset.auto_apply_enabled());
    }

    #[test]
    fn ask_and_suggest_presets_block_unattended_apply() {
        for preset in [PermissionPreset::Ask, PermissionPreset::Suggest] {
            let policy = PermissionPolicy::default().with_preset(preset);
            assert!(!policy.preset.auto_apply_enabled());
            let result = evaluate_auto_apply(&policy, &[diff("d1", "a.ts")], 0, None);
            assert!(result.is_err());
        }
    }

    #[test]
    fn parse_accepts_cli_edit_alias() {
        assert_eq!(PermissionPreset::parse("ask"), Some(PermissionPreset::Ask));
        assert_eq!(PermissionPreset::parse("edit"), Some(PermissionPreset::Suggest));
        assert_eq!(PermissionPreset::parse("suggest"), Some(PermissionPreset::Suggest));
        assert_eq!(PermissionPreset::parse("auto"), Some(PermissionPreset::Auto));
        assert_eq!(PermissionPreset::parse("bogus"), None);
    }

    #[test]
    fn command_patterns_support_exact_prefix_and_trust_all() {
        let patterns = vec![
            "npm test".to_string(),
            "cargo *".to_string(),
            "*".to_string(),
        ];
        assert!(is_command_allowed("npm  test", &patterns));
        assert!(is_command_allowed("cargo build --release", &patterns));
        assert!(is_command_allowed("anything at all", &patterns));

        let restrictive = vec!["npm test".to_string()];
        assert!(is_command_allowed("npm test", &restrictive));
        assert!(!is_command_allowed("npm test --watch", &restrictive));
        assert!(!is_command_allowed("rm -rf /", &restrictive));
    }

    #[test]
    fn deny_paths_match_exact_directory_and_wildcard_shapes() {
        let deny = vec![
            ".env".to_string(),
            "secrets/".to_string(),
            "assets".to_string(),
            "*.pem".to_string(),
        ];
        assert!(path_denied(".env", &deny));
        assert!(path_denied(".ENV", &deny));
        assert!(path_denied("secrets/keys.json", &deny));
        assert!(path_denied("secrets", &deny));
        assert!(path_denied("assets/img/logo.png", &deny));
        assert!(path_denied("server.key.pem", &deny));
        assert!(!path_denied("src/main.ts", &deny));
        assert!(!path_denied("environment.ts", &deny));
        assert!(!path_denied("", &deny));
    }

    #[test]
    fn evaluate_blocks_disallowed_operations_and_denied_paths() {
        let root = temp_config_dir();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/existing.ts"), "x").unwrap();

        let mut policy = PermissionPolicy::default();
        policy.deny_paths = vec!["vendor".to_string()];

        let result = evaluate_auto_apply(
            &policy,
            &[diff("d1", "src/new.ts"), diff("d2", "vendor/lib.js")],
            0,
            Some(&root),
        );
        let violations = result.unwrap_err();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].diff_id, "d2");
        assert!(violations[0].reason.contains("denied"));

        policy.allow_create = false;
        let result = evaluate_auto_apply(
            &policy,
            &[diff("d1", "src/new.ts")],
            0,
            Some(&root),
        );
        let violations = result.unwrap_err();
        assert!(violations[0].reason.contains("Creating new files"));

        policy.allow_edit = false;
        let result = evaluate_auto_apply(
            &policy,
            &[diff("d3", "src/existing.ts")],
            0,
            Some(&root),
        );
        let violations = result.unwrap_err();
        assert!(violations[0].reason.contains("Editing existing files"));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn evaluate_enforces_per_run_and_batch_caps() {
        let root = temp_config_dir();
        std::fs::create_dir_all(&root).unwrap();

        let mut policy = PermissionPolicy::default();
        policy.max_files_per_auto_apply = 2;
        policy.max_auto_applies_per_run = 3;

        let batch = vec![diff("d1", "a.ts"), diff("d2", "b.ts"), diff("d3", "c.ts")];
        let violations = evaluate_auto_apply(&policy, &batch, 0, Some(&root)).unwrap_err();
        assert!(violations
            .iter()
            .any(|v| v.reason.contains("maxFilesPerAutoApply")));

        let small_batch = vec![diff("d1", "a.ts"), diff("d2", "b.ts")];
        assert!(evaluate_auto_apply(&policy, &small_batch, 1, Some(&root)).is_ok());
        let violations = evaluate_auto_apply(&policy, &small_batch, 2, Some(&root)).unwrap_err();
        assert!(violations
            .iter()
            .any(|v| v.reason.contains("maxAutoAppliesPerRun")));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn policy_round_trips_through_disk_and_repairs_invalid_caps() {
        let _guard = env_test_guard();
        let dir = temp_config_dir();

        let policy = PermissionPolicy::default()
            .with_preset(PermissionPreset::Suggest)
            ;
        policy.save_to_dir(&dir).unwrap();
        let loaded = PermissionPolicy::load_from_dir(&dir);
        assert_eq!(loaded.preset, PermissionPreset::Suggest);
        assert!(!loaded.allow_delete);

        let broken = serde_json::json!({
            "preset": "auto",
            "maxAutoAppliesPerRun": 0,
            "maxFilesPerAutoApply": 0
        });
        std::fs::write(
            dir.join("permissions.json"),
            serde_json::to_string(&broken).unwrap(),
        )
        .unwrap();
        let repaired = PermissionPolicy::load_from_dir(&dir);
        assert_eq!(repaired.max_auto_applies_per_run, default_max_auto_applies());
        assert_eq!(repaired.max_files_per_auto_apply, default_max_files());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resolve_existing_target_classifies_create_and_edit() {
        let _guard = env_test_guard();
        let root = temp_config_dir();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/existing.ts"), "x").unwrap();

        assert_eq!(
            resolve_existing_target(Some(&root), "src/existing.ts"),
            Some(true)
        );
        assert_eq!(
            resolve_existing_target(Some(&root), "src/missing.ts"),
            Some(false)
        );
        // Outside the workspace root is rejected.
        assert_eq!(resolve_existing_target(Some(&root), "../outside.ts"), None);

        std::fs::remove_dir_all(&root).ok();
    }
}
