use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params, types::ValueRef};
use serde::Serialize;
use serde_json::Value;

use crate::{
    config::Config,
    domain::{Conversation, DeletePlan, Impact, Receipt, SchemaProbe, SchemaState},
    error::AppError,
    platform,
};

const SEARCH_COLUMNS: &[&str] = &[
    "fts_rowid",
    "source",
    "scope",
    "id",
    "title",
    "updated_at",
    "is_archived",
    "root_fingerprint",
    "cache_fingerprint",
];
const HEADER_COLUMNS: &[&str] = &[
    "composerId",
    "workspaceId",
    "createdAt",
    "lastUpdatedAt",
    "isArchived",
    "isSubagent",
    "recency",
    "checkpointAt",
    "value",
];

pub struct CursorStore {
    config: Config,
}

impl CursorStore {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn probe(&self) -> Result<SchemaProbe, AppError> {
        if !self.config.state_db.is_file() {
            return Ok(SchemaProbe {
                state: SchemaState::Missing,
                search_version: None,
                state_version: None,
                diagnostics: vec![format!(
                    "缺少 state.vscdb：{}",
                    self.config.state_db.display()
                )],
            });
        }

        let state = open_readonly(&self.config.state_db)?;
        let state_version = state.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let mut problems = Vec::new();
        let mut diagnostics = Vec::new();
        if state_version != 1 {
            problems.push(format!(
                "state.vscdb user_version={state_version}，仅验证过 1"
            ));
        }
        check_columns(&state, "ItemTable", &["key", "value"], &mut problems)?;
        check_columns(&state, "cursorDiskKV", &["key", "value"], &mut problems)?;
        check_columns(&state, "composerHeaders", HEADER_COLUMNS, &mut problems)?;

        let search_version = if self.config.search_db.is_file() {
            let search = open_readonly(&self.config.search_db)?;
            let version = search.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            if version != 7 {
                problems.push(format!(
                    "conversation-search.db user_version={version}，仅验证过 7"
                ));
            }
            check_columns(&search, "conversations", SEARCH_COLUMNS, &mut problems)?;
            check_columns(
                &search,
                "conversation_search_candidates",
                &["id", "updated_at"],
                &mut problems,
            )?;
            let fts_sql: Option<String> = search
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='table' AND name='conversation_fts'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if !fts_sql.is_some_and(|sql| sql.to_ascii_lowercase().contains("fts5")) {
                problems.push("conversation_fts 不是已验证的 FTS5 表".into());
            }
            Some(version)
        } else {
            diagnostics
                .push("未发现 conversation-search.db；使用已验证的 state.vscdb 单库模式".into());
            None
        };
        let supported = problems.is_empty();
        diagnostics.extend(problems);

        Ok(SchemaProbe {
            state: if supported {
                SchemaState::Supported
            } else {
                SchemaState::Unsupported
            },
            search_version,
            state_version: Some(state_version),
            diagnostics,
        })
    }

    pub fn load_conversations(&self) -> Result<Vec<Conversation>, AppError> {
        self.require_supported()?;
        if self.config.search_db.is_file() {
            self.load_search_conversations()
        } else {
            self.load_state_conversations()
        }
    }

