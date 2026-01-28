use crate::ops::cli::DeployArgs;
use crate::ops::deploy;
use crate::ops::paths::Paths;
use crate::ops::util::{chmod, ensure_dir, write_string_if_changed};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{event, execute};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use serde::{Deserialize, Serialize};
use std::io::{self, Stdout};
use std::time::Duration;

pub async fn cmd_tui(paths: Paths) -> Result<(), crate::ops::cli::ExitError> {
    let mut stdout = io::stdout();
    enable_raw_mode().map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;

    let mut app = App::new(&paths);
    let outcome = run_loop(&mut terminal, &mut app);

    // Restore terminal.
    disable_raw_mode().ok();
    let mut stdout = io::stdout();
    execute!(
        stdout,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen
    )
    .ok();

    match outcome {
        TuiOutcome::Quit => Ok(()),
        TuiOutcome::RunDeploy(values) => run_deploy(paths, *values).await,
    }
}

async fn run_deploy(paths: Paths, values: AppValues) -> Result<(), crate::ops::cli::ExitError> {
    let args = DeployArgs {
        xp_bin: None,
        node_name: values.node_name,
        access_host: values.access_host,
        cloudflare_toggle: crate::ops::cli::CloudflareToggle {
            cloudflare: values.cloudflare_enabled,
            no_cloudflare: !values.cloudflare_enabled,
        },
        account_id: values.account_id,
        zone_id: values.zone_id,
        hostname: values.hostname,
        tunnel_name: None,
        origin_url: values.origin_url,
        join_token: None,
        join_token_stdin: false,
        join_token_stdin_value: None,
        cloudflare_token: None,
        cloudflare_token_stdin: false,
        cloudflare_token_stdin_value: None,
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
                if let Some(action) = app.handle_key(key)
                    && let Some(outcome) = run_action(app, action)
                {
                    return outcome;
                }
            }
            Event::Paste(text) => {
                if app.is_editable_field() {
                    app.push_str(&text);
                }
            }
            Event::Mouse(m) => app.handle_mouse(m),
            _ => {}
        }
    }
}

fn run_action(app: &mut App, action: AppAction) -> Option<TuiOutcome> {
    match action {
        AppAction::Quit => return Some(TuiOutcome::Quit),
        AppAction::Save { exit_after } => {
            let values = app.to_values();
            if let Err(e) = save_tui_config(&app.paths, &values) {
                app.status_message = Some(format!("save failed: {}", e.message));
                return None;
            }
            if let Err(e) = save_token_if_needed(&app.paths, &values) {
                app.status_message = Some(format!("save failed: {}", e.message));
                return None;
            }
            app.status_message = Some("saved".to_string());
            app.baseline = app.snapshot();
            if exit_after {
                return Some(TuiOutcome::Quit);
            }
        }
        AppAction::Deploy => {
            let values = app.to_values();
            if let Err(e) = save_tui_config(&app.paths, &values) {
                app.status_message = Some(format!("autosave failed: {}", e.message));
                return None;
            }
            if let Err(e) = save_token_if_needed(&app.paths, &values) {
                app.status_message = Some(format!("autosave failed: {}", e.message));
                return None;
            }
            app.status_message = Some("saved".to_string());
            app.baseline = app.snapshot();
            return Some(TuiOutcome::RunDeploy(Box::new(values)));
        }
    }
    None
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
    app.fields_area = chunks[1];

    let help =
        Paragraph::new(help_text(app)).block(Block::default().borders(Borders::ALL).title("Help"));
    f.render_widget(help, chunks[2]);

    if app.mode == UiMode::ConfirmQuit {
        render_quit_confirm(f);
    }
}

#[derive(Debug, Clone)]
struct AppValues {
    node_name: String,
    access_host: String,
    cloudflare_enabled: bool,
    account_id: Option<String>,
    zone_id: Option<String>,
    hostname: Option<String>,
    origin_url: Option<String>,
    api_base_url: Option<String>,
    xray_version: String,
    cloudflare_token: String,
    enable_services: bool,
    dry_run: bool,
}

enum TuiOutcome {
    Quit,
    RunDeploy(Box<AppValues>),
}

