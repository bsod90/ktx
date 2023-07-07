use crate::ui::types::ViewState;
use crate::ui::views::confirmation::ConfirmationDialogView;
use crate::ui::views::list::ContextListView;
use crate::ui::{KtxEvent, KubeContextStatus, RendererMessage};
use async_trait::async_trait;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use futures::stream::StreamExt;
use k8s_openapi::apimachinery::pkg::version::Info;
use kube::config::{KubeConfigOptions, Kubeconfig, NamedContext};
use kube::{Client, Config};
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex};
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Color, Style};
use tui::widgets::{Block, Borders, Paragraph, Wrap};
use tui::{backend::Backend, layout::Rect, Frame};

use super::types::EmptyResult;
use super::views::import::ImportView;

pub type DynAppView<B> = Box<dyn AppView<B> + Send + Sync>;
pub type HandleEventResult = Result<Option<KtxEvent>, Box<dyn Error + Send + Sync>>;

#[async_trait]
pub trait AppView<B>
where
    B: Backend + Sync + Send,
{
    fn draw(&self, f: &mut Frame<B>, area: Rect, state: &AppState, view_state: &mut ViewState);
    fn draw_top_bar(&self, state: &AppState) -> Paragraph<'_>;
    async fn handle_event(&self, event: KtxEvent, state: &AppState) -> HandleEventResult;
    fn get_state_mutex(&self) -> Arc<Mutex<ViewState>>;
    async fn update_filter(&self, _filter: String) {}
    async fn get_filter(&self) -> String {
        "".to_string()
    }
}

#[derive(Debug)]
struct ConnectionError {}

impl Error for ConnectionError {}

impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Connection is Unhealthy")
    }
}

