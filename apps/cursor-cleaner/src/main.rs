mod app;
mod config;
mod domain;
mod error;
mod platform;
mod preflight;
mod store;
mod view;

use std::{env, path::PathBuf, time::Duration};

use app::{App, Effect, Screen};
use config::Config;
use crossterm::event::{self, Event, KeyEventKind};
use domain::{Conversation, DeletePlan, PreflightReport, ProgressSnapshot, Receipt, SchemaProbe};
use error::AppError;
use store::CursorStore;
use tokio::sync::mpsc;
use tui_kit::{TerminalGuard, Theme};

type LoadedData = (SchemaProbe, Vec<Conversation>);
type Completion = Result<(Receipt, Option<LoadedData>), AppError>;

enum RuntimeEvent {
    Loaded(Result<LoadedData, AppError>),
    Preflight(PreflightReport),
    Plan(Result<DeletePlan, AppError>),
    Progress(ProgressSnapshot),
    Completed(Completion),
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("cursor-cleaner: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = arguments_from_args()?;
    let config = Config::load(arguments.config.as_ref())?;
    let mut app = App::new(config);
    let theme = Theme::detect();
    let mut terminal = TerminalGuard::enter()?;
    let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
    dispatch(Effect::ProbeAndLoad, &app, &runtime_tx);
    let mut dirty = true;

    loop {
        while let Ok(message) = runtime_rx.try_recv() {
            handle_runtime_event(&mut app, message);
            dirty = true;
        }
        if dirty {
            terminal
                .terminal_mut()
                .draw(|frame| view::render(frame, &app, theme))?;
            dirty = false;
        }
        if app.should_quit {
            break;
        }
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(effect) = app.handle_key(key) {
                        dispatch(effect, &app, &runtime_tx);
                    }
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        } else {
            tokio::task::yield_now().await;
        }
    }
    Ok(())
}

fn dispatch(effect: Effect, app: &App, runtime_tx: &mpsc::UnboundedSender<RuntimeEvent>) {
    match effect {
        Effect::ProbeAndLoad => {
            let config = app.config.clone();
            let tx = runtime_tx.clone();
            tokio::task::spawn_blocking(move || {
                let store = CursorStore::new(config);
                let result = store.probe().and_then(|probe| {
                    let records = if probe.supported() {
                        store.load_conversations()?
                    } else {
                        Vec::new()
                    };
                    Ok((probe, records))
                });
                let _ = tx.send(RuntimeEvent::Loaded(result));
            });
        }
        Effect::RunPreflight(_ids) => {
            let config = app.config.clone();
            let tx = runtime_tx.clone();
            tokio::spawn(async move {
                let report = preflight::run(&config).await;
                let _ = tx.send(RuntimeEvent::Preflight(report));
            });
        }
        Effect::BuildPlan(ids) => {
            let config = app.config.clone();
            let tx = runtime_tx.clone();
            tokio::task::spawn_blocking(move || {
                let result = CursorStore::new(config).build_delete_plan(&ids);
                let _ = tx.send(RuntimeEvent::Plan(result));
            });
        }
        Effect::Execute(plan) => {
            let config = app.config.clone();
            let tx = runtime_tx.clone();
            tokio::spawn(async move {
                let report = preflight::run(&config).await;
                if !report.can_continue() {
                    let _ = tx.send(RuntimeEvent::Completed(Err(AppError::Preflight(
                        "执行前复检未通过；Cursor 必须完全退出且数据库不得被占用".into(),
                    ))));
                    return;
                }
                let _ = tx.send(RuntimeEvent::Progress(ProgressSnapshot {
                    stage: "执行前复检通过，正在准备所选 transcript 的临时回滚数据…".into(),
                    completed: 1,
                    total: 3,
                }));
                let worker_tx = tx.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let store = CursorStore::new(config);
                    let receipt = store.execute_delete(&plan)?;
                    let _ = worker_tx.send(RuntimeEvent::Progress(ProgressSnapshot {
                        stage: "事务已提交，正在重新加载并校验记录列表…".into(),
                        completed: 2,
                        total: 3,
                    }));
                    let probe = store.probe()?;
                    let records = store.load_conversations()?;
                    Ok::<_, AppError>((receipt, Some((probe, records))))
                })
                .await
                .unwrap_or_else(|error| Err(AppError::Execution(error.to_string())));
                let _ = tx.send(RuntimeEvent::Completed(result));
            });
        }
    }
}

fn handle_runtime_event(app: &mut App, event: RuntimeEvent) {
    match event {
        RuntimeEvent::Loaded(Ok((probe, records))) => app.set_loaded(probe, records),
        RuntimeEvent::Loaded(Err(error)) => app.set_error(error),
        RuntimeEvent::Preflight(report) if app.screen == Screen::Preflight => {
            app.preflight = Some(report);
        }
        RuntimeEvent::Plan(Ok(plan)) if app.screen == Screen::Planning => {
            app.plan = Some(plan);
            app.screen = Screen::Preview;
        }
        RuntimeEvent::Plan(Err(error)) => app.set_error(error),
        RuntimeEvent::Progress(progress) if app.screen == Screen::Running => {
            app.progress = Some(progress);
        }
        RuntimeEvent::Completed(Ok((receipt, refreshed))) => {
            if let Some((probe, records)) = refreshed {
                app.set_loaded(probe, records);
                app.selected.clear();
            }
            app.progress = Some(ProgressSnapshot {
                stage: "完成".into(),
                completed: 3,
                total: 3,
            });
            app.receipt = Some(receipt);
            app.screen = Screen::Receipt;
        }
        RuntimeEvent::Completed(Err(error)) => app.set_error(error),
        _ => {}
    }
}

struct Arguments {
    config: Option<PathBuf>,
}

fn arguments_from_args() -> Result<Arguments, AppError> {
    let mut args = env::args_os().skip(1);
    let mut config = None;
    while let Some(arg) = args.next() {
        if arg == "--config" {
            config = args.next().map(PathBuf::from);
            if config.is_none() {
                return Err(AppError::InvalidConfig("--config 后缺少路径".into()));
            }
        } else if arg == "--help" || arg == "-h" {
            println!(
                "cursor-cleaner [--config PATH]\n\n默认只读检测 Cursor 本地数据库；清理成功后不会保留独立备份。"
            );
            std::process::exit(0);
        } else if arg == "--version" || arg == "-V" {
            println!("cursor-cleaner {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        } else {
            return Err(AppError::InvalidConfig(format!(
                "未知参数：{}",
                arg.to_string_lossy()
            )));
        }
    }
    Ok(Arguments { config })
}
