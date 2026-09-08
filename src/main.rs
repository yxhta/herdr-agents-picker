mod app;
mod fuzzy;
mod herdr;
mod order;
mod preview;
mod ui;

use std::env;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::DefaultTerminal;

use app::{App, Mode};
use herdr::{Agent, Client};

/// Redraw cadence; the spinner frame advances independently at Herdr's ~8fps.
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const LIST_REFRESH: Duration = Duration::from_secs(2);
const RESIZE_SETTLE: Duration = Duration::from_millis(150);

fn main() -> ExitCode {
    let argument = env::args().nth(1);
    match argument.as_deref() {
        None => run_picker(),
        Some("--open") => open_picker_pane(),
        Some("--record-status") => match order::record_status_event() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("agents-picker: {error}");
                ExitCode::FAILURE
            }
        },
        Some("--help" | "-h") => {
            println!("agents-picker          run the picker TUI (inside a herdr plugin pane)");
            println!("agents-picker --open   open the picker pane via the herdr CLI");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("agents-picker: unknown argument: {other}");
            ExitCode::from(2)
        }
    }
}

/// Action entrypoint: ask herdr to open our own picker pane. This is what a
/// `plugin_action` keybinding invokes.
fn open_picker_pane() -> ExitCode {
    let bin = env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string());
    let plugin = env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| "yxhta.agents-picker".to_string());
    let status = Command::new(&bin)
        .args([
            "plugin",
            "pane",
            "open",
            "--plugin",
            &plugin,
            "--entrypoint",
            "picker",
            "--focus",
        ])
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("agents-picker: failed to run {bin}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_picker() -> ExitCode {
    let client = Client::from_env();
    let mut app = App::new(env::var("HOME").ok(), env::var("HERDR_PANE_ID").ok());
    let setup_error = apply_agent_panel_sort(&mut app).or_else(|| apply_status_order(&mut app));
    if let Ok(index) = client.workspace_index() {
        app.set_workspace_index(index);
    }
    match client.list_agents() {
        Ok(agents) => {
            app.set_agents(agents);
            app.error = setup_error;
        }
        Err(error) => app.error = Some(error.to_string()),
    }

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, &client);
    ratatui::restore();

    match result {
        Ok(Some(focus)) => match client.focus_agent(&focus) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("agents-picker: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agents-picker: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs until the user picks an agent (returns its target) or cancels
/// (returns None). The agent list refreshes periodically; preview frames stream
/// from the selected agent.
fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    client: &Client,
) -> std::io::Result<Option<herdr::Focus>> {
    let mut listed_at = Instant::now();
    let mut preview = preview::Controller::new();
    let size = terminal.size()?;
    let mut preview_size = ui::preview_dimensions(Rect::new(0, 0, size.width, size.height));
    let mut pending_resize: Option<(u16, u16, Instant)> = None;

    loop {
        if let Some((width, height, resized_at)) = pending_resize {
            if resized_at.elapsed() >= RESIZE_SETTLE {
                preview_size = ui::preview_dimensions(Rect::new(0, 0, width, height));
                pending_resize = None;
            }
        }
        preview.refresh(app, client, preview_size);
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(POLL_INTERVAL)? {
            let next_event = event::read()?;
            if let Event::Resize(width, height) = next_event {
                pending_resize = Some((width, height, Instant::now()));
            }
            if let Event::Key(key) = next_event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                if ctrl && matches!(key.code, KeyCode::Char('c' | 'g')) {
                    return Ok(None);
                }
                if key.code == KeyCode::Enter {
                    if let Some(focus) = app.selected_agent().and_then(Agent::focus) {
                        return Ok(Some(focus));
                    }
                    continue;
                }
                match app.mode {
                    Mode::Navigate => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                        KeyCode::Char('/') => app.enter_search(),
                        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                        KeyCode::Char('p') if ctrl => app.move_selection(-1),
                        KeyCode::Char('n') if ctrl => app.move_selection(1),
                        KeyCode::Char('r') => {
                            reload_agents(app, client);
                            listed_at = Instant::now();
                        }
                        KeyCode::Char('u') if ctrl => app.clear_filter(),
                        _ => {}
                    },
                    Mode::Search => match key.code {
                        KeyCode::Esc => app.cancel_search(),
                        KeyCode::Up => app.move_selection(-1),
                        KeyCode::Down => app.move_selection(1),
                        KeyCode::Char('p' | 'k') if ctrl => app.move_selection(-1),
                        KeyCode::Char('n' | 'j') if ctrl => app.move_selection(1),
                        KeyCode::Char('r') if ctrl => {
                            reload_agents(app, client);
                            listed_at = Instant::now();
                        }
                        KeyCode::Char('u') if ctrl => app.clear_filter(),
                        KeyCode::Char('w') if ctrl => app.pop_word(),
                        KeyCode::Backspace => app.pop_char(),
                        KeyCode::Char(c) if !ctrl => app.push_char(c),
                        _ => {}
                    },
                }
            }
        }

        if listed_at.elapsed() >= LIST_REFRESH {
            reload_agents(app, client);
            listed_at = Instant::now();
        }
    }
}

fn reload_agents(app: &mut App, client: &Client) {
    let setup_error = apply_agent_panel_sort(app).or_else(|| apply_status_order(app));
    // Best-effort: a stale workspace/tab index just shows an old label, so
    // errors here don't need to surface through `app.error`.
    if let Ok(index) = client.workspace_index() {
        app.set_workspace_index(index);
    }
    match client.list_agents() {
        Ok(agents) => {
            app.set_agents(agents);
            app.error = setup_error;
        }
        Err(error) => app.error = Some(error.to_string()),
    }
}

fn apply_agent_panel_sort(app: &mut App) -> Option<String> {
    match Client::agent_panel_sort() {
        Ok(sort) => {
            app.set_agent_panel_sort(sort);
            None
        }
        Err(error) => Some(error.to_string()),
    }
}

fn apply_status_order(app: &mut App) -> Option<String> {
    match order::load_status_order() {
        Ok(order) => {
            app.set_status_order(order);
            None
        }
        Err(error) => Some(error.to_string()),
    }
}