enum AppAction {
    Quit,
    Save { exit_after: bool },
    Deploy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppSnapshot {
    node_name: String,
    access_host: String,
    cloudflare_enabled: bool,
    account_id: String,
    zone_id: String,
    hostname: String,
    origin_url: String,
    api_base_url: String,
    xray_version: String,
    cloudflare_token: String,
    enable_services: bool,
    dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiMode {
    Nav,
    ConfirmQuit,
}

struct App {
    list_state: ListState,
    focus: usize,
    mode: UiMode,
    status_message: Option<String>,
    baseline: AppSnapshot,
    fields_area: Rect,
    paths: Paths,

    node_name: String,
    access_host: String,

    cloudflare_enabled: bool,
    account_id: String,
    zone_id: String,
    hostname: String,
    origin_url: String,
    api_base_url: String,

    xray_version: String,
    cloudflare_token: String,
    enable_services: bool,
    dry_run: bool,
}

impl App {
    fn new(paths: &Paths) -> Self {
        let mut s = Self {
            list_state: ListState::default(),
            focus: 0,
            mode: UiMode::Nav,
            status_message: None,
            baseline: AppSnapshot {
                node_name: String::new(),
                access_host: String::new(),
                cloudflare_enabled: true,
                account_id: String::new(),
                zone_id: String::new(),
                hostname: String::new(),
                origin_url: String::new(),
                api_base_url: String::new(),
                xray_version: String::new(),
                cloudflare_token: String::new(),
                enable_services: true,
                dry_run: false,
            },
            fields_area: Rect::default(),
            paths: paths.clone(),
            node_name: "node-1".to_string(),
            access_host: String::new(),
            cloudflare_enabled: true,
            account_id: String::new(),
            zone_id: String::new(),
            hostname: String::new(),
            origin_url: "http://127.0.0.1:62416".to_string(),
            api_base_url: String::new(),
            xray_version: "latest".to_string(),
            cloudflare_token: String::new(),
            enable_services: true,
            dry_run: false,
        };
        if let Some(cfg) = load_tui_config(paths) {
            s.apply_config(cfg);
        }
        s.list_state.select(Some(0));
        s.baseline = s.snapshot();
        s
    }

    fn items_len(&self) -> usize {
        11
    }

    fn render_items(&self) -> Vec<ListItem<'static>> {
        let mut v = Vec::new();
        v.push(item("node_name", &self.node_name));
        v.push(item("access_host", &self.access_host));

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

    fn handle_key(&mut self, key: KeyEvent) -> Option<AppAction> {
        if self.mode == UiMode::ConfirmQuit {
            return self.handle_quit_confirm_key(key);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('s' | 'S') => Some(AppAction::Save { exit_after: false }),
                KeyCode::Char('d' | 'D') => Some(AppAction::Deploy),
                KeyCode::Char('q' | 'Q') => {
                    if self.is_dirty() {
                        self.mode = UiMode::ConfirmQuit;
                        None
                    } else {
                        Some(AppAction::Quit)
                    }
                }
                _ => None,
            };
        }

        match key.code {
            KeyCode::Tab => self.next(),
            KeyCode::BackTab => self.prev(),
            KeyCode::Down => self.next(),
            KeyCode::Up => self.prev(),
            KeyCode::Enter | KeyCode::Char(' ') => self.handle_toggle(),
            KeyCode::Backspace => {
                if self.is_editable_field() {
                    self.backspace();
                }
            }
            KeyCode::Char(c) => {
                if self.is_editable_field() {
                    self.push_char(c);
                }
            }
            _ => {}
        }
        self.list_state.select(Some(self.focus));
        None
    }

    fn handle_quit_confirm_key(&mut self, key: KeyEvent) -> Option<AppAction> {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return match key.code {
                KeyCode::Char('s' | 'S') => Some(AppAction::Save { exit_after: true }),
                KeyCode::Char('q' | 'Q') => Some(AppAction::Quit),
                _ => None,
            };
        }

        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.mode = UiMode::Nav,
            _ => {}
        }
        None
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        if self.mode == UiMode::ConfirmQuit {
            return;
        }

        let position = Position::new(mouse.column, mouse.row);
        if !self.fields_area.contains(position) {
            return;
        }

        // List has a border, so the first item starts at y+1.
        let inner_top = self.fields_area.y.saturating_add(1);
        let inner_bottom = self
            .fields_area
            .y
            .saturating_add(self.fields_area.height.saturating_sub(1));
        if mouse.row < inner_top || mouse.row >= inner_bottom {
            return;
        }

        let relative_y = mouse.row.saturating_sub(inner_top) as usize;
        let idx = self.list_state.offset().saturating_add(relative_y);
        if idx >= self.items_len() {
            return;
        }

