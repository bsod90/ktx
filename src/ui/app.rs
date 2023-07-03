use crate::ui::views::confirmation::ConfirmationDialogState;
use crate::ui::views::confirmation::ConfirmationDialogView;
use crate::ui::views::list::ContextListView;
use crate::ui::{KtxEvent, KubeContextStatus, RendererMessage};
use async_trait::async_trait;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use futures::stream::StreamExt;
use kube::config::{KubeConfigOptions, Kubeconfig, NamedContext};
use kube::{Client, Config};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex};
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Style};
use tui::widgets::{Block, Borders, ListState, Paragraph, Wrap};
use tui::{backend::Backend, layout::Rect, Frame};

#[async_trait]
pub trait AppView<B>
where
    B: Backend + Sync + Send,
{
    fn draw(&self, f: &mut Frame<B>, area: Rect, state: &mut AppState);
    fn draw_top_bar(&self, state: &mut AppState) -> Paragraph<'_>;
    async fn handle_event(&self, event: KtxEvent, state: &mut AppState) -> Result<(), String>;
}

// I couldn't find an elegant way to each view's state across the sync/async boundary.
// Hence, for now, all mutable stuff is combined into one big struct and shared with all views.
// This is not ideal, but it works, and I'll revisit this later.
// The core issue that even handling is async, but view rendering is sync,
// becuse terminal.draw(||) accepts a sync callback.
#[derive(Debug, Clone)]
pub struct AppState {
    // Main view state
    pub is_filter_on: bool,
    pub filter: String,
    pub kubeconfig: Kubeconfig,
    pub kubeconfig_path: String,
    pub connectivity_status: std::collections::HashMap<String, KubeContextStatus>,

    // Context list view state
    pub main_list_state: ListState,
    pub remembered_g: bool,

    // Confirmation dialog state
    pub confirmation_selection: ConfirmationDialogState,
}

pub struct KtxApp<B: Backend + Send + Sync> {
    state: Arc<Mutex<AppState>>,
    view_stack: Arc<Mutex<Vec<Box<dyn AppView<B> + Send + Sync>>>>,
    event_bus_tx: mpsc::Sender<KtxEvent>,
    terminal: Mutex<tui::Terminal<B>>,
}

impl AppState {
    pub fn get_filtered_contexts(&self) -> Vec<(NamedContext, KubeContextStatus)> {
        let kubeconfig = &self.kubeconfig;
        let connectivity_status = &self.connectivity_status;
        let mut filtered_contexts = Vec::new();
        for context in &kubeconfig.contexts {
            if context.name.contains(&self.filter) {
                let status = connectivity_status
                    .get(&context.name)
                    .unwrap_or(&KubeContextStatus::Unknown);
                filtered_contexts.push((context.clone(), status.clone()));
            }
        }
        return filtered_contexts;
    }

    pub fn is_current_context(&self, context: &NamedContext) -> bool {
        if let Some(current_context_name) = &self.kubeconfig.current_context {
            return context.name == *current_context_name;
        }
        false
    }
}

