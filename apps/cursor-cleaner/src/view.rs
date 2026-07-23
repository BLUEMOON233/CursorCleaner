use chrono::{DateTime, Local};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
};
use tui_kit::{Theme, draw_shell};

use crate::{
    app::{App, Screen},
    domain::{CheckState, SchemaState, human_bytes},
};

pub fn render(frame: &mut Frame<'_>, app: &App, theme: Theme) {
    if frame.area().width < 40 || frame.area().height < 9 {
        frame.render_widget(
            Paragraph::new("终端窗口太小，请调整到至少 40×9。").alignment(Alignment::Center),
            frame.area(),
        );
        return;
    }
    let (page, footer) = page_meta(app);
    let status = if app.screen == Screen::Running {
        "运行中"
    } else {
        concat!("v", env!("CARGO_PKG_VERSION"))
    };
    let body = draw_shell(frame, "Cursor Cleaner", page, status, footer, theme).body;
    match app.screen {
        Screen::Home => home(frame, body, app, theme),
        Screen::SourceCheck => source_check(frame, body, app, theme),
        Screen::Workspaces => workspaces(frame, body, app, theme),
        Screen::Records => records(frame, body, app, theme),
        Screen::Detail => detail(frame, body, app, theme),
        Screen::Preflight => preflight(frame, body, app, theme),
        Screen::Planning => centered(frame, body, "正在生成不可变影响计划…", theme),
        Screen::Preview => preview(frame, body, app, theme),
        Screen::Confirm => confirm(frame, body, app, theme),
        Screen::Running => running(frame, body, app, theme),
        Screen::Receipt => receipt(frame, body, app, theme),
        Screen::Error => error(frame, body, app, theme),
        Screen::Help => help(frame, body, theme),
    }
    if let Some(message) = &app.toast {
        let area = Rect {
            x: body.x + body.width.saturating_sub(34),
            y: body.y,
            width: body.width.min(34),
            height: body.height.min(4),
        };
        frame.render_widget(
            Paragraph::new(message.as_str())
                .wrap(Wrap { trim: true })
                .block(panel(" 提示 ", theme)),
            area,
        );
    }
}

fn page_meta(app: &App) -> (&'static str, &'static str) {
    match app.screen {
        Screen::Home => ("主页", "↑↓/jk 移动 · Enter 打开 · R 刷新 · Q 退出"),
        Screen::SourceCheck => ("数据源检查", "Enter 浏览 · R 重试 · Esc 返回"),
        Screen::Workspaces => (
            "工作目录",
            "↑↓/jk 移动 · Enter 查看对话 · R 刷新 · Esc 返回",
        ),
        Screen::Records if app.searching => (
            "记录搜索",
            "输入关键词 · Backspace 删除 · Enter 应用 · Esc 结束",
        ),
        Screen::Records => (
            "记录列表与详情",
            "↑↓/jk 移动 · Space 多选 · / 搜索 · F 切换全部/未归档/已归档 · Enter 详情 · X 清理",
        ),
        Screen::Detail => ("记录详情", "↑↓/PgUp/PgDn 滚动 · X 清理 · Esc 返回"),
        Screen::Preflight => ("环境检查", "Enter 生成计划 · R 重试 · Esc 返回"),
        Screen::Planning => ("生成计划", "请稍候"),
        Screen::Preview => ("影响预览", "Enter 继续确认 · Esc 返回"),
        Screen::Confirm => ("执行确认", "←→/hl 选择 · Enter 确认 · Esc 返回"),
        Screen::Running => ("执行进度", "临时回滚快照与事务阶段不可中断"),
        Screen::Receipt => ("操作回执", "Enter 返回主页 · Q 退出"),
        Screen::Error => ("错误", "R 重新检查 · Enter/Esc 返回 · ? 帮助"),
        Screen::Help => ("帮助", "Esc/? 关闭"),
    }
}

