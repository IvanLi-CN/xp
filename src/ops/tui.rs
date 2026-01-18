use crate::ops::cli::DeployArgs;
use crate::ops::cloudflare;
use crate::ops::deploy;
use crate::ops::paths::Paths;
use crate::ops::util::{Mode, chmod, ensure_dir, write_string_if_changed};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{event, execute};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use serde::{Deserialize, Serialize};
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

pub async fn cmd_tui(paths: Paths) -> Result<(), crate::ops::cli::ExitError> {
    let mut stdout = io::stdout();
    enable_raw_mode().map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)
        .map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;

    let mut app = App::new(&paths);
    let outcome = run_loop(&mut terminal, &mut app);

    // Restore terminal.
    disable_raw_mode().ok();
    let mut stdout = io::stdout();
    execute!(stdout, DisableBracketedPaste, LeaveAlternateScreen).ok();

    match outcome {
        TuiOutcome::Quit => Ok(()),
        TuiOutcome::SaveConfig { .. } => Ok(()),
        TuiOutcome::RunDeploy(values) => run_deploy(paths, *values).await,
    }
}

async fn run_deploy(paths: Paths, values: AppValues) -> Result<(), crate::ops::cli::ExitError> {
    let mode = if values.dry_run {
        Mode::DryRun
    } else {
        Mode::Real
    };

    if values.cloudflare_enabled && values.save_token && !values.cloudflare_token.trim().is_empty()
    {
        cloudflare::set_token_value(&paths, &values.cloudflare_token, mode)?;
    }

    let args = DeployArgs {
        xp_bin: PathBuf::from(values.xp_bin),
        node_name: values.node_name,
        public_domain: values.public_domain,
        cloudflare_toggle: crate::ops::cli::CloudflareToggle {
            cloudflare: values.cloudflare_enabled,
            no_cloudflare: !values.cloudflare_enabled,
        },
        account_id: values.account_id,
        zone_id: values.zone_id,
        hostname: values.hostname,
        tunnel_name: None,
        origin_url: values.origin_url,
        api_base_url: values.api_base_url,
        xray_version: values.xray_version,
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: values.enable_services,
            no_enable_services: !values.enable_services,
        },
        yes: false,
        overwrite_existing: false,
        non_interactive: true,
        dry_run: values.dry_run,
    };

    deploy::cmd_deploy(paths, args).await
}

