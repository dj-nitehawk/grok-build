//! Stub Claude-import surface when feature `foreign-sessions` is off.
//!
//! Marker probes no-op (`false`) so MCP / hooks / skills / inspect keep
//! calling the same paths. Scan / apply / mark do not read or write.

use std::path::{Path, PathBuf};

use crate::util::config::McpServerConfig;
use xai_grok_workspace::permission::types::PermissionRule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Skill,
    Rule,
}

#[derive(Debug, Clone)]
pub enum ImportableItem {
    Permission(PermissionRule),
    EnvVar {
        key: String,
        value: String,
    },
    McpServer {
        name: String,
        config: Box<McpServerConfig>,
    },
    Hook {
        event: String,
        matcher: Option<String>,
        command: String,
        timeout: Option<u64>,
    },
    PathEntry {
        kind: PathKind,
        path: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ImportPlan {
    pub global_items: Vec<ImportableItem>,
    pub project_items: Vec<ImportableItem>,
}

impl ImportPlan {
    pub fn total_items(&self) -> usize {
        self.global_items.len() + self.project_items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.global_items.is_empty() && self.project_items.is_empty()
    }

    pub fn summary(&self, _cwd: &Path) -> String {
        "No Claude settings found to import.".to_string()
    }
}

#[derive(Debug, Default)]
pub struct ImportResult {
    pub global_count: usize,
    pub project_count: usize,
    pub modified_files: Vec<String>,
}

impl ImportResult {
    pub fn total(&self) -> usize {
        self.global_count + self.project_count
    }
}

#[must_use]
pub fn scan_importable_settings(_cwd: &Path) -> ImportPlan {
    ImportPlan::default()
}

pub fn apply_import(_plan: &ImportPlan, _cwd: &Path) -> anyhow::Result<ImportResult> {
    Ok(ImportResult::default())
}

pub fn mark_claude_imported() -> anyhow::Result<()> {
    Ok(())
}

#[must_use]
pub fn find_project_root(cwd: &Path) -> PathBuf {
    cwd.to_path_buf()
}

#[must_use]
pub(crate) fn is_claude_import_marked() -> bool {
    false
}

#[must_use]
pub(crate) fn is_claude_import_marked_with_log(_gate_name: &'static str) -> bool {
    false
}