fn home(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let columns = split_responsive(area);
    let labels = ["按工作目录浏览", "数据源检查", "关于与帮助"];
    let items = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let marker = if index == app.home_selection {
                "❯"
            } else {
                " "
            };
            ListItem::new(format!("{marker} {label}")).style(if index == app.home_selection {
                theme.selected()
            } else {
                Style::default().fg(theme.fg)
            })
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items).block(panel(" 操作 ", theme)), columns[0]);

    let status = if app.loading {
        vec![Line::from("● 正在只读检测 Cursor 数据…")]
    } else if let Some(probe) = &app.probe {
        let (symbol, color, text) = match probe.state {
            SchemaState::Supported => ("✓", theme.success, "已支持，可浏览与生成计划"),
            SchemaState::Unsupported => ("!", theme.warning, "未知结构：仅允许诊断"),
            SchemaState::Missing => ("×", theme.danger, "数据源缺失"),
        };
        vec![
            Line::styled(
                format!("{symbol} {text}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            Line::from(format!("本地记录   {}", app.records.len())),
            Line::from(format!(
                "搜索库版本 {}",
                probe.search_version.map_or_else(
                    || "未检测（state.vscdb 单库模式）".into(),
                    |v| v.to_string()
                )
            )),
            Line::from(format!(
                "状态库版本 {}",
                probe
                    .state_version
                    .map_or_else(|| "未知".into(), |v| v.to_string())
            )),
            Line::from(""),
            Line::styled(
                "任何永久清理都会在执行前重新验证，并准备临时回滚数据。",
                Style::default().fg(theme.muted),
            ),
        ]
    } else {
        vec![Line::from("尚未取得数据源状态")]
    };
    frame.render_widget(
        Paragraph::new(status)
            .wrap(Wrap { trim: false })
            .block(panel(" 当前状态 ", theme)),
        columns[1],
    );
}

fn source_check(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    if app.loading {
        centered(frame, area, "正在以只读方式检测 schema 和加载索引…", theme);
        return;
    }
    let Some(probe) = &app.probe else {
        centered(frame, area, "尚无检查结果，按 R 重试。", theme);
        return;
    };
    let (symbol, color, conclusion) = match probe.state {
        SchemaState::Supported => ("✓", theme.success, "数据结构受支持"),
        SchemaState::Unsupported => ("!", theme.warning, "数据结构未知，写操作已禁用"),
        SchemaState::Missing => ("×", theme.danger, "Cursor 数据源不完整"),
    };
    let mut lines = vec![
        Line::styled(
            format!("{symbol} {conclusion}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Line::from(format!("搜索库   {}", app.config.search_db.display())),
        Line::from(format!("状态库   {}", app.config.state_db.display())),
        Line::from(format!("记录数   {}", app.records.len())),
        Line::from(""),
    ];
    for diagnostic in &probe.diagnostics {
        lines.push(Line::styled(
            format!("! {diagnostic}"),
            Style::default().fg(theme.warning),
        ));
    }
    if !probe.supported() {
        lines.push(Line::from(""));
        lines.push(Line::from("未知结构不会被解释或修改。"));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" Schema 与数据源 ", theme)),
        area,
    );
}

fn workspaces(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let columns = split_responsive(area);
    let groups = app.workspace_groups();
    let visible_height = columns[0].height.saturating_sub(2) as usize;
    let offset = app
        .workspace_cursor
        .saturating_sub(visible_height.saturating_sub(1));
    let items = groups
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(index, group)| {
            let prefix = if index == app.workspace_cursor {
                "❯"
            } else {
                " "
            };
            ListItem::new(format!(
                "{prefix} {}  ·  {} 段",
                group.label, group.conversations
            ))
            .style(if index == app.workspace_cursor {
                theme.selected()
            } else {
                Style::default().fg(theme.fg)
            })
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(panel(
            &format!(" 工作目录 · {} 个分组 ", groups.len()),
            theme,
        )),
        columns[0],
    );

    let lines = groups
        .get(app.workspace_cursor)
        .map(|group| {
            let kind = if std::path::Path::new(&group.label).is_absolute() {
                "已确认路径"
            } else {
                "特殊/无法识别分组"
            };
            vec![
                label_line("工作目录", &group.label, theme),
                label_line("类型", kind, theme),
                label_line("对话数量", &group.conversations.to_string(), theme),
                label_line("已归档", &group.archived.to_string(), theme),
                label_line("最近活动", &format_time(group.latest_updated_at), theme),
                Line::from(""),
                Line::styled(
                    "进入后，搜索、多选和清理仅作用于此工作目录的对话。",
                    Style::default().fg(theme.muted),
                ),
                Line::styled(
                    "真实项目目录始终只展示、不写入。",
                    Style::default().fg(theme.success),
                ),
            ]
        })
        .unwrap_or_else(|| vec![Line::from("没有找到可分组的本地对话。")]);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" 分组详情 ", theme)),
        columns[1],
    );
}

