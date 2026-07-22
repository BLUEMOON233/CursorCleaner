use std::{path::PathBuf, time::SystemTime};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaState {
    Supported,
    Unsupported,
    Missing,
}

#[derive(Clone, Debug)]
pub struct SchemaProbe {
    pub state: SchemaState,
    pub search_version: Option<i64>,
    pub state_version: Option<i64>,
    pub diagnostics: Vec<String>,
}

impl SchemaProbe {
    pub fn supported(&self) -> bool {
        self.state == SchemaState::Supported
    }
}

#[derive(Clone, Debug)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    pub source: String,
    pub archived: bool,
    pub workspace: String,
    pub preview: String,
    pub logical_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct WorkspaceSummary {
    pub label: String,
    pub conversations: usize,
    pub archived: usize,
    pub latest_updated_at: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveFilter {
    All,
    Active,
    Archived,
}

impl ArchiveFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Active,
            Self::Active => Self::Archived,
            Self::Archived => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Active => "未归档",
            Self::Archived => "已归档",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CheckResult {
    pub label: String,
    pub state: CheckState,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckState {
    Passed,
    Warning,
    Failed,
}

#[derive(Clone, Debug)]
pub struct PreflightReport {
    pub checks: Vec<CheckResult>,
}

impl PreflightReport {
    pub fn can_continue(&self) -> bool {
        !self
            .checks
            .iter()
            .any(|check| check.state == CheckState::Failed)
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Impact {
    pub conversations: usize,
    pub fts_rows: usize,
    pub candidates: usize,
    pub state_rows: usize,
    pub headers: usize,
    pub transcript_dirs: usize,
    pub transcript_bytes: u64,
    pub unknown_keys: usize,
}

#[derive(Clone, Debug)]
pub struct DeletePlan {
    pub id: u64,
    pub created_at: SystemTime,
    pub conversation_ids: Vec<String>,
    pub owned_ids: Vec<String>,
    pub transcript_dirs: Vec<PathBuf>,
    pub impact: Impact,
    pub protected_paths: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ProgressSnapshot {
    pub stage: String,
    pub completed: usize,
    pub total: usize,
}

#[derive(Clone, Debug)]
pub struct Receipt {
    pub started_at: SystemTime,
    pub ended_at: SystemTime,
    pub deleted_conversations: usize,
    pub deleted_state_rows: usize,
    pub deleted_transcript_dirs: usize,
    pub verified: bool,
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
