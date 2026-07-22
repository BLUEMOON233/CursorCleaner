use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("无法读取配置：{0}")]
    ConfigRead(#[source] io::Error),
    #[error("配置格式无效：{0}")]
    ConfigParse(#[source] toml::de::Error),
    #[error("配置不安全：{0}")]
    InvalidConfig(String),
    #[error("数据源检查失败：{0}")]
    Probe(String),
    #[error("不支持的 Cursor 数据结构：{0}")]
    UnsupportedSchema(String),
    #[error("数据库操作失败：{0}")]
    Database(#[from] rusqlite::Error),
    #[error("环境检查未通过：{0}")]
    Preflight(String),
    #[error("清理计划生成失败：{0}")]
    Planning(String),
    #[error("清理计划已经变化，请重新预览")]
    PlanChanged,
    #[error("执行被拒绝：{0}")]
    Execution(String),
    #[error("I/O 操作失败（{path}）：{source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl AppError {
    pub fn suggestion(&self) -> &'static str {
        match self {
            Self::ConfigRead(_) | Self::ConfigParse(_) | Self::InvalidConfig(_) => {
                "检查配置文件中的绝对路径后重新启动。"
            }
            Self::Probe(_) | Self::UnsupportedSchema(_) => "仅执行诊断；不要对未知结构写入。",
            Self::Preflight(_) => "完全退出 Cursor，修复检查项后按 R 重试。",
            Self::Planning(_) | Self::PlanChanged => "返回记录页并重新生成影响预览。",
            Self::Execution(_) | Self::Database(_) | Self::Io { .. } => {
                "检查数据库状态和错误详情后再重试。"
            }
        }
    }
}