fn records(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let columns = split_responsive(area);
    let indices = app.filtered_indices();
    let visible_height = columns[0].height.saturating_sub(2) as usize;
    let offset = app
        .record_cursor
        .saturating_sub(visible_height.saturating_sub(1));
    let items = indices
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(position, index)| {
            let record = &app.records[*index];
            let prefix = if position == app.record_cursor {
                "❯"
            } else {
                " "
            };
            let selected = if app.selected.contains(&record.id) {
                "●"
            } else {
                "○"
            };
            let archived = if record.archived { " [归档]" } else { "" };
            ListItem::new(format!(
                "{prefix} {selected} {} {}{archived}",
                format_time(record.updated_at),
                record.title.replace('\n', " ")
            ))
            .style(if position == app.record_cursor {
                theme.selected()
            } else {
                Style::default().fg(theme.fg)
            })
        })
        .collect::<Vec<_>>();
    let title = format!(
        " 当前目录 · 记录 {}/{} · 已选 {} · 过滤 {} ",
        indices.len(),
        app.records.len(),
        app.selected.len(),
        app.archive_filter.label()
    );
    frame.render_widget(List::new(items).block(panel(&title, theme)), columns[0]);

    let mut lines = record_lines(app, theme);
    if app.searching || !app.query.is_empty() {
        lines.insert(
            0,
            Line::styled(
                format!(
                    "搜索 /{}{}",
                    app.query,
                    if app.searching { "▌" } else { "" }
                ),
                Style::default().fg(theme.accent),
            ),
        );
        lines.insert(1, Line::from(""));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" 详情预览 ", theme)),
        columns[1],
    );
}

fn detail(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    frame.render_widget(
        Paragraph::new(record_lines(app, theme))
            .scroll((app.detail_scroll, 0))
            .wrap(Wrap { trim: false })
            .block(panel(" 完整记录详情 ", theme)),
        area,
    );
}

fn record_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let Some(record) = app.current_record() else {
        return vec![Line::from("没有符合当前搜索和过滤条件的记录。")];
    };
    vec![
        label_line("标题", &record.title, theme),
        label_line("ID", &record.id, theme),
        label_line("工作区", &record.workspace, theme),
        label_line("更新时间", &format_time(record.updated_at), theme),
        label_line("来源", &record.source, theme),
        label_line(
            "状态",
            if record.archived {
                "已归档"
            } else {
                "未归档"
            },
            theme,
        ),
        label_line("正文索引", &human_bytes(record.logical_bytes), theme),
        Line::from(""),
        Line::styled("用户/助手消息预览", Style::default().fg(theme.muted)),
        Line::from(if record.preview.is_empty() {
            "未从已知字段解析出消息预览。".to_string()
        } else {
            record.preview.clone()
        }),
        Line::from(""),
        Line::styled(
            "项目路径仅展示，程序绝不会写入真实项目目录。",
            Style::default().fg(theme.success),
        ),
    ]
}

fn preflight(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(report) = &app.preflight else {
        centered(
            frame,
            area,
            "正在检查 schema、完整性、Cursor 进程和数据库占用…",
            theme,
        );
        return;
    };
    let lines = report
        .checks
        .iter()
        .map(|check| {
            let (symbol, color) = match check.state {
                CheckState::Passed => ("✓", theme.success),
                CheckState::Warning => ("!", theme.warning),
                CheckState::Failed => ("×", theme.danger),
            };
            Line::from(vec![
                Span::styled(
                    format!("{symbol} {:<12} ", check.label),
                    Style::default().fg(color),
                ),
                Span::raw(check.detail.clone()),
            ])
        })
        .collect::<Vec<_>>();
    let title = if report.can_continue() {
        " 环境检查通过 "
    } else {
        " 环境检查未通过，禁止执行 "
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(title, theme)),
        area,
    );
}

