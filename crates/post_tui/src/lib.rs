use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use post_core::{NodeMap, PostConfig, PostError, Result};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct App {
    pub should_quit: bool,
    pub nodes: Arc<RwLock<NodeMap>>,
    pub last_clipboard: Arc<RwLock<String>>,
    pub status: Arc<RwLock<AppStatus>>,
    pub config: PostConfig,
}

#[derive(Debug, Clone)]
pub enum AppStatus {
    Connecting,
    Connected { node_count: usize },
    Syncing,
    Error(String),
}

impl App {
    pub fn new(config: PostConfig) -> Self {
        Self {
            should_quit: false,
            nodes: Arc::new(RwLock::new(NodeMap::new())),
            last_clipboard: Arc::new(RwLock::new(String::new())),
            status: Arc::new(RwLock::new(AppStatus::Connecting)),
            config,
        }
    }

    pub async fn update_nodes(&self, nodes: NodeMap) {
        let mut current_nodes = self.nodes.write().await;
        *current_nodes = nodes.clone();

        let mut status = self.status.write().await;
        *status = AppStatus::Connected {
            node_count: nodes.len(),
        };
    }

    pub async fn update_clipboard(&self, content: String) {
        let mut clipboard = self.last_clipboard.write().await;
        *clipboard = content;

        let mut status = self.status.write().await;
        *status = AppStatus::Syncing;

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let nodes = self.nodes.read().await;
        *status = AppStatus::Connected {
            node_count: nodes.len(),
        };
    }

    pub async fn set_error(&self, error: String) {
        let mut status = self.status.write().await;
        *status = AppStatus::Error(error);
    }
}

pub async fn run_tui(app: Arc<App>) -> Result<()> {
    enable_raw_mode().map_err(|e| PostError::Other(format!("Failed to enable raw mode: {}", e)))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .map_err(|e| PostError::Other(format!("Failed to setup terminal: {}", e)))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| PostError::Other(format!("Failed to create terminal: {}", e)))?;

    let res = run_app(&mut terminal, app).await;

    disable_raw_mode()
        .map_err(|e| PostError::Other(format!("Failed to disable raw mode: {}", e)))?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .map_err(|e| PostError::Other(format!("Failed to cleanup terminal: {}", e)))?;
    terminal
        .show_cursor()
        .map_err(|e| PostError::Other(format!("Failed to show cursor: {}", e)))?;

    res
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: Arc<App>) -> Result<()> {
    loop {
        {
            let app_clone = app.clone();
            terminal
                .draw(|f| {
                    tokio::task::block_in_place(|| {
                        let rt = tokio::runtime::Handle::current();
                        rt.block_on(async {
                            draw_ui(f, &app_clone).await;
                        });
                    });
                })
                .map_err(|e| PostError::Other(format!("Failed to draw: {}", e)))?;
        }

        if event::poll(std::time::Duration::from_millis(100))
            .map_err(|e| PostError::Other(format!("Failed to poll events: {}", e)))?
        {
            if let Event::Key(key) = event::read()
                .map_err(|e| PostError::Other(format!("Failed to read event: {}", e)))?
            {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Esc => break,
                        KeyCode::Char('r') => {
                            let mut status = app.status.write().await;
                            *status = AppStatus::Connecting;
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

async fn draw_ui(f: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.size());

    draw_header(f, chunks[0], app).await;
    draw_main_content(f, chunks[1], app).await;
    draw_footer(f, chunks[2]);
}

async fn draw_header(f: &mut Frame<'_>, area: Rect, app: &App) {
    let status = app.status.read().await;
    let (status_text, status_color) = match &*status {
        AppStatus::Connecting => ("Connecting...", Color::Yellow),
        AppStatus::Connected { node_count: _ } => ("Connected", Color::Green),
        AppStatus::Syncing => ("Syncing...", Color::Yellow),
        AppStatus::Error(err) => (err.as_str(), Color::Red),
    };

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled("Post Clipboard Sync - ", Style::default()),
        Span::styled(
            status_text,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
    ])])
    .block(Block::default().borders(Borders::ALL).title("Status"));

    f.render_widget(header, area);
}

async fn draw_main_content(f: &mut Frame<'_>, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    draw_nodes_list(f, chunks[0], app).await;
    draw_clipboard_content(f, chunks[1], app).await;
}

async fn draw_nodes_list(f: &mut Frame<'_>, area: Rect, app: &App) {
    let nodes = app.nodes.read().await;
    let items: Vec<ListItem> = nodes
        .iter()
        .map(|(_id, node)| {
            let age = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(node.last_seen);

            let status_indicator = if age < 30 {
                Span::styled("●", Style::default().fg(Color::Green))
            } else if age < 120 {
                Span::styled("●", Style::default().fg(Color::Yellow))
            } else {
                Span::styled("●", Style::default().fg(Color::Red))
            };

            ListItem::new(Line::from(vec![
                status_indicator,
                Span::raw(" "),
                Span::raw(&node.name),
                Span::styled(format!(" ({}s)", age), Style::default().fg(Color::Gray)),
            ]))
        })
        .collect();

    let nodes_list = List::new(items).block(Block::default().borders(Borders::ALL).title("Nodes"));

    f.render_widget(nodes_list, area);
}

async fn draw_clipboard_content(f: &mut Frame<'_>, area: Rect, app: &App) {
    let clipboard = app.last_clipboard.read().await;
    let content = if clipboard.is_empty() {
        "No clipboard content".to_string()
    } else {
        let preview = if clipboard.len() > 500 {
            format!("{}...", &clipboard[..500])
        } else {
            clipboard.clone()
        };
        preview
    };

    let clipboard_widget = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Last Clipboard"),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(clipboard_widget, area);
}

fn draw_footer(f: &mut Frame<'_>, area: Rect) {
    let footer = Paragraph::new("Press 'q' or 'Esc' to quit, 'r' to reconnect")
        .block(Block::default().borders(Borders::ALL).title("Controls"));

    f.render_widget(footer, area);
}
