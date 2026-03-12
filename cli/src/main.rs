mod app;
mod config;
mod log;
mod shellutil;
mod ssh;
mod steps;
mod ui;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use app::{Action, App};
use config::Config;
use log::LogLine;
use steps::{Step, StepCtx};

type Term = Terminal<CrosstermBackend<io::Stdout>>;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = Config::resolve_path(std::env::args().nth(1).as_deref());
    let config = Config::load(&config_path)
        .with_context(|| format!("loading cluster config from {}", config_path.display()))?;
    let config = Arc::new(config);

    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, config).await;
    restore_terminal(&mut terminal)?;
    result
}

fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));
}

fn setup_terminal() -> Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

async fn run(terminal: &mut Term, config: Arc<Config>) -> Result<()> {
    let mut app = App::new(config.clone());
    let mut creds = ssh::Credentials::default();

    let (log_tx, mut log_rx) = mpsc::unbounded_channel::<LogLine>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<(Step, Result<(), String>)>();
    let (probe_tx, mut probe_rx) = mpsc::unbounded_channel::<(String, bool)>();

    probe_all(&config, &probe_tx);

    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));

    terminal.draw(|f| ui::draw(f, &app))?;

    loop {
        tokio::select! {
            maybe_event = events.next() => {
                let Some(event) = maybe_event else { break };
                if let Event::Key(key) = event? {
                    if key.kind == KeyEventKind::Press {
                        match app.handle_key(key) {
                            Action::None => {}
                            Action::Quit => app.quit = true,
                            Action::CredentialsSubmitted(c) => {
                                creds = c;
                                probe_all(&config, &probe_tx);
                            }
                            Action::ProbeAll => probe_all(&config, &probe_tx),
                            Action::Dispatch(step) => {
                                spawn_step(step, config.clone(), creds.clone(), log_tx.clone(), done_tx.clone());
                            }
                        }
                    }
                }
            }
            Some(line) = log_rx.recv() => app.push_log(line),
            Some((step, res)) = done_rx.recv() => app.step_finished(step, res),
            Some((node, reachable)) = probe_rx.recv() => app.set_probe(&node, reachable),
            _ = ticker.tick() => {}
        }

        if app.quit {
            break;
        }
        terminal.draw(|f| ui::draw(f, &app))?;
    }

    Ok(())
}

fn probe_all(config: &Arc<Config>, tx: &mpsc::UnboundedSender<(String, bool)>) {
    let port = config.settings.ssh_port;
    for node in config.nodes.clone() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let reachable = ssh::probe_tcp(&node.address, port, Duration::from_secs(2)).await;
            let _ = tx.send((node.name, reachable));
        });
    }
}

fn spawn_step(
    step: Step,
    config: Arc<Config>,
    creds: ssh::Credentials,
    log_tx: log::LogSender,
    done_tx: mpsc::UnboundedSender<(Step, Result<(), String>)>,
) {
    tokio::spawn(async move {
        let ctx = StepCtx { config, creds, tx: log_tx };
        let res = steps::run(step, &ctx).await.map_err(|e| format!("{e:#}"));
        let _ = done_tx.send((step, res));
    });
}
