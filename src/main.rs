mod model;
mod protocol;
mod ui;

use std::{
    env, io,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use cmux_client::{ClientConfig, CmuxClient};
use crossterm::{
    event::{self, Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use model::{AgentRow, RecentNotification, SurfaceTarget, UnreadNotification, rows_from_records};
use ratatui::{Terminal, backend::CrosstermBackend};

const REFRESH_EVERY: Duration = Duration::from_secs(2);
const POLL_EVERY: Duration = Duration::from_millis(100);
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_millis(500);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(8);
const MAX_NOTIFICATIONS: usize = 20;

fn main() -> Result<()> {
    // Restore the terminal before the default panic output so a panic never
    // leaves the host terminal (or the cmux sidebar PTY) stuck in raw mode +
    // alternate screen with the message swallowed.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));
    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();
    app.connect_or_schedule();

    loop {
        terminal.draw(|frame| ui::draw(frame, &app.view()))?;

        if event::poll(POLL_EVERY)?
            && let TerminalEvent::Key(key) = event::read()?
            && app.handle_key(key)
        {
            break;
        }

        app.tick();
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionKey {
    Agent(u64),
    Notification(u64),
}

struct App {
    agents: Vec<AgentRow>,
    notifications: Vec<RecentNotification>,
    targets: std::collections::HashMap<u64, SurfaceTarget>,
    selected: usize,
    client: Option<CmuxClient>,
    socket_path: Option<PathBuf>,
    status: Status,
    last_refresh: Instant,
    next_reconnect: Instant,
    reconnect_delay: Duration,
}

#[derive(Debug, Clone)]
enum Status {
    Ready,
    Reconnecting { message: String },
}

impl App {
    fn new() -> Self {
        Self {
            agents: Vec::new(),
            notifications: Vec::new(),
            targets: std::collections::HashMap::new(),
            selected: 0,
            client: None,
            socket_path: None,
            status: Status::Reconnecting {
                message: "connecting".to_string(),
            },
            last_refresh: Instant::now(),
            next_reconnect: Instant::now(),
            reconnect_delay: INITIAL_RECONNECT_DELAY,
        }
    }

    fn view(&self) -> ui::View<'_> {
        ui::View {
            agents: &self.agents,
            notifications: &self.notifications,
            selected: self.selected,
            status: match &self.status {
                Status::Ready => ui::ViewStatus::Ready,
                Status::Reconnecting { message } => ui::ViewStatus::Reconnecting { message },
            },
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return true;
        }

        match key.code {
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-1)
            }
            KeyCode::Down => self.move_selection(1),
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(1)
            }
            KeyCode::Enter => self.activate_selected(),
            KeyCode::Char('r') if key.modifiers.is_empty() => self.refresh(),
            // Esc belongs to cmux's focus flow and must never exit the plugin.
            KeyCode::Esc => {}
            _ => {}
        }
        false
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if self.client.is_none() {
            if now >= self.next_reconnect {
                self.connect_or_schedule();
            }
            return;
        }

        if now.duration_since(self.last_refresh) >= REFRESH_EVERY {
            self.refresh();
        }
    }

    fn connect_or_schedule(&mut self) {
        let socket_path = match env::var_os("CMUX_TUI_SOCKET")
            .or_else(|| env::var_os("CMUX_MUX_SOCKET"))
        {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => {
                self.socket_path = None;
                self.disconnect_with_backoff(
                    "CMUX_TUI_SOCKET is not set. Launch this plugin from cmux, or run standalone with CMUX_TUI_SOCKET=/path/to/cmux-tui.sock cargo run."
                        .to_string(),
                );
                return;
            }
        };

        self.socket_path = Some(socket_path.clone());
        match CmuxClient::connect(ClientConfig::from_socket_path(socket_path)) {
            Ok(mut client) => match client.identify() {
                Ok(_) => {
                    self.client = Some(client);
                    self.status = Status::Ready;
                    self.reconnect_delay = INITIAL_RECONNECT_DELAY;
                    self.refresh();
                }
                Err(err) => self.disconnect_with_backoff(format!("cmux did not respond: {err}")),
            },
            Err(err) => self.disconnect_with_backoff(format!("cannot connect to cmux: {err}")),
        }
    }

    fn refresh(&mut self) {
        let result = match self.client.as_mut() {
            Some(client) => protocol::load_snapshot(client),
            None => return,
        };

        match result {
            Ok(snapshot) => {
                // The list re-sorts on every poll (status priority + recency),
                // so remember WHICH row is selected, not its position.
                let keep = self.selection_key();
                self.targets = snapshot.targets;
                self.agents = rows_from_records(&snapshot.agents, &self.targets, unix_time_ms());
                self.sync_notifications(&snapshot.unread);
                self.last_refresh = Instant::now();
                self.restore_selection(keep);
            }
            Err(err) => self.disconnect(format!("cmux socket dropped: {err}")),
        }
    }

    fn selection_key(&self) -> Option<SelectionKey> {
        if self.selected < self.agents.len() {
            self.agents
                .get(self.selected)
                .map(|row| SelectionKey::Agent(row.surface))
        } else {
            self.notifications
                .get(self.selected - self.agents.len())
                .map(|row| SelectionKey::Notification(row.id))
        }
    }

    fn restore_selection(&mut self, keep: Option<SelectionKey>) {
        let found = keep.and_then(|key| match key {
            SelectionKey::Agent(surface) => {
                self.agents.iter().position(|row| row.surface == surface)
            }
            SelectionKey::Notification(id) => self
                .notifications
                .iter()
                .position(|row| row.id == id)
                .map(|i| self.agents.len() + i),
        });
        match found {
            Some(index) => self.selected = index,
            None => self.clamp_selection(),
        }
    }

    fn sync_notifications(&mut self, unread: &[UnreadNotification]) {
        for notification in &mut self.notifications {
            notification.unread = notification.surface.is_some_and(|surface| {
                unread.iter().any(|item| {
                    item.surface == surface && item.id == notification.id && item.unread
                })
            });
            if let Some(target) = notification
                .surface
                .and_then(|surface| self.targets.get(&surface))
            {
                notification.breadcrumb = target.breadcrumb.clone();
            }
        }

        for item in unread.iter().filter(|item| item.unread) {
            if self
                .notifications
                .iter()
                .any(|notification| notification.id == item.id)
            {
                continue;
            }
            let breadcrumb = self
                .targets
                .get(&item.surface)
                .map(|target| target.breadcrumb.clone())
                .unwrap_or_else(|| format!("surface {}", item.surface));
            self.notifications.push(RecentNotification {
                id: item.id,
                title: format!("{} notification", title_case(&item.level)),
                level: item.level.clone(),
                surface: Some(item.surface),
                breadcrumb,
                unread: true,
                created_at_ms: unix_time_ms(),
            });
        }
        self.sort_and_limit_notifications();
    }

    fn sort_and_limit_notifications(&mut self) {
        self.notifications.sort_by(|left, right| {
            right
                .created_at_ms
                .cmp(&left.created_at_ms)
                .then_with(|| right.id.cmp(&left.id))
        });
        self.notifications.truncate(MAX_NOTIFICATIONS);
    }

    fn activate_selected(&mut self) {
        let surface = if self.selected < self.agents.len() {
            Some(self.agents[self.selected].surface)
        } else {
            self.notifications
                .get(self.selected.saturating_sub(self.agents.len()))
                .and_then(|notification| notification.surface)
        };
        let Some(target) = surface
            .and_then(|surface| self.targets.get(&surface))
            .cloned()
        else {
            return;
        };
        let result = match self.client.as_mut() {
            Some(client) => activate_target(client, &target),
            None => return,
        };
        match result {
            Ok(()) => self.refresh(),
            Err(err) => self.disconnect(format!("cmux command failed: {err}")),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let total = self.agents.len() + self.notifications.len();
        if total == 0 {
            self.selected = 0;
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(total - 1);
    }

    fn clamp_selection(&mut self) {
        let total = self.agents.len() + self.notifications.len();
        self.selected = if total == 0 {
            0
        } else {
            self.selected.min(total - 1)
        };
    }

    fn disconnect(&mut self, message: String) {
        self.client = None;
        self.disconnect_with_backoff(message);
    }

    fn disconnect_with_backoff(&mut self, message: String) {
        self.status = Status::Reconnecting { message };
        self.next_reconnect = Instant::now() + self.reconnect_delay;
        self.reconnect_delay = (self.reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Unread".to_string(),
    }
}

fn activate_target(client: &mut CmuxClient, target: &SurfaceTarget) -> cmux_client::Result<()> {
    client.select_workspace(Some(target.workspace_index), None)?;
    client.select_screen(Some(target.screen_index), None)?;
    client.focus_pane(target.pane)?;
    client.select_tab(Some(target.pane), Some(target.tab_index), None)?;
    Ok(())
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
