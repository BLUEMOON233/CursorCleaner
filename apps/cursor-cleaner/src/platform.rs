use std::{env, fs::Metadata, path::PathBuf};

#[cfg(any(target_os = "windows", test))]
use rusqlite::{Connection, OpenFlags};
#[cfg(any(target_os = "windows", test))]
use std::time::Duration;
use tokio::process::Command;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostPlatform {
    MacOs,
    Windows,
    Unsupported,
}

impl HostPlatform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Unsupported
        }
    }

    pub const fn supported(self) -> bool {
        matches!(self, Self::MacOs | Self::Windows)
    }
}

pub fn default_home() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(path) = env::var_os("USERPROFILE") {
            return PathBuf::from(path);
        }
        if let (Some(drive), Some(path)) = (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
            let mut home = PathBuf::from(drive);
            home.push(path);
            return home;
        }
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_cursor_user(home: &std::path::Path) -> PathBuf {
    let appdata = env::var_os("APPDATA").map(PathBuf::from);
    cursor_user_for(HostPlatform::current(), home, appdata.as_deref())
}

fn cursor_user_for(
    platform: HostPlatform,
    home: &std::path::Path,
    appdata: Option<&std::path::Path>,
) -> PathBuf {
    match platform {
        HostPlatform::MacOs => home.join("Library/Application Support/Cursor/User"),
        HostPlatform::Windows => appdata
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("Cursor/User"),
        HostPlatform::Unsupported => home.join(".config/Cursor/User"),
    }
}

pub async fn cursor_processes() -> Result<Vec<String>, String> {
    match HostPlatform::current() {
        HostPlatform::MacOs => macos_cursor_processes().await,
        HostPlatform::Windows => windows_cursor_processes().await,
        HostPlatform::Unsupported => Err("当前操作系统不受支持，无法确认 Cursor 进程状态".into()),
    }
}

pub async fn database_holders(path: &std::path::Path) -> Result<Vec<String>, String> {
    match HostPlatform::current() {
        HostPlatform::MacOs => macos_database_holders(path).await,
        HostPlatform::Windows => windows_database_lock(path),
        HostPlatform::Unsupported => Err("当前操作系统不受支持，无法确认数据库并发状态".into()),
    }
}

pub fn is_link_like(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(target_os = "windows"))]
    false
}

async fn macos_cursor_processes() -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("pgrep")
            .args(["-fl", "/Applications/Cursor.app/Contents/"])
            .output()
            .await
            .map_err(|error| format!("无法可靠执行 pgrep：{error}"))?;
        if !output.status.success() && output.status.code() != Some(1) {
            return Err("pgrep 返回异常状态，无法确认并发安全".into());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let pid = fields.next()?;
                let name = fields.next().unwrap_or("Cursor");
                Some(format!("{name} (PID {pid})"))
            })
            .collect())
    }
    #[cfg(not(target_os = "macos"))]
    Err("macOS 进程检查不可用".into())
}

async fn windows_cursor_processes() -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist.exe")
            .args(["/FI", "IMAGENAME eq Cursor.exe", "/FO", "CSV", "/NH"])
            .output()
            .await
            .map_err(|error| format!("无法可靠执行 tasklist.exe：{error}"))?;
        if !output.status.success() {
            return Err("tasklist.exe 返回异常状态，无法确认并发安全".into());
        }
        Ok(parse_windows_tasklist(&output.stdout))
    }
    #[cfg(not(target_os = "windows"))]
    Err("Windows 进程检查不可用".into())
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_tasklist(output: &[u8]) -> Vec<String> {
    output
        .split(|byte| *byte == b'\n')
        .filter(|line| contains_ascii_case_insensitive(line, b"cursor.exe"))
        .map(|line| {
            let pid = line
                .split(|byte| *byte == b',')
                .nth(1)
                .map(|field| {
                    field
                        .iter()
                        .copied()
                        .filter(u8::is_ascii_digit)
                        .map(char::from)
                        .collect::<String>()
                })
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "?".into());
            format!("Cursor.exe (PID {pid})")
        })
        .collect()
}

#[cfg(any(target_os = "windows", test))]
fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}

async fn macos_database_holders(path: &std::path::Path) -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("lsof")
            .arg("-Fpc")
            .arg(path)
            .output()
            .await
            .map_err(|error| format!("无法可靠执行 lsof：{error}"))?;
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(format!(
                "lsof 无法检查 {}，已按不安全处理",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        let mut pid = String::new();
        let mut holders = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(value) = line.strip_prefix('p') {
                pid = value.to_string();
            } else if let Some(value) = line.strip_prefix('c') {
                holders.push(format!("{value} (PID {pid})"));
            }
        }
        holders.sort();
        holders.dedup();
        Ok(holders)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("macOS 文件占用检查不可用".into())
    }
}

fn windows_database_lock(path: &std::path::Path) -> Result<Vec<String>, String> {
    #[cfg(target_os = "windows")]
    {
        database_write_lock(path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err("Windows 数据库锁检查不可用".into())
    }
}

#[cfg(any(target_os = "windows", test))]
fn database_write_lock(path: &std::path::Path) -> Result<Vec<String>, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("无法以写入模式验证数据库锁：{error}"))?;
    connection
        .busy_timeout(Duration::from_millis(250))
        .map_err(|error| format!("无法设置数据库锁超时：{error}"))?;
    connection
        .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
        .map_err(|error| format!("数据库存在写入竞争或无法取得安全写锁：{error}"))?;
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tasklist_without_decoding_localized_output() {
        let output = br#""Cursor.exe","5472","Console","1","100,000 K"
INFO: No tasks are running
"Cursor.EXE","8244","Console","1","20,000 K""#;
        assert_eq!(
            parse_windows_tasklist(output),
            ["Cursor.exe (PID 5472)", "Cursor.exe (PID 8244)"]
        );
    }

    #[test]
    fn sqlite_lock_probe_refuses_an_active_writer() {
        let path = std::env::temp_dir().join(format!(
            "cursor-cleaner-lock-test-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE test(id INTEGER); BEGIN IMMEDIATE;")
            .unwrap();
        assert!(database_write_lock(&path).is_err());
        connection.execute_batch("ROLLBACK").unwrap();
        assert!(database_write_lock(&path).is_ok());
        drop(connection);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn chooses_platform_specific_cursor_user_roots() {
        let home = std::path::Path::new("/home/test");
        let appdata = std::path::Path::new("/roaming");
        assert_eq!(
            cursor_user_for(HostPlatform::MacOs, home, None),
            home.join("Library/Application Support/Cursor/User")
        );
        assert_eq!(
            cursor_user_for(HostPlatform::Windows, home, Some(appdata)),
            appdata.join("Cursor/User")
        );
        assert_eq!(
            cursor_user_for(HostPlatform::Windows, home, None),
            home.join("AppData/Roaming/Cursor/User")
        );
    }
}
