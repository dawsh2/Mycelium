use crate::views::{
    arbitrage::ArbitrageView, data_flow::DataFlowView, home::HomeView, pools::PoolsView,
};
use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use mycelium_transport::{config::Topology, MessageBus};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Tabs},
    Frame,
};
use std::sync::Arc;

const ADAPTER_SERVICE: &str = "polygon-adapter";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppTab {
    Home,
    DataFlow,
    Pools,
    Arbitrage,
}

impl AppTab {
    fn title(self) -> &'static str {
        match self {
            AppTab::Home => "Home",
            AppTab::DataFlow => "Data Flow",
            AppTab::Pools => "Pools",
            AppTab::Arbitrage => "Arbitrage",
        }
    }

    fn all() -> &'static [AppTab] {
        &[
            AppTab::Home,
            AppTab::DataFlow,
            AppTab::Pools,
            AppTab::Arbitrage,
        ]
    }
}

pub enum AppAction {
    None,
    Quit,
}

pub struct App {
    tabs: Vec<AppTab>,
    active: usize,
    pub home: HomeView,
    pub data_flow: DataFlowView,
    pub pools: PoolsView,
    pub arbitrage: ArbitrageView,
}

impl App {
    pub async fn new(topology_path: &str) -> Result<Self> {
        let topology = Topology::load(topology_path)
            .with_context(|| format!("Failed to load topology at {}", topology_path))?;
        let bus = Arc::new(MessageBus::from_topology(topology, "polygon-terminal"));

        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let postgres_url = std::env::var("POSTGRES_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/postgres".to_string());
        let log_path = std::env::var("ADAPTER_LOG")
            .unwrap_or_else(|_| "/tmp/bandit/logs/adapter.log".to_string());

        let home = HomeView::new(topology_path, log_path, redis_url, postgres_url).await;
        let data_flow = DataFlowView::new(bus.clone(), ADAPTER_SERVICE).await;
        let pools = PoolsView::new(bus.clone()).await?;
        let arbitrage = ArbitrageView::new(bus, ADAPTER_SERVICE).await;

        Ok(Self {
            tabs: AppTab::all().to_vec(),
            active: 0,
            home,
            data_flow,
            pools,
            arbitrage,
        })
    }

    pub fn render(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(f.size());

        let titles: Vec<Line> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, tab)| {
                let name = tab.title();
                Line::from(Span::styled(
                    format!("{} {}", idx + 1, name),
                    Style::default()
                        .fg(if idx == self.active {
                            Color::Cyan
                        } else {
                            Color::White
                        })
                        .add_modifier(if idx == self.active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ))
            })
            .collect();

        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Bandit Monitor"),
            )
            .select(self.active)
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        f.render_widget(tabs, chunks[0]);

        let content = chunks[1];
        match self.tabs[self.active] {
            AppTab::Home => self.home.render(f, content),
            AppTab::DataFlow => self.data_flow.render(f, content),
            AppTab::Pools => self.pools.render(f, content),
            AppTab::Arbitrage => self.arbitrage.render(f, content),
        }
    }

    pub async fn handle_event(&mut self, event: Event) -> Result<AppAction> {
        match event {
            Event::Key(key) => self.handle_key(key).await,
            _ => Ok(AppAction::None),
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<AppAction> {
        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() {
            return Ok(AppAction::Quit);
        }

        match key.code {
            KeyCode::Right | KeyCode::Tab => self.next_tab(),
            KeyCode::Left => self.prev_tab(),
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if c == 'c' {
                    return Ok(AppAction::Quit);
                }
            }
            _ => {}
        }

        match self.tabs[self.active] {
            AppTab::Home => self.home.handle_key(key).await?,
            AppTab::DataFlow => self.data_flow.handle_key(key).await?,
            AppTab::Pools => self.handle_pools_key(key).await?,
            AppTab::Arbitrage => self.arbitrage.handle_key(key).await?,
        }

        Ok(AppAction::None)
    }

    async fn handle_pools_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Up => self.pools.scroll_up(),
            KeyCode::Down => self.pools.scroll_down(),
            KeyCode::PageUp => self.pools.page_up(),
            KeyCode::PageDown => self.pools.page_down(),
            KeyCode::Char('c') => self.pools.clear().await,
            KeyCode::Char('v') => {
                if let Err(err) = self.pools.start_validation().await {
                    tracing::error!("Pool validation failed: {}", err);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn next_tab(&mut self) {
        self.active = (self.active + 1) % self.tabs.len();
    }

    fn prev_tab(&mut self) {
        if self.active == 0 {
            self.active = self.tabs.len() - 1;
        } else {
            self.active -= 1;
        }
    }

    pub async fn tick(&mut self) {
        self.home.tick().await;
        self.data_flow.tick().await;
        self.arbitrage.tick().await;
    }
}