fn preview(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(plan) = &app.plan else { return };
    let lines = vec![
        Line::styled(
            format!("! 永久删除 {} 个选中对话", plan.conversation_ids.len()),
            Style::default()
                .fg(theme.danger)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(format!("计划标识         {:016x}", plan.id)),
        Line::from(format!("关联会话标识     {}", plan.owned_ids.len())),
        Line::from(format!("搜索主记录       {}", plan.impact.conversations)),
        Line::from(format!(
            "FTS / 候选记录   {} / {}",
            plan.impact.fts_rows, plan.impact.candidates
        )),
        Line::from(format!(
            "正文状态 / 头    {} / {}",
            plan.impact.state_rows, plan.impact.headers
        )),
        Line::from(format!(
            "Transcript        {} 个目录 / {}",
            plan.impact.transcript_dirs,
            human_bytes(plan.impact.transcript_bytes)
        )),
        Line::from(format!(
            "未知关联 key     {}（仅报告，不删除）",
            plan.impact.unknown_keys
        )),
        Line::from(""),
        Line::styled(
            "执行顺序：重新验证 → 临时回滚快照 → 清理 transcript → 数据库事务 → 校验 → 清理快照 → 回执",
            Style::default().fg(theme.warning),
        ),
        Line::from(""),
        Line::styled(
            "受保护项目路径（绝不写入）",
            Style::default().fg(theme.success),
        ),
    ];
    let mut lines = lines;
    if plan.protected_paths.is_empty() {
        lines.push(Line::from("- 未解析出路径；写入仍限制在 Cursor 数据白名单"));
    } else {
        for path in &plan.protected_paths {
            lines.push(Line::from(format!("- {path}")));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" 清理影响 ", theme)),
        area,
    );
}

fn confirm(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let cancel = if app.confirm_execute {
        Style::default().fg(theme.muted)
    } else {
        theme.selected()
    };
    let execute = if app.confirm_execute {
        Style::default()
            .fg(theme.danger)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    };
    let count = app
        .plan
        .as_ref()
        .map_or(0, |plan| plan.conversation_ids.len());
    let content = vec![
        Line::from(format!(
            "确认永久清理 {count} 个对话？成功后临时回滚数据会立即清理。"
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  取消（默认）  ", cancel),
            Span::raw("    "),
            Span::styled("  执行清理  ", execute),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(content)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false })
            .block(panel(" 默认取消 ", theme)),
        centered_rect(78, 8, area),
    );
}

fn running(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let progress = app.progress.as_ref();
    let completed = progress.map_or(0, |value| value.completed);
    let total = progress.map_or(1, |value| value.total.max(1));
    let ratio = completed as f64 / total as f64;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(progress.map_or("准备执行", |value| value.stage.as_str()))
            .wrap(Wrap { trim: false })
            .block(panel(" 当前阶段 ", theme)),
        chunks[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(panel(" 总进度 ", theme))
            .gauge_style(Style::default().fg(theme.success))
            .ratio(ratio)
            .label(format!("{completed} / {total}")),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(
            "临时回滚快照和 SQLite 事务期间拒绝重复提交；异常退出时终端由 RAII 自动恢复。",
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(theme.muted))
        .block(panel(" 安全状态 ", theme)),
        chunks[2],
    );
}

fn receipt(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let Some(receipt) = &app.receipt else { return };
    let started: DateTime<Local> = receipt.started_at.into();
    let ended: DateTime<Local> = receipt.ended_at.into();
    let elapsed = receipt
        .ended_at
        .duration_since(receipt.started_at)
        .unwrap_or_default();
    let lines = vec![
        Line::styled(
            if receipt.verified {
                "✓ 执行成功并通过校验"
            } else {
                "× 校验未通过"
            },
            Style::default()
                .fg(if receipt.verified {
                    theme.success
                } else {
                    theme.danger
                })
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(format!(
            "开始       {}",
            started.format("%Y-%m-%d %H:%M:%S")
        )),
        Line::from(format!("结束       {}", ended.format("%Y-%m-%d %H:%M:%S"))),
        Line::from(format!("耗时       {:.1} 秒", elapsed.as_secs_f64())),
        Line::from(format!("对话记录   {}", receipt.deleted_conversations)),
        Line::from(format!("状态记录   {}", receipt.deleted_state_rows)),
        Line::from(format!("目录       {}", receipt.deleted_transcript_dirs)),
        Line::from(""),
        Line::styled(
            "临时回滚数据已清理；未保留独立备份。",
            Style::default().fg(theme.success),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" 操作结果 ", theme)),
        area,
    );
}

