use std::{collections::BTreeSet, path::Path};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    config::Config,
    domain::{
        ArchiveFilter, Conversation, DeletePlan, PreflightReport, ProgressSnapshot, Receipt,
        SchemaProbe, WorkspaceSummary,
    },
    error::AppError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    Home,
    SourceCheck,
    Workspaces,
    Records,
    Detail,
    Preflight,
    Planning,
    Preview,
    Confirm,
    Running,
    Receipt,
    Error,
    Help,
}

#[derive(Clone, Debug)]
pub enum Effect {
    ProbeAndLoad,
    RunPreflight(Vec<String>),
    BuildPlan(Vec<String>),
    Execute(DeletePlan),
}

pub struct App {
    pub config: Config,
    pub screen: Screen,
    pub previous_screen: Screen,
    pub home_selection: usize,
    pub workspace_cursor: usize,
    pub active_workspace: Option<String>,
    pub record_cursor: usize,
    pub detail_scroll: u16,
    pub probe: Option<SchemaProbe>,
    pub loading: bool,
    pub records: Vec<Conversation>,
    pub selected: BTreeSet<String>,
    pub query: String,
    pub searching: bool,
    pub archive_filter: ArchiveFilter,
    pub preflight: Option<PreflightReport>,
    pub pending_ids: Vec<String>,
    pub plan: Option<DeletePlan>,
    pub confirm_execute: bool,
    pub progress: Option<ProgressSnapshot>,
    pub receipt: Option<Receipt>,
    pub error: Option<(String, String, String)>,
    pub toast: Option<String>,
    pub should_quit: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            screen: Screen::Home,
            previous_screen: Screen::Home,
            home_selection: 0,
            workspace_cursor: 0,
            active_workspace: None,
            record_cursor: 0,
            detail_scroll: 0,
            probe: None,
            loading: true,
            records: Vec::new(),
            selected: BTreeSet::new(),
            query: String::new(),
            searching: false,
            archive_filter: ArchiveFilter::All,
            preflight: None,
            pending_ids: Vec::new(),
            plan: None,
            confirm_execute: false,
            progress: None,
            receipt: None,
            error: None,
            toast: None,
            should_quit: false,
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                self.active_workspace
                    .as_ref()
                    .is_none_or(|workspace| &record.workspace == workspace)
            })
            .filter(|(_, record)| match self.archive_filter {
                ArchiveFilter::All => true,
                ArchiveFilter::Active => !record.archived,
                ArchiveFilter::Archived => record.archived,
            })
            .filter(|(_, record)| {
                query.is_empty()
                    || record.title.to_lowercase().contains(&query)
                    || record.id.to_lowercase().contains(&query)
                    || record.workspace.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn workspace_groups(&self) -> Vec<WorkspaceSummary> {
        let mut groups = std::collections::BTreeMap::<String, WorkspaceSummary>::new();
        for record in &self.records {
            let group =
                groups
                    .entry(record.workspace.clone())
                    .or_insert_with(|| WorkspaceSummary {
                        label: record.workspace.clone(),
                        conversations: 0,
                        archived: 0,
                        latest_updated_at: 0,
                    });
            group.conversations += 1;
            group.archived += usize::from(record.archived);
            group.latest_updated_at = group.latest_updated_at.max(record.updated_at);
        }
        let mut groups = groups.into_values().collect::<Vec<_>>();
        groups.sort_by(|left, right| {
            let left_kind = usize::from(!Path::new(&left.label).is_absolute());
            let right_kind = usize::from(!Path::new(&right.label).is_absolute());
            left_kind
                .cmp(&right_kind)
                .then_with(|| right.latest_updated_at.cmp(&left.latest_updated_at))
                .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
        });
        groups
    }

    pub fn current_record(&self) -> Option<&Conversation> {
        self.filtered_indices()
            .get(self.record_cursor)
            .and_then(|index| self.records.get(*index))
    }

    pub fn set_loaded(&mut self, probe: SchemaProbe, records: Vec<Conversation>) {
        self.loading = false;
        self.probe = Some(probe);
        self.records = records;
        self.record_cursor = 0;
        let groups = self.workspace_groups();
        if self
            .active_workspace
            .as_ref()
            .is_some_and(|active| !groups.iter().any(|group| &group.label == active))
        {
            self.active_workspace = None;
        }
        self.workspace_cursor = self.workspace_cursor.min(groups.len().saturating_sub(1));
        self.selected
            .retain(|id| self.records.iter().any(|record| &record.id == id));
    }

    pub fn set_error(&mut self, error: AppError) {
        self.error = Some((
            error.to_string(),
            error.suggestion().into(),
            format!("{error:?}"),
        ));
        self.screen = Screen::Error;
        self.loading = false;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Effect> {
        self.toast = None;
        if self.searching {
            return self.key_search(key);
        }
        if key.code == KeyCode::Char('?') && self.screen != Screen::Help {
            self.previous_screen = self.screen;
            self.screen = Screen::Help;
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.screen == Screen::Running {
                self.toast =
                    Some("正在创建临时回滚快照或执行数据库事务，不能在中途强制退出。".into());
            } else {
                self.should_quit = true;
            }
            return None;
        }
        match self.screen {
            Screen::Home => self.key_home(key),
            Screen::SourceCheck => self.key_source(key),
            Screen::Workspaces => self.key_workspaces(key),
            Screen::Records => self.key_records(key),
            Screen::Detail => self.key_detail(key),
            Screen::Preflight => self.key_preflight(key),
            Screen::Planning | Screen::Running => None,
            Screen::Preview => self.key_preview(key),
            Screen::Confirm => self.key_confirm(key),
            Screen::Receipt => self.key_receipt(key),
            Screen::Error => self.key_error(key),
            Screen::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                    self.screen = self.previous_screen;
                }
                None
            }
        }
    }

    fn key_home(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => {
                self.home_selection = self.home_selection.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.home_selection = (self.home_selection + 1).min(2)
            }
            KeyCode::Char('r') => return Some(self.start_probe()),
            KeyCode::Enter => match self.home_selection {
                0 if self.probe.as_ref().is_some_and(SchemaProbe::supported) => {
                    self.active_workspace = None;
                    self.screen = Screen::Workspaces;
                }
                0 => self.screen = Screen::SourceCheck,
                1 => self.screen = Screen::SourceCheck,
                _ => {
                    self.previous_screen = Screen::Home;
                    self.screen = Screen::Help;
                }
            },
            _ => {}
        }
        None
    }

    fn key_source(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Home,
            KeyCode::Char('r') => return Some(self.start_probe()),
            KeyCode::Enter if self.probe.as_ref().is_some_and(SchemaProbe::supported) => {
                self.active_workspace = None;
                self.screen = Screen::Workspaces;
            }
            _ => {}
        }
        None
    }

    fn key_workspaces(&mut self, key: KeyEvent) -> Option<Effect> {
        let groups = self.workspace_groups();
        match key.code {
            KeyCode::Esc => self.screen = Screen::Home,
            KeyCode::Up | KeyCode::Char('k') => {
                self.workspace_cursor = self.workspace_cursor.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.workspace_cursor =
                    (self.workspace_cursor + 1).min(groups.len().saturating_sub(1))
            }
            KeyCode::PageUp => self.workspace_cursor = self.workspace_cursor.saturating_sub(10),
            KeyCode::PageDown => {
                self.workspace_cursor =
                    (self.workspace_cursor + 10).min(groups.len().saturating_sub(1))
            }
            KeyCode::Home => self.workspace_cursor = 0,
            KeyCode::End => self.workspace_cursor = groups.len().saturating_sub(1),
            KeyCode::Enter => {
                if let Some(group) = groups.get(self.workspace_cursor) {
                    self.active_workspace = Some(group.label.clone());
                    self.record_cursor = 0;
                    self.query.clear();
                    self.archive_filter = ArchiveFilter::All;
                    self.selected.clear();
                    self.screen = Screen::Records;
                }
            }
            KeyCode::Char('r') => return Some(self.start_probe()),
            _ => {}
        }
        None
    }

    fn key_records(&mut self, key: KeyEvent) -> Option<Effect> {
        let visible = self.filtered_indices();
        match key.code {
            KeyCode::Esc => {
                self.selected.clear();
                self.screen = Screen::Workspaces;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.record_cursor = self.record_cursor.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.record_cursor = (self.record_cursor + 1).min(visible.len().saturating_sub(1))
            }
            KeyCode::PageUp => self.record_cursor = self.record_cursor.saturating_sub(10),
            KeyCode::PageDown => {
                self.record_cursor = (self.record_cursor + 10).min(visible.len().saturating_sub(1))
            }
            KeyCode::Home => self.record_cursor = 0,
            KeyCode::End => self.record_cursor = visible.len().saturating_sub(1),
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Char('f') => {
                self.archive_filter = self.archive_filter.next();
                self.record_cursor = 0;
            }
            KeyCode::Char(' ') => {
                if let Some(id) = self.current_record().map(|record| record.id.clone())
                    && !self.selected.remove(&id)
                {
                    self.selected.insert(id);
                }
            }
            KeyCode::Enter if self.current_record().is_some() => {
                self.detail_scroll = 0;
                self.screen = Screen::Detail;
            }
            KeyCode::Char('x') | KeyCode::Delete => return self.start_delete_flow(),
            KeyCode::Char('r') => return Some(self.start_probe()),
            _ => {}
        }
        None
    }

    fn key_detail(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Records,
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.detail_scroll = self.detail_scroll.saturating_add(1)
            }
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(10),
            KeyCode::Char('x') | KeyCode::Delete => return self.start_delete_flow(),
            _ => {}
        }
        None
    }

    fn key_search(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.searching = false,
            KeyCode::Backspace => {
                self.query.pop();
                self.record_cursor = 0;
            }
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.query.push(character);
                self.record_cursor = 0;
            }
            _ => {}
        }
        None
    }

    fn key_preflight(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Records,
            KeyCode::Char('r') => {
                self.preflight = None;
                return Some(Effect::RunPreflight(self.pending_ids.clone()));
            }
            KeyCode::Enter
                if self
                    .preflight
                    .as_ref()
                    .is_some_and(PreflightReport::can_continue) =>
            {
                self.screen = Screen::Planning;
                return Some(Effect::BuildPlan(self.pending_ids.clone()));
            }
            _ => {}
        }
        None
    }

    fn key_preview(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Esc => self.screen = Screen::Records,
            KeyCode::Enter => {
                self.confirm_execute = false;
                self.screen = Screen::Confirm;
            }
            _ => {}
        }
        None
    }

    fn key_confirm(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.confirm_execute = false,
            KeyCode::Right | KeyCode::Char('l') => self.confirm_execute = true,
            KeyCode::Esc => self.screen = Screen::Preview,
            KeyCode::Enter if self.confirm_execute => {
                if let Some(plan) = self.plan.clone() {
                    self.start_running("执行前重新验证 schema、占用和计划…", 0, 3);
                    return Some(Effect::Execute(plan));
                }
            }
            KeyCode::Enter => self.screen = Screen::Preview,
            _ => {}
        }
        None
    }

    fn key_receipt(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => self.screen = Screen::Home,
            KeyCode::Char('q') => self.should_quit = true,
            _ => {}
        }
        None
    }

    fn key_error(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.screen = Screen::Home,
            KeyCode::Char('r') => return Some(self.start_probe()),
            _ => {}
        }
        None
    }

    fn start_delete_flow(&mut self) -> Option<Effect> {
        let mut ids: Vec<String> = self.selected.iter().cloned().collect();
        if ids.is_empty()
            && let Some(record) = self.current_record()
        {
            ids.push(record.id.clone());
        }
        if ids.is_empty() {
            self.toast = Some("没有可清理的记录。".into());
            return None;
        }
        self.pending_ids = ids.clone();
        self.preflight = None;
        self.plan = None;
        self.screen = Screen::Preflight;
        Some(Effect::RunPreflight(ids))
    }

    fn start_probe(&mut self) -> Effect {
        self.loading = true;
        self.probe = None;
        Effect::ProbeAndLoad
    }

    fn start_running(&mut self, stage: &str, completed: usize, total: usize) {
        self.progress = Some(ProgressSnapshot {
            stage: stage.into(),
            completed,
            total,
        });
        self.screen = Screen::Running;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_confirmation_defaults_to_cancel() {
        let mut app = App::new(Config::default());
        app.screen = Screen::Preview;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::Confirm);
        assert!(!app.confirm_execute);
    }

    #[test]
    fn search_matches_cjk_and_workspace() {
        let mut app = App::new(Config::default());
        app.records.push(Conversation {
            id: "id-1".into(),
            title: "修复终端".into(),
            updated_at: 0,
            source: "local".into(),
            archived: false,
            workspace: "/很长/中文/路径".into(),
            preview: String::new(),
            logical_bytes: 0,
        });
        app.query = "中文".into();
        assert_eq!(app.filtered_indices(), vec![0]);
    }

    #[test]
    fn groups_by_workspace_and_limits_record_list() {
        let mut app = App::new(Config::default());
        for (id, workspace, updated_at) in [
            ("id-a", "/项目/甲", 10),
            ("id-b", "/项目/乙", 30),
            ("id-c", "/项目/甲", 20),
            ("id-d", "无法识别工作目录", 40),
        ] {
            app.records.push(Conversation {
                id: id.into(),
                title: id.into(),
                updated_at,
                source: "local".into(),
                archived: false,
                workspace: workspace.into(),
                preview: String::new(),
                logical_bytes: 0,
            });
        }
        let groups = app.workspace_groups();
        assert_eq!(groups[0].label, "/项目/乙");
        assert_eq!(groups[1].label, "/项目/甲");
        assert_eq!(groups[1].conversations, 2);
        assert_eq!(groups[2].label, "无法识别工作目录");

        app.active_workspace = Some("/项目/甲".into());
        assert_eq!(app.filtered_indices(), vec![0, 2]);
    }
}