    fn load_search_conversations(&self) -> Result<Vec<Conversation>, AppError> {
        let search = open_readonly(&self.config.search_db)?;
        let state = open_readonly(&self.config.state_db)?;
        let workspace_paths = workspace_paths(&self.config.workspace_storage);
        let mut statement = search.prepare(
            "SELECT id,title,updated_at,source,is_archived FROM conversations ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)? != 0,
            ))
        })?;
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for row in rows {
            let (id, title, updated_at, source, archived) = row?;
            if !seen.insert(id.clone()) {
                continue;
            }
            let raw: Option<Vec<u8>> = state
                .query_row(
                    "SELECT value FROM cursorDiskKV WHERE key=?1",
                    params![format!("composerData:{id}")],
                    |row| value_bytes(row.get_ref(0)?),
                )
                .optional()?;
            let payload = raw
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
            let header: Option<(Option<String>, Option<Vec<u8>>)> = state
                .query_row(
                    "SELECT workspaceId,value FROM composerHeaders WHERE composerId=?1",
                    params![id],
                    |row| Ok((row.get(0)?, optional_value_bytes(row.get_ref(1)?))),
                )
                .optional()?;
            let header_payload = header
                .as_ref()
                .and_then(|(_, raw)| raw.as_deref())
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
            let header_workspace_id = header.as_ref().and_then(|(id, _)| id.as_deref());
            let workspace = infer_workspace(
                &state,
                &id,
                payload.as_ref(),
                header_payload.as_ref(),
                header_workspace_id,
                &workspace_paths,
                &self.config.projects_root,
            )?;
            let preview = payload
                .as_ref()
                .map(|value| conversation_preview(value, self.config.max_preview_chars))
                .unwrap_or_default();
            result.push(Conversation {
                id,
                logical_bytes: raw.as_ref().map_or(0, |value| value.len() as u64)
                    + title.len() as u64,
                title,
                updated_at,
                source,
                archived,
                workspace,
                preview,
            });
        }
        Ok(result)
    }

    fn load_state_conversations(&self) -> Result<Vec<Conversation>, AppError> {
        let state = open_readonly(&self.config.state_db)?;
        let workspace_paths = workspace_paths(&self.config.workspace_storage);
        let mut statement = state.prepare(
            "SELECT composerId,workspaceId,COALESCE(lastUpdatedAt,createdAt,0),\
             COALESCE(isArchived,0),value FROM composerHeaders \
             WHERE COALESCE(isSubagent,0)=0 ORDER BY COALESCE(lastUpdatedAt,createdAt,0) DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)? != 0,
                optional_value_bytes(row.get_ref(4)?),
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, workspace_id, updated_at, archived, header_raw) = row?;
            if !valid_id(&id) {
                continue;
            }
            let raw: Option<Vec<u8>> = state
                .query_row(
                    "SELECT value FROM cursorDiskKV WHERE key=?1",
                    params![format!("composerData:{id}")],
                    |row| value_bytes(row.get_ref(0)?),
                )
                .optional()?;
            let payload = raw
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
            let header_payload = header_raw
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
            let workspace = infer_workspace(
                &state,
                &id,
                payload.as_ref(),
                header_payload.as_ref(),
                workspace_id.as_deref(),
                &workspace_paths,
                &self.config.projects_root,
            )?;
            let title = header_payload
                .as_ref()
                .and_then(conversation_title)
                .or_else(|| payload.as_ref().and_then(conversation_title))
                .unwrap_or_else(|| "未命名对话".into());
            let preview = payload
                .as_ref()
                .map(|value| conversation_preview(value, self.config.max_preview_chars))
                .unwrap_or_default();
            result.push(Conversation {
                id,
                logical_bytes: raw.as_ref().map_or(0, |value| value.len() as u64)
                    + header_raw.as_ref().map_or(0, |value| value.len() as u64),
                title,
                updated_at,
                source: "state.vscdb".into(),
                archived,
                workspace,
                preview,
            });
        }
        Ok(result)
    }

    pub fn integrity_check(&self) -> Result<(), AppError> {
        for (label, path) in [("state.vscdb", &self.config.state_db)].into_iter().chain(
            self.config
                .search_db
                .is_file()
                .then_some(("conversation-search.db", &self.config.search_db)),
        ) {
            let connection = open_readonly(path)?;
            let result: String =
                connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
            if result != "ok" {
                return Err(AppError::Preflight(format!(
                    "{label} 完整性检查失败：{}",
                    redact_detail(&result)
                )));
            }
        }
        Ok(())
    }

    pub fn build_delete_plan(&self, ids: &[String]) -> Result<DeletePlan, AppError> {
        self.require_supported()?;
        if ids.is_empty() {
            return Err(AppError::Planning("没有选择对话".into()));
        }
        let requested: BTreeSet<String> = ids
            .iter()
            .map(|id| {
                if valid_id(id) {
                    Ok(id.clone())
                } else {
                    Err(AppError::Planning("对话标识格式无效".into()))
                }
            })
            .collect::<Result<_, _>>()?;
        let search = self
            .config
            .search_db
            .is_file()
            .then(|| open_readonly(&self.config.search_db))
            .transpose()?;
        let state = open_readonly(&self.config.state_db)?;
        let owned = owned_ids(&state, &requested)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        requested.hash(&mut hasher);
        owned.hash(&mut hasher);
        let mut impact = Impact::default();
        let mut protected_paths = BTreeSet::new();

        if let Some(search) = &search {
            for id in &owned {
                hash_query(
                    search,
                    "SELECT fts_rowid,source,scope,id,title,updated_at,is_archived,root_fingerprint,cache_fingerprint FROM conversations WHERE id=?1 ORDER BY source,scope",
                    id,
                    &mut hasher,
                    &mut impact.conversations,
                )?;
                let mut fts = search.prepare(
                    "SELECT rowid,title,body FROM conversation_fts WHERE rowid IN (SELECT fts_rowid FROM conversations WHERE id=?1) ORDER BY rowid",
                )?;
                hash_rows(fts.query(params![id])?, &mut hasher, &mut impact.fts_rows)?;
                hash_query(
                    search,
                    "SELECT id,updated_at FROM conversation_search_candidates WHERE id=?1",
                    id,
                    &mut hasher,
                    &mut impact.candidates,
                )?;
            }
            if requested
                .iter()
                .any(|id| !conversation_exists(search, id).unwrap_or(false))
            {
                return Err(AppError::Planning("搜索库中有选中对话已经不存在".into()));
            }
        } else {
            impact.conversations = owned.len();
            "state-only".hash(&mut hasher);
        }

        let mut known_state_keys = Vec::new();
        let mut unknown = BTreeSet::new();
        for table in ["cursorDiskKV", "ItemTable"] {
            let sql = format!("SELECT key,value FROM {table} ORDER BY key");
            let mut statement = state.prepare(&sql)?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let key: String = row.get(0)?;
                let is_known = if table == "cursorDiskKV" {
                    known_cursor_key(&key, &owned)
                } else {
                    known_item_key(&key, &owned)
                };
                if is_known {
                    table.hash(&mut hasher);
                    key.hash(&mut hasher);
                    hash_value(row.get_ref(1)?, &mut hasher);
                    known_state_keys.push((table, key));
                    impact.state_rows += 1;
                } else if owned.iter().any(|id| key.contains(id)) {
                    unknown.insert(format!("{table}/{key}"));
                }
            }
        }
        impact.unknown_keys = unknown.len();

        for id in &owned {
            hash_query(
                &state,
                "SELECT composerId,workspaceId,createdAt,lastUpdatedAt,isArchived,isSubagent,recency,checkpointAt,value FROM composerHeaders WHERE composerId=?1",
                id,
                &mut hasher,
                &mut impact.headers,
            )?;
            if let Some(raw) = composer_value(&state, id)?
                && let Ok(payload) = serde_json::from_slice::<Value>(&raw)
                && let Some(path) = workspace_path(&payload)
            {
                protected_paths.insert(path);
            }
        }
        known_state_keys.hash(&mut hasher);

        let transcript_dirs = find_transcript_dirs(&self.config.projects_root, &owned)?;
        impact.transcript_dirs = transcript_dirs.len();
        for path in &transcript_dirs {
            path.hash(&mut hasher);
            impact.transcript_bytes += hash_directory(path, &mut hasher)?;
        }
        impact.hash(&mut hasher);
        let id = hasher.finish();
        Ok(DeletePlan {
            id,
            created_at: SystemTime::now(),
            conversation_ids: requested.into_iter().collect(),
            owned_ids: owned.into_iter().collect(),
            transcript_dirs,
            impact,
            protected_paths: protected_paths.into_iter().collect(),
        })
    }

    pub fn execute_delete(&self, approved: &DeletePlan) -> Result<Receipt, AppError> {
        let started_at = SystemTime::now();
        if approved.created_at.elapsed().unwrap_or_default() > Duration::from_secs(600) {
            return Err(AppError::PlanChanged);
        }
        self.require_supported()?;
        self.integrity_check()?;
        let fresh = self.build_delete_plan(&approved.conversation_ids)?;
        if fresh.id != approved.id || fresh.impact != approved.impact {
            return Err(AppError::PlanChanged);
        }

        let recovery = self.new_recovery_dir("delete")?;
        let copied = snapshot_transcripts(
            &self.config.projects_root,
            &fresh.transcript_dirs,
            &recovery.join("transcripts"),
        )?;
        let mut manifest = RecoveryManifest {
            kind: "delete".into(),
            plan_id: Some(fresh.id),
            created_unix: unix_now(),
            conversation_count: fresh.conversation_ids.len(),
            transcript_dirs: copied
                .iter()
                .map(|(source, stored)| RecoveryTranscript {
                    source: source.clone(),
                    stored: stored.clone(),
                })
                .collect(),
            status: "snapshot-complete".into(),
        };
        write_manifest(&recovery, &manifest)?;

        for path in &fresh.transcript_dirs {
            assert_safe_transcript(path, &self.config.projects_root, &fresh.owned_ids)?;
            if let Err(source) = fs::remove_dir_all(path) {
                return match restore_transcripts(&copied) {
                    Ok(()) => {
                        let _ = self.remove_recovery_dir(&recovery);
                        Err(AppError::Io {
                            path: path.clone(),
                            source,
                        })
                    }
                    Err(restore) => {
                        manifest.status = "rollback-failed".into();
                        let _ = write_manifest(&recovery, &manifest);
                        Err(AppError::Execution(format!(
                            "清理 transcript 失败：{source}；自动恢复失败：{restore}；应急恢复数据保留在 {}",
                            recovery.display()
                        )))
                    }
                };
            }
        }

        if let Err(error) = self.delete_rows_transaction(&fresh) {
            let restore_error = restore_transcripts(&copied).err();
            manifest.status = if restore_error.is_some() {
                "rollback-failed"
            } else {
                "rolled-back"
            }
            .into();
            let _ = write_manifest(&recovery, &manifest);
            return match restore_error {
                Some(restore) => Err(AppError::Execution(format!(
                    "数据库事务已回滚，但 transcript 恢复失败：{restore}；应急恢复数据保留在 {}",
                    recovery.display()
                ))),
                None => {
                    let _ = self.remove_recovery_dir(&recovery);
                    Err(error)
                }
            };
        }

        let verified = fresh
            .conversation_ids
            .iter()
            .all(|id| !self.conversation_exists(id).unwrap_or(true));
        if !verified {
            manifest.status = "verification-failed".into();
            write_manifest(&recovery, &manifest)?;
            return Err(AppError::Execution(format!(
                "执行后校验失败；应急恢复数据保留在 {}",
                recovery.display()
            )));
        }
        manifest.status = "complete".into();
        write_manifest(&recovery, &manifest)?;
        self.remove_recovery_dir(&recovery)?;
        Ok(Receipt {
            started_at,
            ended_at: SystemTime::now(),
            deleted_conversations: fresh.impact.conversations,
            deleted_state_rows: fresh.impact.state_rows + fresh.impact.headers,
            deleted_transcript_dirs: fresh.impact.transcript_dirs,
            verified,
        })
    }

    fn delete_rows_transaction(&self, plan: &DeletePlan) -> Result<(), AppError> {
        let state = Connection::open_with_flags(
            &self.config.state_db,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        state.busy_timeout(Duration::from_secs(1))?;
        let has_search = self.config.search_db.is_file();
        if has_search {
            state.execute(
                "ATTACH DATABASE ?1 AS search",
                params![self.config.search_db.to_string_lossy()],
            )?;
        }
        state.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            for id in &plan.owned_ids {
                let keys = cursor_keys_for_id(&state, id)?;
                for key in keys {
                    state.execute("DELETE FROM cursorDiskKV WHERE key=?1", params![key])?;
                }
                let keys = item_keys_for_id(&state, id)?;
                for key in keys {
                    state.execute("DELETE FROM ItemTable WHERE key=?1", params![key])?;
                }
                state.execute(
                    "DELETE FROM composerHeaders WHERE composerId=?1",
                    params![id],
                )?;

                if has_search {
                    let mut rowids = Vec::new();
                    let mut query = state.prepare(
                        "SELECT fts_rowid FROM search.conversations WHERE id=?1 ORDER BY fts_rowid",
                    )?;
                    for row in query.query_map(params![id], |row| row.get::<_, i64>(0))? {
                        rowids.push(row?);
                    }
                    drop(query);
                    for rowid in rowids {
                        state.execute(
                            "DELETE FROM search.conversation_fts WHERE rowid=?1",
                            params![rowid],
                        )?;
                    }
                    state.execute(
                        "DELETE FROM search.conversation_search_candidates WHERE id=?1",
                        params![id],
                    )?;
                    state.execute("DELETE FROM search.conversations WHERE id=?1", params![id])?;
                }
            }
            for id in &plan.conversation_ids {
                let remaining: i64 = if has_search {
                    state.query_row(
                        "SELECT count(*) FROM search.conversations WHERE id=?1",
                        params![id],
                        |row| row.get(0),
                    )?
                } else {
                    state.query_row(
                        "SELECT count(*) FROM composerHeaders WHERE composerId=?1",
                        params![id],
                        |row| row.get(0),
                    )?
                };
                if remaining != 0 {
                    return Err(AppError::Execution("事务内记录数量校验失败".into()));
                }
            }
            let state_ok: String =
                state.query_row("PRAGMA main.quick_check", [], |row| row.get(0))?;
            let search_ok = if has_search {
                state.query_row("PRAGMA search.quick_check", [], |row| {
                    row.get::<_, String>(0)
                })? == "ok"
            } else {
                true
            };
            if state_ok != "ok" || !search_ok {
                return Err(AppError::Execution("事务内数据库完整性校验失败".into()));
            }
            Ok(())
        })();
        match result {
            Ok(()) => state.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = state.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(())
    }

    fn conversation_exists(&self, id: &str) -> Result<bool, AppError> {
        if self.config.search_db.is_file() {
            let search = open_readonly(&self.config.search_db)?;
            Ok(conversation_exists(&search, id)?)
        } else {
            let state = open_readonly(&self.config.state_db)?;
            let count: i64 = state.query_row(
                "SELECT count(*) FROM composerHeaders WHERE composerId=?1",
                params![id],
                |row| row.get(0),
            )?;
            Ok(count != 0)
        }
    }

    fn require_supported(&self) -> Result<SchemaProbe, AppError> {
        let probe = self.probe()?;
        if !probe.supported() {
            return Err(AppError::UnsupportedSchema(probe.diagnostics.join("；")));
        }
        Ok(probe)
    }

    fn new_recovery_dir(&self, label: &str) -> Result<PathBuf, AppError> {
        fs::create_dir_all(&self.config.recovery_root).map_err(|source| AppError::Io {
            path: self.config.recovery_root.clone(),
            source,
        })?;
        if fs::symlink_metadata(&self.config.recovery_root)
            .is_ok_and(|metadata| platform::is_link_like(&metadata))
        {
            return Err(AppError::Execution("临时回滚目录不能是符号链接".into()));
        }
        for attempt in 0..100_u32 {
            let path = self.config.recovery_root.join(format!(
                "{}-{label}-{}-{attempt:02}",
                unix_now(),
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => return Err(AppError::Io { path, source }),
            }
        }
        Err(AppError::Execution("无法创建唯一临时回滚目录".into()))
    }

    fn remove_recovery_dir(&self, recovery: &Path) -> Result<(), AppError> {
        fs::remove_dir_all(recovery).map_err(|source| AppError::Io {
            path: recovery.to_path_buf(),
            source,
        })?;
        let _ = fs::remove_dir(&self.config.recovery_root);
        Ok(())
    }
}