fn error(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let (summary, suggestion, detail) = app
        .error
        .as_ref()
        .map(|value| (value.0.as_str(), value.1.as_str(), value.2.as_str()))
        .unwrap_or(("未知错误", "返回后重试。", "无技术详情"));
    let lines = vec![
        Line::styled(
            format!("× {summary}"),
            Style::default()
                .fg(theme.danger)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::styled(
            format!("建议：{suggestion}"),
            Style::default().fg(theme.warning),
        ),
        Line::from(""),
        Line::styled(
            "技术详情（已避免输出对话正文）",
            Style::default().fg(theme.muted),
        ),
        Line::from(detail.to_string()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" 无法继续 ", theme)),
        area,
    );
}

fn help(frame: &mut Frame<'_>, area: Rect, theme: Theme) {
    let text = "↑↓ / jk       移动或滚动\nSpace          多选记录\n/              搜索标题、ID、工作区\nF              切换全部/未归档/已归档\nEnter          打开主要操作\nX / Delete     进入清理安全流程（不会直接删除）\nEsc            返回\nCtrl+C         安全退出；事务阶段拒绝中断\n?              打开或关闭帮助\n\n流程：环境检查 → 计划 → 影响预览 → 默认取消确认 → 临时回滚快照 → 执行 → 校验 → 回执";
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(panel(" 键盘与安全流程 ", theme)),
        area,
    );
}

fn label_line(label: &str, value: &str, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), Style::default().fg(theme.muted)),
        Span::raw(value.to_string()),
    ])
}

fn format_time(milliseconds: i64) -> String {
    DateTime::from_timestamp_millis(milliseconds)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "未知时间".into())
}

fn centered(frame: &mut Frame<'_>, area: Rect, text: &str, theme: Theme) {
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.muted)),
        centered_rect(80, 3, area),
    );
}

fn panel<'a>(title: &'a str, theme: Theme) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
}

fn split_responsive(area: Rect) -> Vec<Rect> {
    if area.width >= 72 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(area)
            .to_vec()
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(area)
            .to_vec()
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height.min(area.height)),
            Constraint::Fill(1),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        config::Config,
        domain::{Conversation, DeletePlan, Impact, ProgressSnapshot, Receipt, SchemaProbe},
    };

    fn populated_app(screen: Screen) -> App {
        let mut app = App::new(Config::default());
        app.screen = screen;
        app.loading = false;
        app.probe = Some(SchemaProbe {
            state: SchemaState::Supported,
            search_version: Some(7),
            state_version: Some(1),
            diagnostics: vec![],
        });
        app.records.push(Conversation {
            id: "11111111-2222-4333-8444-555555555555".into(),
            title: "包含中文和 very-long-title 的对话记录".into(),
            updated_at: 1_721_000_000_000,
            source: "local".into(),
            archived: false,
            workspace: "/极其/长的/中文/工作区/路径/用于/验证/响应式布局".into(),
            preview: "user: 请检查终端恢复\nassistant: 已处理".into(),
            logical_bytes: 128,
        });
        app.plan = Some(DeletePlan {
            id: 7,
            created_at: SystemTime::now(),
            conversation_ids: vec![app.records[0].id.clone()],
            owned_ids: vec![app.records[0].id.clone()],
            transcript_dirs: vec![],
            impact: Impact {
                conversations: 1,
                ..Impact::default()
            },
            protected_paths: vec![app.records[0].workspace.clone()],
        });
        app.progress = Some(ProgressSnapshot {
            stage: "正在创建临时回滚快照".into(),
            completed: 1,
            total: 3,
        });
        app.receipt = Some(Receipt {
            started_at: SystemTime::now(),
            ended_at: SystemTime::now(),
            deleted_conversations: 1,
            deleted_state_rows: 2,
            deleted_transcript_dirs: 0,
            verified: true,
        });
        app.error = Some(("测试错误".into(), "返回重试".into(), "detail".into()));
        app
    }

    #[test]
    fn renders_all_core_pages_at_required_sizes() {
        let screens = [
            Screen::Home,
            Screen::SourceCheck,
            Screen::Workspaces,
            Screen::Records,
            Screen::Detail,
            Screen::Preflight,
            Screen::Planning,
            Screen::Preview,
            Screen::Confirm,
            Screen::Running,
            Screen::Receipt,
            Screen::Error,
            Screen::Help,
        ];
        for screen in screens {
            for (width, height) in [(100, 30), (72, 24), (52, 12), (40, 9), (39, 8)] {
                let backend = TestBackend::new(width, height);
                let mut terminal = Terminal::new(backend).unwrap();
                terminal
                    .draw(|frame| render(frame, &populated_app(screen), Theme::monochrome()))
                    .unwrap();
            }
        }
    }
}