fn run_loop(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    app: &mut App,
) -> TuiOutcome {
    loop {
        terminal.draw(|f| ui(f, app)).ok();

        if !event::poll(Duration::from_millis(200)).unwrap_or(false) {
            continue;
        }

        let Ok(ev) = event::read() else {
            continue;
        };
        match ev {
            Event::Key(key) => {
                if let Some(outcome) = app.handle_key(key) {
                    match outcome {
                        TuiOutcome::Quit => return TuiOutcome::Quit,
                        TuiOutcome::RunDeploy(values) => {
                            return TuiOutcome::RunDeploy(values);
                        }
                        TuiOutcome::SaveConfig { values, exit_after } => {
                            match save_tui_config(&app.paths, &values) {
                                Ok(()) => {
                                    if let Err(e) = save_token_if_needed(&app.paths, &values) {
                                        app.status_message =
                                            Some(format!("save failed: {}", e.message));
                                    } else {
                                        app.status_message = Some("saved".to_string());
                                        if exit_after {
                                            return TuiOutcome::Quit;
                                        }
                                    }
                                }
                                Err(e) => {
                                    app.status_message =
                                        Some(format!("save failed: {}", e.message));
                                }
                            }
                        }
                    }
                }
            }
            Event::Paste(text) => {
                if app.editing {
                    app.push_str(&text);
                }
            }
            _ => {}
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    let title = Paragraph::new("xp-ops TUI · Deploy wizard")
        .block(Block::default().borders(Borders::ALL).title("Overview"));
    f.render_widget(title, chunks[0]);

    let items = app.render_items();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Fields"))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan))
        .highlight_symbol(">");
    f.render_stateful_widget(list, chunks[1], &mut app.list_state);

    let help =
        Paragraph::new(help_text(app)).block(Block::default().borders(Borders::ALL).title("Help"));
    f.render_widget(help, chunks[2]);
}

#[derive(Debug, Clone)]
struct AppValues {
    xp_bin: String,
    node_name: String,
    public_domain: String,
    cloudflare_enabled: bool,
    account_id: Option<String>,
    zone_id: Option<String>,
    hostname: Option<String>,
    origin_url: Option<String>,
    api_base_url: Option<String>,
    xray_version: String,
    cloudflare_token: String,
    save_token: bool,
    enable_services: bool,
    dry_run: bool,
}

enum TuiOutcome {
    Quit,
    SaveConfig {
        values: Box<AppValues>,
        exit_after: bool,
    },
    RunDeploy(Box<AppValues>),
}

struct App {
    list_state: ListState,
    focus: usize,
    editing: bool,
    status_message: Option<String>,
    paths: Paths,

    xp_bin: String,
    node_name: String,
    public_domain: String,

    cloudflare_enabled: bool,
    account_id: String,
    zone_id: String,
    hostname: String,
    origin_url: String,
    api_base_url: String,

    xray_version: String,
    cloudflare_token: String,
    save_token: bool,
    enable_services: bool,
    dry_run: bool,
}

impl App {
    fn new(paths: &Paths) -> Self {
        let mut s = Self {
            list_state: ListState::default(),
            focus: 0,
            editing: false,
            status_message: None,
            paths: paths.clone(),
            xp_bin: String::new(),
            node_name: "node-1".to_string(),
            public_domain: String::new(),
            cloudflare_enabled: true,
            account_id: String::new(),
            zone_id: String::new(),
            hostname: String::new(),
            origin_url: "http://127.0.0.1:62416".to_string(),
            api_base_url: String::new(),
            xray_version: "latest".to_string(),
            cloudflare_token: String::new(),
            save_token: true,
            enable_services: true,
            dry_run: false,
        };
        if let Some(cfg) = load_tui_config(paths) {
            s.apply_config(cfg);
        }
        s.list_state.select(Some(0));
        s
    }

    fn items_len(&self) -> usize {
        13
    }

    fn render_items(&self) -> Vec<ListItem<'static>> {
        let mut v = Vec::new();
        v.push(item("xp_bin", &self.xp_bin));
        v.push(item("node_name", &self.node_name));
        v.push(item("public_domain", &self.public_domain));

        v.push(item(
            "cloudflare_enabled",
            if self.cloudflare_enabled {
                "true"
            } else {
                "false"
            },
        ));

        if self.cloudflare_enabled {
            let derived = if self.hostname.trim().is_empty() {
                "(auto)".to_string()
            } else {
                format!("(auto) https://{}", self.hostname.trim())
            };
            v.push(item("api_base_url", &derived));
        } else {
            v.push(item("api_base_url", &self.api_base_url));
        }

        if self.cloudflare_enabled {
            v.push(item("account_id", &self.account_id));
            v.push(item("zone_id", &self.zone_id));
            v.push(item("hostname", &self.hostname));
            v.push(item("origin_url", &self.origin_url));
        } else {
            v.push(item("account_id (disabled)", "-"));
            v.push(item("zone_id (disabled)", "-"));
            v.push(item("hostname (disabled)", "-"));
            v.push(item("origin_url (disabled)", "-"));
        }

        v.push(item("cloudflare_token", &self.token_display()));
        v.push(item(
            "save_token",
            if self.save_token { "true" } else { "false" },
        ));
        v.push(item(
            "enable_services",
            if self.enable_services {
                "true"
            } else {
                "false"
            },
        ));
        v.push(item("dry_run", if self.dry_run { "true" } else { "false" }));
        v
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiOutcome> {
        if self.editing {
            return self.handle_edit_key(key);
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Some(TuiOutcome::Quit),
            KeyCode::Tab => self.next(),
            KeyCode::BackTab => self.prev(),
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.prev(),
            KeyCode::Enter => self.handle_enter(),
            KeyCode::Char('s') => {
                return Some(TuiOutcome::SaveConfig {
                    values: Box::new(self.to_values()),
                    exit_after: false,
                });
            }
            KeyCode::Char('S') => {
                return Some(TuiOutcome::SaveConfig {
                    values: Box::new(self.to_values()),
                    exit_after: true,
                });
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                return Some(TuiOutcome::RunDeploy(Box::new(self.to_values())));
            }
            _ => {}
        }
        self.list_state.select(Some(self.focus));
        None
    }

    fn handle_edit_key(&mut self, key: KeyEvent) -> Option<TuiOutcome> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.editing = false;
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return None;
                }
                self.push_char(c);
            }
            _ => {}
        }
        self.list_state.select(Some(self.focus));
        None
    }

    fn next(&mut self) {
        self.focus = (self.focus + 1).min(self.items_len() - 1);
    }

    fn prev(&mut self) {
        if self.focus == 0 {
            return;
        }
        self.focus -= 1;
    }

    fn handle_enter(&mut self) {
        match self.focus {
            3 => self.cloudflare_enabled = !self.cloudflare_enabled,
            10 => self.save_token = !self.save_token,
            11 => self.enable_services = !self.enable_services,
            12 => self.dry_run = !self.dry_run,
            _ => {
                if self.is_editable_field() {
                    self.editing = true;
                } else {
                    self.next();
                }
            }
        }
    }

    fn is_editable_field(&self) -> bool {
        match self.focus {
            0..=2 => true,
            4 => !self.cloudflare_enabled,
            5..=8 => self.cloudflare_enabled,
            9 => true,
            _ => false,
        }
    }

    fn push_char(&mut self, c: char) {
        match self.focus {
            0 => self.xp_bin.push(c),
            1 => self.node_name.push(c),
            2 => self.public_domain.push(c),
            4 if !self.cloudflare_enabled => self.api_base_url.push(c),
            5 if self.cloudflare_enabled => self.account_id.push(c),
            6 if self.cloudflare_enabled => self.zone_id.push(c),
            7 if self.cloudflare_enabled => self.hostname.push(c),
            8 if self.cloudflare_enabled => self.origin_url.push(c),
            9 => self.cloudflare_token.push(c),
            _ => {}
        }
    }

    fn push_str(&mut self, s: &str) {
        for c in s.chars() {
            if matches!(c, '\n' | '\r' | '\t') {
                continue;
            }
            self.push_char(c);
        }
    }

    fn token_display(&self) -> String {
        if !self.cloudflare_token.is_empty() {
            return mask_token(&self.cloudflare_token);
        }
        if self.paths.etc_xp_ops_cloudflare_token().exists() {
            return "(saved)".to_string();
        }
        String::new()
    }

    fn backspace(&mut self) {
        match self.focus {
            0 => {
                self.xp_bin.pop();
            }
            1 => {
                self.node_name.pop();
            }
            2 => {
                self.public_domain.pop();
            }
            4 if !self.cloudflare_enabled => {
                self.api_base_url.pop();
            }
            5 if self.cloudflare_enabled => {
                self.account_id.pop();
            }
            6 if self.cloudflare_enabled => {
                self.zone_id.pop();
            }
            7 if self.cloudflare_enabled => {
                self.hostname.pop();
            }
            8 if self.cloudflare_enabled => {
                self.origin_url.pop();
            }
            9 => {
                self.cloudflare_token.pop();
            }
            _ => {}
        }
    }

    fn to_values(&self) -> AppValues {
        AppValues {
            xp_bin: self.xp_bin.clone(),
            node_name: self.node_name.clone(),
            public_domain: self.public_domain.clone(),
            cloudflare_enabled: self.cloudflare_enabled,
            account_id: if self.cloudflare_enabled {
                Some(self.account_id.clone()).filter(|s| !s.trim().is_empty())
            } else {
                None
            },
            zone_id: if self.cloudflare_enabled {
                Some(self.zone_id.clone()).filter(|s| !s.trim().is_empty())
            } else {
                None
            },
            hostname: if self.cloudflare_enabled {
                Some(self.hostname.clone()).filter(|s| !s.trim().is_empty())
            } else {
                None
            },
            origin_url: if self.cloudflare_enabled {
                Some(self.origin_url.clone()).filter(|s| !s.trim().is_empty())
            } else {
                None
            },
            api_base_url: if self.cloudflare_enabled {
                None
            } else {
                Some(self.api_base_url.clone()).filter(|s| !s.trim().is_empty())
            },
            xray_version: self.xray_version.clone(),
            cloudflare_token: self.cloudflare_token.clone(),
            save_token: self.save_token,
            enable_services: self.enable_services,
            dry_run: self.dry_run,
        }
    }

    fn apply_config(&mut self, cfg: TuiConfig) {
        if let Some(v) = cfg.xp_bin {
            self.xp_bin = v;
        }
        if let Some(v) = cfg.node_name {
            self.node_name = v;
        }
        if let Some(v) = cfg.public_domain {
            self.public_domain = v;
        }
        if let Some(v) = cfg.cloudflare_enabled {
            self.cloudflare_enabled = v;
        }
        if let Some(v) = cfg.account_id {
            self.account_id = v;
        }
        if let Some(v) = cfg.zone_id {
            self.zone_id = v;
        }
        if let Some(v) = cfg.hostname {
            self.hostname = v;
        }
        if let Some(v) = cfg.origin_url {
            self.origin_url = v;
        }
        if let Some(v) = cfg.api_base_url {
            self.api_base_url = v;
        }
        if let Some(v) = cfg.xray_version {
            self.xray_version = v;
        }
        if let Some(v) = cfg.save_token {
            self.save_token = v;
        }
        if let Some(v) = cfg.enable_services {
            self.enable_services = v;
        }
    }
}