fn open_readonly(path: &Path) -> Result<Connection, AppError> {
    if !path.is_file() {
        return Err(AppError::Probe(format!("找不到数据库：{}", path.display())));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(1))?;
    connection.execute_batch("PRAGMA query_only=ON")?;
    Ok(connection)
}

fn check_columns(
    connection: &Connection,
    table: &str,
    expected: &[&str],
    diagnostics: &mut Vec<String>,
) -> Result<(), AppError> {
    let sql = format!("PRAGMA table_info('{table}')");
    let mut statement = connection.prepare(&sql)?;
    let actual: Vec<String> = statement
        .query_map([], |row| row.get(1))?
        .collect::<Result<_, _>>()?;
    if actual != expected {
        diagnostics.push(format!(
            "{table} 列结构不匹配：期望 {}，实际 {}",
            expected.join(","),
            actual.join(",")
        ));
    }
    Ok(())
}

fn workspace_paths(root: &Path) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Ok(entries) = fs::read_dir(root) else {
        return result;
    };
    for entry in entries.flatten() {
        let path = entry.path().join("workspace.json");
        let Ok(raw) = fs::read(&path) else { continue };
        let Ok(value) = serde_json::from_slice::<Value>(&raw) else {
            continue;
        };
        if let Some(folder) = value.get("folder").and_then(Value::as_str)
            && let Some(path) = file_uri_path(folder)
        {
            result.insert(entry.file_name().to_string_lossy().into_owned(), path);
        }
    }
    result
}

