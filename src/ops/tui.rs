use crate::ops::cli::DeployArgs;
use crate::ops::cloudflare;
use crate::ops::deploy;
use crate::ops::paths::Paths;
use crate::ops::util::Mode;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{event, execute};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

pub async fn cmd_tui(paths: Paths) -> Result<(), crate::ops::cli::ExitError> {
    let mut stdout = io::stdout();
    enable_raw_mode().map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;
    execute!(stdout, EnterAlternateScreen)
        .map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).map_err(|e| crate::ops::cli::ExitError::new(2, format!("{e}")))?;

    let mut app = App::new();
    let outcome = run_loop(&mut terminal, &mut app);

    // Restore terminal.
    disable_raw_mode().ok();
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen).ok();

    match outcome {
        TuiOutcome::Quit => Ok(()),
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
        origin_url: values.origin_url,
        api_base_url: values.api_base_url,
        xray_version: values.xray_version,
        enable_services_toggle: crate::ops::cli::EnableServicesToggle {
            enable_services: values.enable_services,
            no_enable_services: !values.enable_services,
        },
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

        let Ok(Event::Key(key)) = event::read() else {
            continue;
        };
        if let Some(outcome) = app.handle_key(key) {
            return outcome;
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

    let help = Paragraph::new(
        "Tab/Shift+Tab: focus · Enter: toggle/run · Type: edit · Backspace: delete · q/Esc: quit",
    )
    .block(Block::default().borders(Borders::ALL).title("Help"));
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
    RunDeploy(Box<AppValues>),
}

struct App {
    list_state: ListState,
    focus: usize,

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
    fn new() -> Self {
        let mut s = Self {
            list_state: ListState::default(),
            focus: 0,
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
        s.list_state.select(Some(0));
        s
    }

    fn items_len(&self) -> usize {
        14
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

        v.push(item(
            "cloudflare_token",
            if self.cloudflare_token.is_empty() {
                ""
            } else {
                "********"
            },
        ));
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
        v.push(item("RUN", "press Enter"));
        v
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiOutcome> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Some(TuiOutcome::Quit),
            KeyCode::Tab => self.next(),
            KeyCode::BackTab => self.prev(),
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.prev(),
            KeyCode::Enter => {
                if self.focus == self.items_len() - 1 {
                    return Some(TuiOutcome::RunDeploy(Box::new(self.to_values())));
                }
                self.toggle_or_advance();
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

    fn toggle_or_advance(&mut self) {
        match self.focus {
            3 => self.cloudflare_enabled = !self.cloudflare_enabled,
            10 => self.save_token = !self.save_token,
            11 => self.enable_services = !self.enable_services,
            12 => self.dry_run = !self.dry_run,
            _ => self.next(),
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
}

fn item(label: &str, value: &str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().fg(Color::Yellow)),
        Span::raw(value.to_string()),
    ]))
}