fn item(label: &str, value: &str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow)),
        Span::raw(value.to_string()),
    ]))
}

fn mask_token(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    "*".repeat(token.chars().count())
}

fn help_text(app: &App) -> String {
    if app.editing {
        "Mode: EDIT · Enter/Esc: finish · Type: input · Backspace: delete · q: literal".to_string()
    } else {
        let base = "Mode: NAV · Tab/Shift+Tab: focus · Enter: edit/toggle · s: save · S: save+exit · d: deploy · q/Esc: quit";
        if let Some(msg) = app.status_message.as_ref() {
            format!("{base} · Status: {msg}")
        } else {
            base.to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TuiConfig {
    xp_bin: Option<String>,
    node_name: Option<String>,
    public_domain: Option<String>,
    cloudflare_enabled: Option<bool>,
    account_id: Option<String>,
    zone_id: Option<String>,
    hostname: Option<String>,
    origin_url: Option<String>,
    api_base_url: Option<String>,
    xray_version: Option<String>,
    save_token: Option<bool>,
    enable_services: Option<bool>,
}

fn load_tui_config(paths: &Paths) -> Option<TuiConfig> {
    let path = paths.etc_xp_ops_deploy_settings();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return None;
    };
    match serde_json::from_str::<TuiConfig>(&raw) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("warn: invalid TUI config {}: {e}", path.display());
            None
        }
    }
}

