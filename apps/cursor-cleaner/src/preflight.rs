use std::{fs, time::SystemTime};

use crate::{
    config::Config,
    domain::{CheckResult, CheckState, PreflightReport},
    platform,
    store::CursorStore,
};

pub async fn run(config: &Config) -> PreflightReport {
    let mut checks = Vec::new();
    let store = CursorStore::new(config.clone());
    match store.probe() {
        Ok(probe) if probe.supported() => {
            let detail = match probe.search_version {
                Some(version) => format!(
                    "conversation-search.db v{version} / state.vscdb v{}",
                    probe.state_version.unwrap_or_default()
                ),
                None => format!(
                    "state.vscdb v{}（单库模式）",
                    probe.state_version.unwrap_or_default()
                ),
            };
            checks.push(passed("Schema", detail));
        }
        Ok(probe) => checks.push(failed("Schema", probe.diagnostics.join("；"))),
        Err(error) => checks.push(failed("Schema", error.to_string())),
    }

    match store.integrity_check() {
        Ok(()) => checks.push(passed("数据库", "两份数据库 quick_check 均通过")),
        Err(error) => checks.push(failed("数据库", error.to_string())),
    }

    if config.projects_root.is_dir() {
        checks.push(passed("Transcript", "Cursor transcript 根目录可访问"));
    } else {
        checks.push(warning(
            "Transcript",
            "未发现 transcript 根目录；数据库清理仍可执行",
        ));
    }

    match platform::cursor_processes().await {
        Ok(processes) if processes.is_empty() => {
            checks.push(passed("Cursor 进程", "未检测到 Cursor 后台进程"));
        }
        Ok(processes) => checks.push(failed(
            "Cursor 进程",
            format!("仍在运行：{}", processes.join("、")),
        )),
        Err(detail) => checks.push(failed("Cursor 进程", detail)),
    }

    for database in [&config.search_db, &config.state_db]
        .into_iter()
        .filter(|path| path.is_file())
    {
        match platform::database_holders(database).await {
            Ok(holders) if holders.is_empty() => checks.push(passed(
                "数据库占用",
                format!(
                    "{} 未被其他进程打开",
                    database.file_name().unwrap_or_default().to_string_lossy()
                ),
            )),
            Ok(holders) => checks.push(failed(
                "数据库占用",
                format!(
                    "{} 仍被 {} 打开",
                    database.file_name().unwrap_or_default().to_string_lossy(),
                    holders.join("、")
                ),
            )),
            Err(detail) => checks.push(failed("数据库占用", detail)),
        }
    }

    match recovery_writable(config) {
        Ok(()) => checks.push(passed("临时回滚", "可创建并清理唯一测试文件")),
        Err(detail) => checks.push(failed("临时回滚", detail)),
    }

    PreflightReport { checks }
}

fn recovery_writable(config: &Config) -> Result<(), String> {
    fs::create_dir_all(&config.recovery_root).map_err(|error| error.to_string())?;
    if fs::symlink_metadata(&config.recovery_root)
        .is_ok_and(|metadata| platform::is_link_like(&metadata))
    {
        return Err("临时回滚根目录是符号链接".into());
    }
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let test = config.recovery_root.join(format!(
        ".cursor-cleaner-write-test-{}-{stamp}",
        std::process::id()
    ));
    fs::write(&test, b"preflight").map_err(|error| error.to_string())?;
    fs::remove_file(&test).map_err(|error| error.to_string())?;
    let _ = fs::remove_dir(&config.recovery_root);
    Ok(())
}

fn passed(label: impl Into<String>, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        label: label.into(),
        state: CheckState::Passed,
        detail: detail.into(),
    }
}

fn failed(label: impl Into<String>, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        label: label.into(),
        state: CheckState::Failed,
        detail: detail.into(),
    }
}

fn warning(label: impl Into<String>, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        label: label.into(),
        state: CheckState::Warning,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_rejects_failed_check() {
        let report = PreflightReport {
            checks: vec![CheckResult {
                label: "并发".into(),
                state: CheckState::Failed,
                detail: "占用".into(),
            }],
        };
        assert!(!report.can_continue());
    }
}