        self.focus = idx;
        self.list_state.select(Some(self.focus));
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

    fn handle_toggle(&mut self) {
        match self.focus {
            2 => self.cloudflare_enabled = !self.cloudflare_enabled,
            9 => self.enable_services = !self.enable_services,
            10 => self.dry_run = !self.dry_run,
            _ => {}
        }
    }

    fn is_editable_field(&self) -> bool {
        match self.focus {
            0..=1 => true,
            3 => !self.cloudflare_enabled,
            4..=7 => self.cloudflare_enabled,
            8 => true,
            _ => false,
        }
    }

    fn push_char(&mut self, c: char) {
        match self.focus {
            0 => self.node_name.push(c),
            1 => self.access_host.push(c),
            3 if !self.cloudflare_enabled => self.api_base_url.push(c),
            4 if self.cloudflare_enabled => self.account_id.push(c),
            5 if self.cloudflare_enabled => self.zone_id.push(c),
            6 if self.cloudflare_enabled => self.hostname.push(c),
            7 if self.cloudflare_enabled => self.origin_url.push(c),
            8 => self.cloudflare_token.push(c),
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
                self.node_name.pop();
            }
            1 => {
                self.access_host.pop();
            }
            3 if !self.cloudflare_enabled => {
                self.api_base_url.pop();
            }
            4 if self.cloudflare_enabled => {
                self.account_id.pop();
            }
            5 if self.cloudflare_enabled => {
                self.zone_id.pop();
            }
            6 if self.cloudflare_enabled => {
                self.hostname.pop();
            }
            7 if self.cloudflare_enabled => {
                self.origin_url.pop();
            }
            8 => {
                self.cloudflare_token.pop();
            }
            _ => {}
        }
    }

    fn to_values(&self) -> AppValues {
        AppValues {
            node_name: self.node_name.clone(),
            access_host: self.access_host.clone(),
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
            enable_services: self.enable_services,
            dry_run: self.dry_run,
        }
    }

    fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            node_name: self.node_name.clone(),
            access_host: self.access_host.clone(),
            cloudflare_enabled: self.cloudflare_enabled,
            account_id: self.account_id.clone(),
            zone_id: self.zone_id.clone(),
            hostname: self.hostname.clone(),
            origin_url: self.origin_url.clone(),
            api_base_url: self.api_base_url.clone(),
            xray_version: self.xray_version.clone(),
            cloudflare_token: self.cloudflare_token.clone(),
            enable_services: self.enable_services,
            dry_run: self.dry_run,
        }
    }

    fn is_dirty(&self) -> bool {
        self.snapshot() != self.baseline
    }

    fn apply_config(&mut self, cfg: TuiConfig) {
        if let Some(v) = cfg.node_name {
            self.node_name = v;
        }
        if let Some(v) = cfg.access_host {
            self.access_host = v;
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
    let base = match app.mode {
        UiMode::Nav => {
            "Tab/Shift+Tab/↑/↓/Click: focus · Type/Backspace/Paste: edit · Space/Enter: toggle · Ctrl+S: save · Ctrl+D: autosave+deploy · Ctrl+Q: quit (asks to save if dirty)"
        }
        UiMode::ConfirmQuit => {
            "Mode: CONFIRM QUIT · Ctrl+S: save+exit · Ctrl+Q: exit without save · Esc/Enter: cancel"
        }
    };
    if let Some(msg) = app.status_message.as_ref() {
        format!("{base} · Status: {msg}")
    } else {
        base.to_string()
    }
}

fn render_quit_confirm(f: &mut Frame) {
    let area = centered_rect(60, 30, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Unsaved changes");
    let msg = Paragraph::new(
        "You have unsaved changes.\n\nCtrl+S: save and exit\nCtrl+Q: exit without saving\nEsc/Enter: cancel",
    )
    .block(block);
    f.render_widget(msg, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TuiConfig {
    node_name: Option<String>,
    #[serde(alias = "public_domain")]
    access_host: Option<String>,
    cloudflare_enabled: Option<bool>,
    account_id: Option<String>,
    zone_id: Option<String>,
    hostname: Option<String>,
    origin_url: Option<String>,
    api_base_url: Option<String>,
    xray_version: Option<String>,
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
        node_name: Some(values.node_name.clone()),
        access_host: Some(values.access_host.clone()),
        cloudflare_enabled: Some(values.cloudflare_enabled),
        account_id: values.account_id.clone(),
        zone_id: values.zone_id.clone(),
        hostname: values.hostname.clone(),
        origin_url: values.origin_url.clone(),
        api_base_url: values.api_base_url.clone(),
        xray_version: Some(values.xray_version.clone()),
        enable_services: Some(values.enable_services),
    };

    let path = paths.etc_xp_ops_deploy_settings();
    ensure_dir(&paths.etc_xp_ops_deploy_dir()).map_err(|e| {
        crate::ops::cli::ExitError::new(
            4,
            format!(
                "filesystem_error: ensure dir {}: {e}",
                paths.etc_xp_ops_deploy_dir().display()
            ),
        )
    })?;
    let content = serde_json::to_string_pretty(&cfg)
        .map_err(|e| crate::ops::cli::ExitError::new(4, format!("filesystem_error: {e}")))?;
    write_string_if_changed(&path, &(content + "\n")).map_err(|e| {
        crate::ops::cli::ExitError::new(
            4,
            format!("filesystem_error: write {}: {e}", path.display()),
        )
    })?;
    chmod(&path, 0o640).ok();
    Ok(())
}

fn save_token_if_needed(
    paths: &Paths,
    values: &AppValues,
) -> Result<(), crate::ops::cli::ExitError> {
    let token = values.cloudflare_token.trim();
    if token.is_empty() {
        // Keep existing token unchanged when input is empty.
        return Ok(());
    }

    let token_dir = paths.etc_xp_ops_cloudflare_dir();
    ensure_dir(&token_dir).map_err(|e| {
        crate::ops::cli::ExitError::new(
            4,
            format!("filesystem_error: ensure dir {}: {e}", token_dir.display()),
        )
    })?;

    let token_path = paths.etc_xp_ops_cloudflare_token();
    write_string_if_changed(&token_path, token).map_err(|e| {
        crate::ops::cli::ExitError::new(
            4,
            format!("filesystem_error: write {}: {e}", token_path.display()),
        )
    })?;
    chmod(&token_path, 0o600).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_paths() -> (tempfile::TempDir, Paths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        (tmp, paths)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_q_exits_when_not_dirty() {
        let (_tmp, paths) = test_paths();
        let mut app = App::new(&paths);
        let action = app.handle_key(ctrl('q'));
        assert!(matches!(action, Some(AppAction::Quit)));
    }

    #[test]
    fn ctrl_q_enters_confirm_quit_when_dirty() {
        let (_tmp, paths) = test_paths();
        let mut app = App::new(&paths);
        app.node_name.push('x');

        let action = app.handle_key(ctrl('q'));
        assert!(action.is_none());
        assert_eq!(app.mode, UiMode::ConfirmQuit);
    }

    #[test]
    fn confirm_quit_cancel_returns_to_nav() {
        let (_tmp, paths) = test_paths();
        let mut app = App::new(&paths);
        app.node_name.push('x');
        let _ = app.handle_key(ctrl('q'));
        assert_eq!(app.mode, UiMode::ConfirmQuit);

        let action = app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(action.is_none());
        assert_eq!(app.mode, UiMode::Nav);
    }

    #[test]
    fn confirm_quit_ctrl_s_means_save_and_exit() {
        let (_tmp, paths) = test_paths();
        let mut app = App::new(&paths);
        app.node_name.push('x');
        let _ = app.handle_key(ctrl('q'));

        let action = app.handle_key(ctrl('s'));
        assert!(matches!(action, Some(AppAction::Save { exit_after: true })));
    }

    #[test]
    fn plain_q_does_not_quit() {
        let (_tmp, paths) = test_paths();
        let mut app = App::new(&paths);
        let action = app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(action.is_none());
        assert_eq!(app.mode, UiMode::Nav);
    }

    #[test]
    fn save_tui_config_omits_legacy_save_token_field() {
        let (tmp, paths) = test_paths();
        let values = AppValues {
            node_name: "node-1".to_string(),
            access_host: "node-1.example.net".to_string(),
            cloudflare_enabled: true,
            account_id: Some("acc".to_string()),
            zone_id: Some("zone".to_string()),
            hostname: Some("node-1.example.com".to_string()),
            origin_url: Some("http://127.0.0.1:62416".to_string()),
            api_base_url: None,
            xray_version: "latest".to_string(),
            cloudflare_token: String::new(),
            enable_services: true,
            dry_run: false,
        };

        save_tui_config(&paths, &values).unwrap();
        let raw = fs::read_to_string(tmp.path().join("etc/xp-ops/deploy/settings.json")).unwrap();
        assert!(raw.contains("\"node_name\""));
        assert!(!raw.contains("save_token"));
    }

    #[test]
    fn load_tui_config_supports_public_domain_alias() {
        let (tmp, paths) = test_paths();
        let p = tmp.path().join("etc/xp-ops/deploy/settings.json");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, r#"{ "public_domain": "node-1.example.net" }"#).unwrap();

        let app = App::new(&paths);
        assert_eq!(app.access_host, "node-1.example.net");
    }

    #[test]
    fn save_token_empty_keeps_existing_token_unchanged() {
        let (tmp, paths) = test_paths();
        let token_path = tmp.path().join("etc/xp-ops/cloudflare_tunnel/api_token");
        fs::create_dir_all(token_path.parent().unwrap()).unwrap();
        fs::write(&token_path, "oldtoken").unwrap();

        let values = AppValues {
            node_name: String::new(),
            access_host: String::new(),
            cloudflare_enabled: true,
            account_id: None,
            zone_id: None,
            hostname: None,
            origin_url: None,
            api_base_url: None,
            xray_version: "latest".to_string(),
            cloudflare_token: String::new(),
            enable_services: true,
            dry_run: false,
        };
        save_token_if_needed(&paths, &values).unwrap();

        let raw = fs::read_to_string(token_path).unwrap();
        assert_eq!(raw, "oldtoken");
    }

    #[test]
    fn save_token_non_empty_writes_trimmed_value() {
        let (tmp, paths) = test_paths();
        let token_path = tmp.path().join("etc/xp-ops/cloudflare_tunnel/api_token");
        fs::create_dir_all(token_path.parent().unwrap()).unwrap();
        fs::write(&token_path, "oldtoken").unwrap();

        let values = AppValues {
            node_name: String::new(),
            access_host: String::new(),
            cloudflare_enabled: true,
            account_id: None,
            zone_id: None,
            hostname: None,
            origin_url: None,
            api_base_url: None,
            xray_version: "latest".to_string(),
            cloudflare_token: " newtoken \n".to_string(),
            enable_services: true,
            dry_run: false,
        };
        save_token_if_needed(&paths, &values).unwrap();

        let raw = fs::read_to_string(token_path).unwrap();
        assert_eq!(raw, "newtoken");
    }

    #[test]
    fn save_tui_config_error_includes_deploy_dir() {
        let (tmp, paths) = test_paths();
        let deploy_dir = tmp.path().join("etc/xp-ops/deploy");
        fs::create_dir_all(deploy_dir.parent().unwrap()).unwrap();
        fs::write(&deploy_dir, "not a dir").unwrap();

        let values = AppValues {
            node_name: String::new(),
            access_host: String::new(),
            cloudflare_enabled: true,
            account_id: None,
            zone_id: None,
            hostname: None,
            origin_url: None,
            api_base_url: None,
            xray_version: "latest".to_string(),
            cloudflare_token: String::new(),
            enable_services: true,
            dry_run: false,
        };
        let err = save_tui_config(&paths, &values).unwrap_err();
        assert_eq!(err.code, 4);
        assert!(err.message.contains("ensure dir"));
        assert!(err.message.contains("etc/xp-ops/deploy"));
    }

    #[test]
    fn save_token_error_includes_token_dir() {
        let (tmp, paths) = test_paths();
        let token_dir = tmp.path().join("etc/xp-ops/cloudflare_tunnel");
        fs::create_dir_all(token_dir.parent().unwrap()).unwrap();
        fs::write(&token_dir, "not a dir").unwrap();

        let values = AppValues {
            node_name: String::new(),
            access_host: String::new(),
            cloudflare_enabled: true,
            account_id: None,
            zone_id: None,
            hostname: None,
            origin_url: None,
            api_base_url: None,
            xray_version: "latest".to_string(),
            cloudflare_token: "tok".to_string(),
            enable_services: true,
            dry_run: false,
        };
        let err = save_token_if_needed(&paths, &values).unwrap_err();
        assert_eq!(err.code, 4);
        assert!(err.message.contains("ensure dir"));
        assert!(err.message.contains("etc/xp-ops/cloudflare_tunnel"));
    }
}