fn save_tui_config(paths: &Paths, values: &AppValues) -> Result<(), crate::ops::cli::ExitError> {
    let cfg = TuiConfig {
        xp_bin: Some(values.xp_bin.clone()),
        node_name: Some(values.node_name.clone()),
        public_domain: Some(values.public_domain.clone()),
        cloudflare_enabled: Some(values.cloudflare_enabled),
        account_id: values.account_id.clone(),
        zone_id: values.zone_id.clone(),
        hostname: values.hostname.clone(),
        origin_url: values.origin_url.clone(),
        api_base_url: values.api_base_url.clone(),
        xray_version: Some(values.xray_version.clone()),
        save_token: Some(values.save_token),
        enable_services: Some(values.enable_services),
    };

    let path = paths.etc_xp_ops_deploy_settings();
    ensure_dir(&paths.etc_xp_ops_deploy_dir())
        .map_err(|e| crate::ops::cli::ExitError::new(4, format!("filesystem_error: {e}")))?;
    let content = serde_json::to_string_pretty(&cfg)
        .map_err(|e| crate::ops::cli::ExitError::new(4, format!("filesystem_error: {e}")))?;
    write_string_if_changed(&path, &(content + "\n"))
        .map_err(|e| crate::ops::cli::ExitError::new(4, format!("filesystem_error: {e}")))?;
    chmod(&path, 0o640).ok();
    Ok(())
}

fn save_token_if_needed(
    paths: &Paths,
    values: &AppValues,
) -> Result<(), crate::ops::cli::ExitError> {
    if values.save_token && !values.cloudflare_token.trim().is_empty() {
        cloudflare::set_token_value(paths, &values.cloudflare_token, Mode::Real)?;
    }
    Ok(())
}