impl<B> KtxApp<B>
where
    B: Backend + Send + Sync,
{
    pub fn new(
        kubeconfig_path: String,
        terminal: tui::Terminal<B>,
        event_bus_tx: mpsc::Sender<KtxEvent>,
    ) -> Self {
        let kubeconfig =
            Kubeconfig::read_from(&kubeconfig_path).expect("Unable to read kubeconfig");
        Self {
            state: Arc::new(Mutex::new(AppState {
                filter: String::new(),
                is_filter_on: false,
                kubeconfig_path,
                connectivity_status: std::collections::HashMap::new(),
                kubeconfig,
                main_list_state: ListState::default(),
                remembered_g: false,
                confirmation_selection: ConfirmationDialogState::None,
            })),
            event_bus_tx,
            view_stack: Arc::new(Mutex::new(Vec::new())),
            terminal: Mutex::new(terminal),
        }
    }

    pub async fn start(&self) {
        let mut view_stack = self.view_stack.lock().await;
        self.state.lock().await.main_list_state.select(Some(0));
        view_stack.push(Box::new(ContextListView::new::<B>(
            self.event_bus_tx.clone(),
        )));
    }

    async fn test_connections(&self, state: &AppState) {
        let kubeconfig = state.kubeconfig.clone();
        let contexts = state.kubeconfig.contexts.clone();
        let event_bus = self.event_bus_tx.clone();
        tokio::spawn(async move {
            let handles: Vec<_> = contexts
                .iter()
                .map(|context| {
                    let kubeconfig = kubeconfig.clone();
                    let event_bus = event_bus.clone();
                    let context = context.clone();
                    tokio::spawn(async move {
                        let name = context.name.clone();
                        let options = KubeConfigOptions {
                            context: Some(name.clone()),
                            cluster: None,
                            user: None,
                        };
                        let config = Config::from_custom_kubeconfig(kubeconfig.clone(), &options)
                            .await
                            .unwrap();
                        let client = match Client::try_from(config) {
                            Ok(client) => client,
                            Err(_) => {
                                let _ = event_bus
                                    .send(KtxEvent::SetConnectivityStatus((
                                        name,
                                        KubeContextStatus::Unhealthy,
                                    )))
                                    .await;
                                return;
                            }
                        };
                        match client.apiserver_version().await {
                            Ok(version) => {
                                let _ = event_bus
                                    .send(KtxEvent::SetConnectivityStatus((
                                        name,
                                        KubeContextStatus::Healthy(format!(
                                            "{}.{}",
                                            version.major, version.minor
                                        )),
                                    )))
                                    .await;
                            }
                            Err(_) => {
                                let _ = event_bus
                                    .send(KtxEvent::SetConnectivityStatus((
                                        name,
                                        KubeContextStatus::Unhealthy,
                                    )))
                                    .await;
                            }
                        };
                    })
                })
                .collect();
            futures::stream::iter(handles)
                .buffer_unordered(10)
                .collect::<Vec<_>>()
                .await;
        });
    }

    async fn handle_filter_on_navigation(&self, code: KeyCode, state: &mut AppState) {
        match code {
            event::KeyCode::Char(c) => {
                state.filter.push(c);
            }
            event::KeyCode::Backspace => {
                state.filter.pop();
            }
            event::KeyCode::Enter | event::KeyCode::Esc => {
                let _ = self.event_bus_tx.send(KtxEvent::ExitFilterMode).await;
            }
            _ => {}
        };
    }

    async fn handle_terminal_event(
        &self,
        event: Event,
        state: &mut AppState,
    ) -> Result<(), String> {
        let view_stack = self.view_stack.lock().await;
        let current_view = view_stack.last().unwrap();
        match event {
            Event::Key(KeyEvent { code, .. }) => {
                if state.is_filter_on {
                    self.handle_filter_on_navigation(code, state).await;
                } else {
                    match code {
                        event::KeyCode::Char('/') => {
                            let _ = self.event_bus_tx.send(KtxEvent::EnterFilterMode).await;
                        }
                        _ => {
                            let _ = current_view
                                .handle_event(KtxEvent::TerminalEvent(event), state)
                                .await;
                        }
                    }
                }
            }
            _ => {
                let _ = current_view
                    .handle_event(KtxEvent::TerminalEvent(event), state)
                    .await;
            }
        };
        Ok(())
    }

    async fn handle_app_event(&self, event: KtxEvent, state: &mut AppState) -> Result<(), String> {
        match event {
            KtxEvent::ExitFilterMode => {
                state.is_filter_on = false;
            }
            KtxEvent::EnterFilterMode => {
                state.is_filter_on = true;
            }
            KtxEvent::TestConnections => {
                self.test_connections(state).await;
            }
            KtxEvent::SetConnectivityStatus((name, status)) => {
                state.connectivity_status.insert(name, status);
            }
            KtxEvent::DeleteContext(name) => {
                let mut view_stack = self.view_stack.lock().await;
                view_stack.push(Box::new(ConfirmationDialogView::new::<B>(
                    self.event_bus_tx.clone(),
                    format!(
                        "Are you sure you want to delete\n\n{}\n\nfrom your kubeconfig file?",
                        name
                    ),
                    KtxEvent::DeleteContextConfirm(name),
                )));
            }
            KtxEvent::PopView | KtxEvent::DialogReject | KtxEvent::DialogConfirm => {
                let mut view_stack = self.view_stack.lock().await;
                if view_stack.len() > 1 {
                    view_stack.pop();
                } else {
                    let _ = self.event_bus_tx.send(KtxEvent::Exit).await;
                }
            }
            KtxEvent::DeleteContextConfirm(name) => {
                state.kubeconfig.contexts.retain(|c| c.name != name);
                self.write_kubeconfig(state).await.unwrap();
            }
            KtxEvent::SetContext(name) => {
                state.kubeconfig.current_context = Some(name);
                self.write_kubeconfig(state).await.unwrap();
            }
            _ => {
                let view_stack = self.view_stack.lock().await;
                let _ = view_stack.last().unwrap().handle_event(event, state).await;
            }
        };
        Ok(())
    }

    pub async fn start_renderer(&self, mut rx: mpsc::Receiver<RendererMessage>) -> () {
        enable_raw_mode().expect("Failed to enable raw mode");
        self.terminal
            .lock()
            .await
            .clear()
            .expect("Failed to clear terminal");
        loop {
            match rx.recv().await {
                Some(RendererMessage::Render) => {
                    // Drain all pending render messages.
                    while let Ok(RendererMessage::Render) = rx.try_recv() {
                        // just drain the channel, do nothing with the messages
                    }
                    let mut state = self.state.lock().await;
                    let view_stack = self.view_stack.lock().await;
                    let current_view = view_stack.last().unwrap();
                    let mut terminal = self.terminal.lock().await;
                    terminal
                        .draw(move |f| self.draw(f, f.size(), &mut state, current_view))
                        .expect("Unable to draw terminal");
                }
                Some(RendererMessage::Stop) | None => {
                    break;
                }
            }
        }
    }

    fn draw_top_bar(
        &self,
        f: &mut Frame<B>,
        area: Rect,
        state: &mut AppState,
        current_view: &Box<dyn AppView<B> + Send + Sync>,
    ) {
        if state.is_filter_on {
            let filter_input = Paragraph::new(state.filter.as_str())
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("Filter"))
                .wrap(Wrap { trim: true });
            f.render_widget(filter_input, area);
        } else {
            let top_bar_content = current_view
                .draw_top_bar(state)
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true });
            f.render_widget(top_bar_content, area);
        }
    }

    fn draw(
        &self,
        f: &mut Frame<B>,
        _area: Rect,
        state: &mut AppState,
        current_view: &Box<dyn AppView<B> + Send + Sync>,
    ) {
        let size = f.size();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(size);
        self.draw_top_bar(f, layout[0], state, current_view);
        current_view.draw(f, layout[1], state);
    }

    pub async fn handle_event(&self, event: KtxEvent) -> Result<(), String> {
        let mut state = self.state.lock().await;
        match event {
            KtxEvent::TerminalEvent(evt) => self.handle_terminal_event(evt, &mut state).await,
            _ => self.handle_app_event(event, &mut state).await,
        }
    }

    pub async fn shutdown(&self) {
        self.terminal
            .lock()
            .await
            .clear()
            .expect("Failed to clear terminal");
        disable_raw_mode().expect("Failed to disable raw mode");
    }

    async fn write_kubeconfig(&self, state: &mut AppState) -> Result<(), Box<dyn Error>> {
        let serialized_kubeconfig = serde_yaml::to_string(&state.kubeconfig)?;
        let path = Path::new(state.kubeconfig_path.as_str());
        let mut file = fs::File::create(&path).await?;
        file.write_all(serialized_kubeconfig.as_bytes()).await?;
        Ok(())
    }
}