#[derive(Debug, Clone)]
enum UiMessage {
    Error(String),
    Info(String),
    Success(String),
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub is_filter_on: bool,
    pub kubeconfig: Kubeconfig,
    pub kubeconfig_path: String,
    pub connectivity_status: std::collections::HashMap<String, KubeContextStatus>,
    pub config_lock: Arc<Mutex<()>>,
    last_message: Option<UiMessage>,
    last_message_timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

pub struct KtxApp<B: Backend + Send + Sync> {
    state: Arc<Mutex<AppState>>,
    view_stack: Arc<Mutex<Vec<DynAppView<B>>>>,
    event_bus_tx: mpsc::Sender<KtxEvent>,
    terminal: Mutex<tui::Terminal<B>>,
}

impl AppState {
    pub fn get_filtered_contexts(&self, filter: &str) -> Vec<(NamedContext, KubeContextStatus)> {
        let kubeconfig = &self.kubeconfig;
        let connectivity_status = &self.connectivity_status;
        let mut filtered_contexts = Vec::new();
        for context in &kubeconfig.contexts {
            if context
                .name
                .to_lowercase()
                .contains(filter.to_lowercase().as_str())
            {
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
                is_filter_on: false,
                kubeconfig_path,
                connectivity_status: std::collections::HashMap::new(),
                kubeconfig,
                last_message: None,
                last_message_timestamp: None,
                config_lock: Arc::new(Mutex::new(())),
            })),
            event_bus_tx,
            view_stack: Arc::new(Mutex::new(Vec::new())),
            terminal: Mutex::new(terminal),
        }
    }

    pub async fn start(&self) {
        let mut view_stack = self.view_stack.lock().await;
        view_stack.push(Box::new(ContextListView::new::<B>(
            self.event_bus_tx.clone(),
        )));
    }

    async fn test_connections(&self, state: &AppState) -> EmptyResult {
        let kubeconfig = state.kubeconfig.clone();
        let contexts = state.kubeconfig.contexts.clone();
        let event_bus = self.event_bus_tx.clone();
        tokio::spawn(async move {
            let mut handles: Vec<_> = vec![];
            for context in contexts {
                let kubeconfig = kubeconfig.clone();
                let event_bus = event_bus.clone();
                let context = context.clone();
                let handle = tokio::spawn(async move {
                    let name = context.name.clone();
                    let options = KubeConfigOptions {
                        context: Some(name.clone()),
                        cluster: None,
                        user: None,
                    };
                    let status = match async {
                        let config = Config::from_custom_kubeconfig(kubeconfig.clone(), &options)
                            .await
                            .map_err(|_| ConnectionError {})?;
                        let client = Client::try_from(config)?;
                        Ok::<Info, Box<dyn Error + Sync + Send>>(client.apiserver_version().await?)
                    }
                    .await
                    {
                        Ok(version) => KtxEvent::SetConnectivityStatus((
                            name,
                            KubeContextStatus::Healthy(format!(
                                "{}.{}",
                                version.major, version.minor
                            )),
                        )),
                        Err(e) => {
                            let _ = event_bus
                                .send(KtxEvent::PushInfoMessage(e.to_string()))
                                .await;
                            KtxEvent::SetConnectivityStatus((name, KubeContextStatus::Unhealthy))
                        }
                    };
                    let _ = event_bus.send(status).await;
                });
                handles.push(handle);
                // Let the eventloop chill for a bit to avoid freezing the UI
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            futures::stream::iter(handles)
                .buffer_unordered(10)
                .collect::<Vec<_>>()
                .await;
        });
        Ok(())
    }

    async fn handle_filter_on_navigation(
        &self,
        code: KeyCode,
        view: &DynAppView<B>,
    ) -> EmptyResult {
        let mut current_filter = view.get_filter().await;
        match code {
            event::KeyCode::Char(c) => {
                current_filter.push(c);
            }
            event::KeyCode::Backspace => {
                current_filter.pop();
            }
            event::KeyCode::Enter | event::KeyCode::Esc => {
                let _ = self.event_bus_tx.send(KtxEvent::ExitFilterMode).await;
            }
            _ => {}
        };
        view.update_filter(current_filter).await;
        Ok(())
    }

    async fn propagate_event(&self, event: KtxEvent, state: &mut AppState) -> HandleEventResult {
        let view_stack = self.view_stack.lock().await;
        let current_view = view_stack.last().unwrap();
        current_view.handle_event(event, state).await
    }

    async fn handle_terminal_event(&self, event: Event, state: &mut AppState) -> EmptyResult {
        // "Inversed" event handling order because filter is technically in focus and should
        // handle events before any other view
        if state.is_filter_on {
            let view_stack = self.view_stack.lock().await;
            let current_view = view_stack.last().unwrap();
            if let Event::Key(key_event) = event {
                self.handle_filter_on_navigation(key_event.code, &current_view)
                    .await?;
            }
        } else {
            self.propagate_event(KtxEvent::TerminalEvent(event), state)
                .await?;
        };
        Ok(())
    }

    async fn handle_app_event(&self, event: KtxEvent, state: &mut AppState) -> EmptyResult {
        if let Some(event) = self.propagate_event(event, state).await? {
            match event {
                KtxEvent::ExitFilterMode => {
                    state.is_filter_on = false;
                }
                KtxEvent::EnterFilterMode => {
                    state.is_filter_on = true;
                }
                KtxEvent::TestConnections => {
                    self.test_connections(state).await?;
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
                KtxEvent::RefreshConfig => {
                    let _config_guard = state.config_lock.lock().await;
                    state.kubeconfig = Kubeconfig::read_from(&state.kubeconfig_path)?;
                }
                KtxEvent::PushErrorMessage(error) => {
                    state.last_message = Some(UiMessage::Error(error));
                    state.last_message_timestamp = Some(chrono::Utc::now());
                }
                KtxEvent::PushInfoMessage(error) => {
                    state.last_message = Some(UiMessage::Info(error));
                    state.last_message_timestamp = Some(chrono::Utc::now());
                }
                KtxEvent::PushSuccessMessage(error) => {
                    state.last_message = Some(UiMessage::Success(error));
                    state.last_message_timestamp = Some(chrono::Utc::now());
                }
                KtxEvent::ShowImportView(path) => {
                    let mut view_stack = self.view_stack.lock().await;
                    let import_view = ImportView::new::<B>(self.event_bus_tx.clone(), path);
                    import_view.load_options().await?;
                    view_stack.push(Box::new(import_view));
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
                    self.write_kubeconfig(state).await?;
                }
                KtxEvent::SetContext(name) => {
                    state.kubeconfig.current_context = Some(name);
                    self.write_kubeconfig(state).await?;
                }
                _ => {}
            };
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
                    let view_filter = current_view.get_filter().await;
                    let state_mutex = current_view.get_state_mutex();
                    let mut view_state = state_mutex.lock().await;
                    let mut terminal = self.terminal.lock().await;
                    terminal
                        .draw(move |f| {
                            self.draw(
                                f,
                                f.size(),
                                &mut state,
                                current_view,
                                &mut view_state,
                                view_filter,
                            )
                        })
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
        current_view: &DynAppView<B>,
        view_filter: String,
    ) {
        if state.is_filter_on {
            let filter_input = Paragraph::new(view_filter)
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
        current_view: &DynAppView<B>,
        view_state: &mut ViewState,
        view_filter: String,
    ) {
        let size = f.size();
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    Constraint::Length(3),
                    Constraint::Min(0),
                    Constraint::Length(2),
                ]
                .as_ref(),
            )
            .split(size);
        self.draw_top_bar(f, layout[0], state, current_view, view_filter);
        current_view.draw(f, layout[1], state, view_state);
        self.draw_error_bar(f, layout[2], state);
    }

    pub fn draw_error_bar(&self, f: &mut Frame<B>, area: Rect, state: &mut AppState) {
        if let (Some(msg), Some(ts)) = (&state.last_message, &state.last_message_timestamp) {
            if *ts + chrono::Duration::seconds(6) > chrono::Utc::now() {
                let error_bar = match msg {
                    UiMessage::Error(msg) => {
                        Paragraph::new(msg.as_str()).style(Style::default().fg(Color::Red))
                    }
                    UiMessage::Info(msg) => {
                        Paragraph::new(msg.as_str()).style(Style::default().fg(Color::DarkGray))
                    }
                    UiMessage::Success(msg) => {
                        Paragraph::new(msg.as_str()).style(Style::default().fg(Color::Green))
                    }
                }
                .wrap(Wrap { trim: true });
                f.render_widget(error_bar, area);
            }
        }
    }

    pub async fn handle_event(&self, event: KtxEvent) {
        let mut state = self.state.lock().await;
        let result = match event {
            KtxEvent::TerminalEvent(evt) => self.handle_terminal_event(evt, &mut state).await,
            _ => self.handle_app_event(event, &mut state).await,
        };
        if let Err(e) = result {
            let _ = self
                .event_bus_tx
                .send(KtxEvent::PushErrorMessage(e.to_string()))
                .await;
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

    async fn write_kubeconfig(&self, state: &mut AppState) -> EmptyResult {
        let _config_guard = state.config_lock.lock().await;
        let serialized_kubeconfig = serde_yaml::to_string(&state.kubeconfig)?;
        let path = Path::new(state.kubeconfig_path.as_str());
        let mut file = fs::File::create(&path).await?;
        file.write_all(serialized_kubeconfig.as_bytes()).await?;
        Ok(())
    }
}