fn workspace_from_payload(
    value: Option<&Value>,
    known: &HashMap<String, String>,
) -> Option<String> {
    let identifier = value?.get("workspaceIdentifier")?;
    if let Some(uri) = identifier.get("uri") {
        for key in ["fsPath", "path"] {
            if let Some(path) = uri.get(key).and_then(Value::as_str)
                && Path::new(path).is_absolute()
            {
                return Some(path.to_string());
            }
        }
    }
    identifier
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| known.get(id).cloned())
}

fn workspace_path(value: &Value) -> Option<String> {
    workspace_from_payload(Some(value), &HashMap::new())
}

fn infer_workspace(
    state: &Connection,
    conversation_id: &str,
    composer: Option<&Value>,
    header: Option<&Value>,
    header_workspace_id: Option<&str>,
    known: &HashMap<String, String>,
    projects_root: &Path,
) -> Result<String, AppError> {
    for payload in [composer, header].into_iter().flatten() {
        if let Some(path) = workspace_from_payload(Some(payload), known) {
            return Ok(path);
        }
        if let Some(path) = tracked_repo_workspace(payload) {
            return Ok(path);
        }
    }
    if let Some(id) = header_workspace_id {
        if id == "empty-window" {
            return Ok("未打开文件夹（empty-window）".into());
        }
        if let Some(path) = known.get(id) {
            return Ok(path.clone());
        }
    }
    let pattern = format!("cursor/glass.tabs.v2/%/{conversation_id}/state.json");
    let mut statement = state.prepare("SELECT key FROM ItemTable WHERE key LIKE ?1")?;
    let tab_keys = statement
        .query_map(params![pattern], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for key in tab_keys {
        let parts = key.split('/').collect::<Vec<_>>();
        if let Some(workspace_id) = parts.get(2) {
            if *workspace_id == "empty-window" {
                return Ok("未打开文件夹（empty-window）".into());
            }
            if let Some(path) = known.get(*workspace_id) {
                return Ok(path.clone());
            }
        }
    }

    for project_id in transcript_project_ids(projects_root, conversation_id) {
        let matching = known
            .values()
            .filter(|path| project_slug_matches(path, &project_id))
            .cloned()
            .collect::<BTreeSet<_>>();
        if matching.len() == 1 {
            return Ok(matching.into_iter().next().unwrap_or_default());
        }
        if project_id == "empty-window" {
            return Ok("未打开文件夹（empty-window）".into());
        }
        if project_id.starts_with("var-folders-") {
            return Ok("临时/历史工作区（路径已不可用）".into());
        }
    }
    Ok("无法识别工作目录".into())
}

fn tracked_repo_workspace(payload: &Value) -> Option<String> {
    let paths = payload
        .get("trackedGitRepos")?
        .as_array()?
        .iter()
        .filter_map(|value| value.get("repoPath").and_then(Value::as_str))
        .filter(|path| Path::new(path).is_absolute())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    (paths.len() == 1)
        .then(|| paths.into_iter().next())
        .flatten()
}

fn transcript_project_ids(projects_root: &Path, conversation_id: &str) -> Vec<String> {
    let Ok(projects) = fs::read_dir(projects_root) else {
        return Vec::new();
    };
    projects
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .join("agent-transcripts")
                .join(conversation_id)
                .is_dir()
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

fn project_slug_matches(path: &str, project_id: &str) -> bool {
    project_slug_matches_for(path, project_id, cfg!(target_os = "windows"))
}

fn project_slug_matches_for(path: &str, project_id: &str, case_insensitive: bool) -> bool {
    let base = path.trim_matches(['/', '\\']).replace(['/', '\\'], "-");
    let candidates = [base.clone(), base.replace(':', ""), base.replace(':', "-")];
    if case_insensitive {
        candidates
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(project_id))
    } else {
        candidates.iter().any(|candidate| candidate == project_id)
    }
}

fn file_uri_path(value: &str) -> Option<String> {
    file_uri_path_for(value, cfg!(target_os = "windows"))
}

fn file_uri_path_for(value: &str, windows: bool) -> Option<String> {
    let raw = value.strip_prefix("file://")?;
    let decoded = percent_decode(raw)?;
    if windows {
        if let Some(path) = decoded.strip_prefix('/')
            && path.as_bytes().get(1) == Some(&b':')
            && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        {
            return Some(path.replace('/', "\\"));
        }
        if !decoded.starts_with('/') && !decoded.is_empty() {
            return Some(format!("\\\\{}", decoded.replace('/', "\\")));
        }
        None
    } else {
        Path::new(&decoded).is_absolute().then_some(decoded)
    }
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes.get(index + 1..index + 3)?;
            let text = std::str::from_utf8(pair).ok()?;
            output.push(u8::from_str_radix(text, 16).ok()?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn conversation_title(value: &Value) -> Option<String> {
    ["title", "name", "composerTitle"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| title.chars().take(200).collect())
}

fn conversation_preview(value: &Value, max_chars: usize) -> String {
    fn visit(value: &Value, output: &mut Vec<String>) {
        if output.len() >= 4 {
            return;
        }
        match value {
            Value::Object(map) => {
                let role = map
                    .get("role")
                    .or_else(|| map.get("type"))
                    .and_then(Value::as_str)
                    .filter(|role| matches!(*role, "user" | "assistant"));
                if let Some(role) = role {
                    for key in ["text", "content", "message"] {
                        if let Some(text) = map.get(key).and_then(Value::as_str) {
                            output.push(format!("{role}: {}", text.replace('\n', " ")));
                            return;
                        }
                    }
                }
                for child in map.values() {
                    visit(child, output);
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, output);
                }
            }
            _ => {}
        }
    }
    let mut lines = Vec::new();
    visit(value, &mut lines);
    let text = lines.join("\n");
    text.chars().take(max_chars).collect()
}

fn owned_ids(
    state: &Connection,
    requested: &BTreeSet<String>,
) -> Result<BTreeSet<String>, AppError> {
    let mut owned = requested.clone();
    let mut queue: VecDeque<String> = requested.iter().cloned().collect();
    while let Some(id) = queue.pop_front() {
        let Some(raw) = composer_value(state, &id)? else {
            if requested.contains(&id) {
                return Err(AppError::Planning(format!(
                    "缺少精确正文状态 composerData:{id}"
                )));
            }
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&raw) else {
            return Err(AppError::Planning(format!(
                "composerData:{id} 不是有效 JSON"
            )));
        };
        for field in ["subComposerIds", "subagentComposerIds"] {
            if let Some(values) = value.get(field).and_then(Value::as_array) {
                for child in values
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|id| valid_id(id))
                {
                    if owned.insert(child.to_string()) {
                        queue.push_back(child.to_string());
                    }
                }
            }
        }
    }
    Ok(owned)
}

fn composer_value(state: &Connection, id: &str) -> Result<Option<Vec<u8>>, AppError> {
    Ok(state
        .query_row(
            "SELECT value FROM cursorDiskKV WHERE key=?1",
            params![format!("composerData:{id}")],
            |row| value_bytes(row.get_ref(0)?),
        )
        .optional()?)
}

fn value_bytes(value: ValueRef<'_>) -> Result<Vec<u8>, rusqlite::Error> {
    match value {
        ValueRef::Text(value) | ValueRef::Blob(value) => Ok(value.to_vec()),
        _ => Err(rusqlite::Error::InvalidColumnType(
            0,
            "value".into(),
            value.data_type(),
        )),
    }
}

fn optional_value_bytes(value: ValueRef<'_>) -> Option<Vec<u8>> {
    match value {
        ValueRef::Text(value) | ValueRef::Blob(value) => Some(value.to_vec()),
        _ => None,
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn conversation_exists(connection: &Connection, id: &str) -> Result<bool, rusqlite::Error> {
    let count: i64 = connection.query_row(
        "SELECT count(*) FROM conversations WHERE id=?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn hash_query(
    connection: &Connection,
    sql: &str,
    id: &str,
    hasher: &mut impl Hasher,
    count: &mut usize,
) -> Result<(), AppError> {
    let mut statement = connection.prepare(sql)?;
    hash_rows(statement.query(params![id])?, hasher, count)
}

fn hash_rows(
    mut rows: rusqlite::Rows<'_>,
    hasher: &mut impl Hasher,
    count: &mut usize,
) -> Result<(), AppError> {
    while let Some(row) = rows.next()? {
        *count += 1;
        for column in 0..row.as_ref().column_count() {
            hash_value(row.get_ref(column)?, hasher);
        }
    }
    Ok(())
}

fn hash_value(value: ValueRef<'_>, hasher: &mut impl Hasher) {
    match value {
        ValueRef::Null => 0_u8.hash(hasher),
        ValueRef::Integer(value) => value.hash(hasher),
        ValueRef::Real(value) => value.to_bits().hash(hasher),
        ValueRef::Text(value) | ValueRef::Blob(value) => value.hash(hasher),
    }
}

fn known_cursor_key(key: &str, ids: &BTreeSet<String>) -> bool {
    ids.iter().any(|id| {
        key == format!("composerData:{id}")
            || key == format!("composerVirtualRowHeights:{id}")
            || [
                "bubbleId:",
                "checkpointId:",
                "codeBlockPartialInlineDiffFates:",
                "ofsContent:",
            ]
            .iter()
            .any(|prefix| key.starts_with(&format!("{prefix}{id}:")))
    })
}

fn known_item_key(key: &str, ids: &BTreeSet<String>) -> bool {
    ids.iter().any(|id| {
        key == format!("glass/cursor.editorPanelVisibility.agent/{id}")
            || key == format!("cursor/glass.editorPanelFullscreen/{id}")
            || (key.starts_with("cursor/glass.tabs.v2/")
                && key.ends_with(&format!("/{id}/state.json")))
    })
}

fn cursor_keys_for_id(connection: &Connection, id: &str) -> Result<Vec<String>, AppError> {
    let ids = BTreeSet::from([id.to_string()]);
    table_keys(connection, "cursorDiskKV", |key| {
        known_cursor_key(key, &ids)
    })
}

fn item_keys_for_id(connection: &Connection, id: &str) -> Result<Vec<String>, AppError> {
    let ids = BTreeSet::from([id.to_string()]);
    table_keys(connection, "ItemTable", |key| known_item_key(key, &ids))
}

fn table_keys(
    connection: &Connection,
    table: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<Vec<String>, AppError> {
    let mut statement = connection.prepare(&format!("SELECT key FROM {table}"))?;
    let keys = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(keys.into_iter().filter(|key| predicate(key)).collect())
}

fn find_transcript_dirs(
    projects_root: &Path,
    ids: &BTreeSet<String>,
) -> Result<Vec<PathBuf>, AppError> {
    if !projects_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let projects = fs::read_dir(projects_root).map_err(|source| AppError::Io {
        path: projects_root.to_path_buf(),
        source,
    })?;
    for project in projects {
        let project = project.map_err(|source| AppError::Io {
            path: projects_root.to_path_buf(),
            source,
        })?;
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let transcripts = project.path().join("agent-transcripts");
        if fs::symlink_metadata(&transcripts)
            .is_ok_and(|metadata| platform::is_link_like(&metadata))
        {
            return Err(AppError::Planning(format!(
                "transcript 根目录是符号链接：{}",
                transcripts.display()
            )));
        }
        for id in ids {
            let path = transcripts.join(id);
            if path.is_dir() {
                assert_safe_transcript(
                    &path,
                    projects_root,
                    &ids.iter().cloned().collect::<Vec<_>>(),
                )?;
                result.push(path);
            }
        }
    }
    result.sort();
    result.dedup();
    Ok(result)
}

fn assert_safe_transcript(
    path: &Path,
    projects_root: &Path,
    allowed_ids: &[String],
) -> Result<(), AppError> {
    let root = fs::canonicalize(projects_root).map_err(|source| AppError::Io {
        path: projects_root.to_path_buf(),
        source,
    })?;
    let target = fs::canonicalize(path).map_err(|source| AppError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let relative = target
        .strip_prefix(&root)
        .map_err(|_| AppError::Execution("transcript 目标逃逸白名单".into()))?;
    let parts: Vec<_> = relative.components().collect();
    if parts.len() != 3
        || parts[1].as_os_str() != "agent-transcripts"
        || !allowed_ids
            .iter()
            .any(|id| parts[2].as_os_str() == std::ffi::OsStr::new(id))
    {
        return Err(AppError::Execution(format!(
            "transcript 路径结构不安全：{}",
            path.display()
        )));
    }
    ensure_no_symlink(path)?;
    Ok(())
}

fn ensure_no_symlink(path: &Path) -> Result<(), AppError> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| platform::is_link_like(&metadata)) {
        return Err(AppError::Execution(format!(
            "删除目标包含符号链接：{}",
            path.display()
        )));
    }
    for entry in fs::read_dir(path).map_err(|source| AppError::Io {
        path: path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| AppError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let kind = entry.file_type().map_err(|source| AppError::Io {
            path: entry.path(),
            source,
        })?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|source| AppError::Io {
            path: entry.path(),
            source,
        })?;
        if platform::is_link_like(&metadata) {
            return Err(AppError::Execution(format!(
                "删除目标包含符号链接：{}",
                entry.path().display()
            )));
        }
        if kind.is_dir() {
            ensure_no_symlink(&entry.path())?;
        }
    }
    Ok(())
}

fn hash_directory(path: &Path, hasher: &mut impl Hasher) -> Result<u64, AppError> {
    let mut total = 0;
    let mut entries = fs::read_dir(path)
        .map_err(|source| AppError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| AppError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        entry.file_name().hash(hasher);
        let metadata = entry.metadata().map_err(|source| AppError::Io {
            path: entry.path(),
            source,
        })?;
        metadata.len().hash(hasher);
        metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .hash(hasher);
        if metadata.is_dir() {
            total += hash_directory(&entry.path(), hasher)?;
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn snapshot_transcripts(
    projects_root: &Path,
    sources: &[PathBuf],
    destination_root: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, AppError> {
    let canonical_root = fs::canonicalize(projects_root).map_err(|source| AppError::Io {
        path: projects_root.to_path_buf(),
        source,
    })?;
    let mut result = Vec::new();
    for source in sources {
        let relative = fs::canonicalize(source)
            .map_err(|error| AppError::Planning(error.to_string()))?
            .strip_prefix(&canonical_root)
            .map_err(|_| AppError::Execution("transcript 快照目标逃逸白名单".into()))?
            .to_path_buf();
        let destination = destination_root.join(relative);
        copy_directory(source, &destination)?;
        result.push((source.clone(), destination));
    }
    Ok(result)
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), AppError> {
    ensure_no_symlink(source)?;
    fs::create_dir_all(destination).map_err(|source_error| AppError::Io {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    for entry in fs::read_dir(source).map_err(|source_error| AppError::Io {
        path: source.to_path_buf(),
        source: source_error,
    })? {
        let entry = entry.map_err(|source_error| AppError::Io {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(|source_error| AppError::Io {
            path: entry.path(),
            source: source_error,
        })?;
        if kind.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target).map_err(|source_error| AppError::Io {
                path: target,
                source: source_error,
            })?;
        } else {
            return Err(AppError::Execution(format!(
                "快照源包含不支持的文件类型：{}",
                entry.path().display()
            )));
        }
    }
    Ok(())
}

fn restore_transcripts(entries: &[(PathBuf, PathBuf)]) -> Result<(), AppError> {
    for (destination, source) in entries {
        if destination.exists() {
            continue;
        }
        copy_directory(source, destination)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct RecoveryManifest {
    kind: String,
    plan_id: Option<u64>,
    created_unix: u64,
    conversation_count: usize,
    transcript_dirs: Vec<RecoveryTranscript>,
    status: String,
}

#[derive(Serialize)]
struct RecoveryTranscript {
    source: PathBuf,
    stored: PathBuf,
}

fn write_manifest(path: &Path, manifest: &RecoveryManifest) -> Result<(), AppError> {
    let manifest_path = path.join("manifest.toml");
    let raw = toml::to_string_pretty(manifest)
        .map_err(|error| AppError::Execution(format!("无法序列化回滚清单：{error}")))?;
    fs::write(&manifest_path, raw).map_err(|source| AppError::Io {
        path: manifest_path,
        source,
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn redact_detail(value: &str) -> String {
    value.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        config: Config,
        conversation_id: String,
        protected_project: PathBuf,
        transcript: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "cursor-cleaner-test-{label}-{}-{stamp}",
                std::process::id()
            ));
            let cursor_user = root.join("cursor-user");
            let projects_root = root.join("cursor-projects");
            let workspace_storage = cursor_user.join("workspaceStorage");
            let recovery_root = root.join("recovery");
            fs::create_dir_all(&workspace_storage).unwrap();
            fs::create_dir_all(&projects_root).unwrap();
            let config = Config {
                search_db: cursor_user.join("globalStorage/conversation-search.db"),
                state_db: cursor_user.join("globalStorage/state.vscdb"),
                workspace_storage,
                projects_root,
                recovery_root,
                max_preview_chars: 800,
            };
            fs::create_dir_all(config.search_db.parent().unwrap()).unwrap();
            let protected_project = root.join("真实项目-不要修改");
            fs::create_dir_all(&protected_project).unwrap();
            fs::write(protected_project.join("keep.txt"), b"keep").unwrap();
            let conversation_id = "11111111-2222-4333-8444-555555555555".to_string();
            create_search_db(&config.search_db, &conversation_id);
            create_state_db(&config.state_db, &conversation_id, &protected_project);
            let transcript = config
                .projects_root
                .join("project-cache/agent-transcripts")
                .join(&conversation_id);
            fs::create_dir_all(&transcript).unwrap();
            fs::write(transcript.join("record.jsonl"), b"{\"type\":\"test\"}\n").unwrap();
            Self {
                root,
                config,
                conversation_id,
                protected_project,
                transcript,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn create_search_db(path: &Path, id: &str) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "
                PRAGMA user_version=7;
                CREATE TABLE conversations (
                    fts_rowid INTEGER PRIMARY KEY,
                    source TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    updated_at INTEGER NOT NULL,
                    is_archived INTEGER NOT NULL,
                    root_fingerprint TEXT,
                    cache_fingerprint TEXT
                );
                CREATE VIRTUAL TABLE conversation_fts USING fts5(title, body);
                CREATE TABLE conversation_search_candidates (
                    id TEXT PRIMARY KEY,
                    updated_at INTEGER NOT NULL
                );
                ",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO conversations VALUES (1,'local','',?1,'测试对话',1721000000000,0,'root',NULL)",
                params![id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO conversation_fts(rowid,title,body) VALUES (1,'测试对话','隐私正文')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO conversation_search_candidates VALUES (?1,1721000000000)",
                params![id],
            )
            .unwrap();
    }

    fn create_state_db(path: &Path, id: &str, project: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "
                PRAGMA user_version=1;
                CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);
                CREATE TABLE cursorDiskKV (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);
                CREATE TABLE composerHeaders (
                    composerId TEXT PRIMARY KEY,
                    workspaceId TEXT,
                    createdAt INTEGER,
                    lastUpdatedAt INTEGER,
                    isArchived INTEGER,
                    isSubagent INTEGER,
                    recency INTEGER,
                    checkpointAt INTEGER,
                    value TEXT
                );
                ",
            )
            .unwrap();
        let payload = serde_json::json!({
            "composerId": id,
            "workspaceIdentifier": {
                "id": "workspace-id",
                "uri": {"fsPath": project.to_string_lossy()}
            },
            "messages": [
                {"role": "user", "text": "这段正文不得进入日志"},
                {"role": "assistant", "text": "仅在详情页预览"}
            ]
        })
        .to_string();
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES (?1,?2)",
                params![format!("composerData:{id}"), payload.as_bytes()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES (?1,?2)",
                params![format!("bubbleId:{id}:bubble"), b"message".as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES ('unrelated',x'6b656570')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO ItemTable VALUES (?1,x'74727565')",
                params![format!("glass/cursor.editorPanelVisibility.agent/{id}")],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO composerHeaders VALUES (?1,'workspace-id',1,2,0,0,2,NULL,?2)",
                params![id, payload],
            )
            .unwrap();
    }

    #[test]
    fn probes_supported_and_unknown_schema() {
        let fixture = Fixture::new("probe");
        let store = CursorStore::new(fixture.config.clone());
        assert!(store.probe().unwrap().supported());
        let connection = Connection::open(&fixture.config.search_db).unwrap();
        connection.execute_batch("PRAGMA user_version=99").unwrap();
        drop(connection);
        let probe = store.probe().unwrap();
        assert_eq!(probe.state, SchemaState::Unsupported);
        assert!(probe.diagnostics.iter().any(|value| value.contains("99")));
        assert!(fixture.transcript.is_dir());
    }

    #[test]
    fn deletes_only_cursor_copy_and_cleans_temporary_recovery_data() {
        let fixture = Fixture::new("delete");
        let store = CursorStore::new(fixture.config.clone());
        let records = store.load_conversations().unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].preview.contains("user:"));
        let plan = store
            .build_delete_plan(std::slice::from_ref(&fixture.conversation_id))
            .unwrap();
        assert_eq!(plan.impact.conversations, 1);
        assert_eq!(plan.impact.fts_rows, 1);
        assert_eq!(plan.impact.transcript_dirs, 1);

        let receipt = store.execute_delete(&plan).unwrap();
        assert!(receipt.verified);
        assert!(!fixture.transcript.exists());
        assert_eq!(
            fs::read_to_string(fixture.protected_project.join("keep.txt")).unwrap(),
            "keep"
        );
        let search = Connection::open(&fixture.config.search_db).unwrap();
        let remaining: i64 = search
            .query_row("SELECT count(*) FROM conversations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
        assert!(!fixture.config.recovery_root.exists());
    }

    #[test]
    fn locked_database_refuses_and_restores_transcript() {
        let fixture = Fixture::new("locked");
        let store = CursorStore::new(fixture.config.clone());
        let plan = store
            .build_delete_plan(std::slice::from_ref(&fixture.conversation_id))
            .unwrap();
        let lock = Connection::open(&fixture.config.state_db).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();

        let error = store.execute_delete(&plan).unwrap_err();
        assert!(matches!(
            error,
            AppError::Database(_) | AppError::Execution(_)
        ));
        assert!(fixture.transcript.is_dir());
        let search = Connection::open(&fixture.config.search_db).unwrap();
        let remaining: i64 = search
            .query_row("SELECT count(*) FROM conversations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
        lock.execute_batch("ROLLBACK").unwrap();
        assert!(!fixture.config.recovery_root.exists());
    }

    #[test]
    fn state_only_layout_loads_plans_and_deletes_from_temporary_copy() {
        let fixture = Fixture::new("state-only");
        fs::remove_file(&fixture.config.search_db).unwrap();
        let store = CursorStore::new(fixture.config.clone());

        let probe = store.probe().unwrap();
        assert_eq!(probe.state, SchemaState::Supported);
        assert_eq!(probe.search_version, None);
        assert!(
            probe
                .diagnostics
                .iter()
                .any(|value| value.contains("单库模式"))
        );

        let records = store.load_conversations().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source, "state.vscdb");
        assert_eq!(
            records[0].workspace,
            fixture.protected_project.to_string_lossy()
        );

        let plan = store
            .build_delete_plan(std::slice::from_ref(&fixture.conversation_id))
            .unwrap();
        assert_eq!(plan.impact.conversations, 1);
        assert_eq!(plan.impact.fts_rows, 0);
        assert_eq!(plan.impact.candidates, 0);

        let receipt = store.execute_delete(&plan).unwrap();
        assert!(receipt.verified);
        assert!(!fixture.transcript.exists());
        assert!(!fixture.config.recovery_root.exists());
        let state = Connection::open(&fixture.config.state_db).unwrap();
        let remaining: i64 = state
            .query_row("SELECT count(*) FROM composerHeaders", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn parses_windows_file_uris_without_accepting_remote_uris() {
        assert_eq!(
            file_uri_path_for("file:///C%3A/Users/test/My%20Project", true),
            Some(r"C:\Users\test\My Project".into())
        );
        assert_eq!(
            file_uri_path_for("file:///C:/Users/test/project", true),
            Some(r"C:\Users\test\project".into())
        );
        assert_eq!(
            file_uri_path_for("file://server/share/project", true),
            Some(r"\\server\share\project".into())
        );
        assert_eq!(
            file_uri_path_for("vscode-remote://ssh-remote+host/project", true),
            None
        );
        assert!(project_slug_matches_for(
            r"C:\Users\Test\Project",
            "c-users-test-project",
            true
        ));
    }
}
