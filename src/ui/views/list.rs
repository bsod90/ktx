use std::sync::Arc;

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent};
use kube::config::NamedContext;
use tokio::sync::{mpsc, Mutex};
use tui::{
    backend::Backend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{ListItem, ListState, Paragraph},
    Frame,
};

use crate::ui::views::utils::{
    action_style, handle_list_navigation_event, handle_list_navigation_keyboard_event, key_style,
    styled_list,
};
use crate::ui::{
    app::HandleEventResult,
    types::{KtxEvent, KubeContextStatus, ViewState},
};
use crate::ui::{
    app::{AppState, AppView},
    types::CloudImportPath,
};

pub struct ContextListViewState {
    pub list_state: ListState,
    pub remembered_g: bool,
    pub filter: String,
}

pub struct ContextListView {
    event_bus_tx: mpsc::Sender<KtxEvent>,
    state: Arc<Mutex<ViewState>>,
}

const STATUS_PADDING: usize = 10;

impl ContextListView {
    pub fn new<B: Backend>(event_bus_tx: mpsc::Sender<KtxEvent>) -> Self {
        let mut state = ContextListViewState {
            list_state: ListState::default(),
            remembered_g: false,
            filter: "".to_string(),
        };
        state.list_state.select(Some(0));
        Self {
            event_bus_tx,
            state: Arc::new(Mutex::new(ViewState::ContextListView(state))),
        }
    }

    async fn send_event(&self, event: KtxEvent) {
        let _ = self.event_bus_tx.send(event).await;
    }

    async fn handle_keyboard(
        &self,
        event: Event,
        state: &AppState,
        view_state: &mut ContextListViewState,
    ) -> HandleEventResult {
        let list_state = &view_state.list_state;
        let filtered_contexts = state.get_filtered_contexts(view_state.filter.as_str());
        if let Some(event) = handle_list_navigation_keyboard_event(
            event,
            self.event_bus_tx.clone(),
            &mut view_state.remembered_g,
        )
        .await?
        {
            match event {
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) if list_state.selected().is_some() => {
                    let name = filtered_contexts[list_state.selected().unwrap()]
                        .0
                        .name
                        .clone();
                    self.send_event(KtxEvent::SetContext(name)).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Esc | KeyCode::Char('q'),
                    ..
                }) => {
                    self.send_event(KtxEvent::PopView).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('d'),
                    ..
                }) if list_state.selected().is_some() => {
                    let _ = self
                        .send_event(KtxEvent::DeleteContext(
                            filtered_contexts[list_state.selected().unwrap()]
                                .0
                                .name
                                .clone(),
                        ))
                        .await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('t'),
                    ..
                }) => {
                    self.send_event(KtxEvent::TestConnections).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('i'),
                    ..
                }) => {
                    self.send_event(KtxEvent::ShowImportView(CloudImportPath::from(vec![])))
                        .await;
                }
                _ => {
                    view_state.remembered_g = false;
                    return Ok(Some(KtxEvent::TerminalEvent(event)));
                }
            }
        }
        Ok(None)
    }

    async fn handle_app_event(
        &self,
        event: KtxEvent,
        state: &AppState,
        view_state: &mut ContextListViewState,
    ) -> HandleEventResult {
        let filtered_contexts = state.get_filtered_contexts(view_state.filter.as_str());
        let list_state = &mut view_state.list_state;
        handle_list_navigation_event(event, list_state, filtered_contexts.len()).await
    }

    fn render_context(
        &self,
        c: &(NamedContext, KubeContextStatus),
        state: &AppState,
        area: &Rect,
    ) -> ListItem {
        let title = if state.is_current_context(&c.0) {
            Span::styled(
                c.0.name.clone(),
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw(c.0.name.clone())
        };
        let status = match &c.1 {
            KubeContextStatus::Healthy(v) => Span::styled(
                format!("Healthy ({})", v),
                Style::default().fg(Color::Green),
            ),
            KubeContextStatus::Unhealthy => {
                Span::styled("Unhealthy", Style::default().fg(Color::Red))
            }
            KubeContextStatus::Unknown => {
                Span::styled("Unknown", Style::default().fg(Color::DarkGray))
            }
        };
        let spacer_length = area
            .width
            .saturating_sub(title.width() as u16 + status.width() as u16 + STATUS_PADDING as u16);
        let spacer = Span::styled(" ".repeat(spacer_length as usize), Style::default());
        ListItem::new(Line::from(vec![title, spacer, status]))
    }
}

#[async_trait]
impl<B> AppView<B> for ContextListView
where
    B: Backend + Sync + Send,
{
    fn get_state_mutex(&self) -> Arc<Mutex<ViewState>> {
        self.state.clone()
    }

    async fn update_filter(&self, filter: String) {
        let mut state = self.state.lock().await;
        let mut state = ContextListViewState::from_view_state(&mut state);
        state.filter = filter;
    }

    async fn get_filter(&self) -> String {
        let mut state = self.state.lock().await;
        let state = ContextListViewState::from_view_state(&mut state);
        state.filter.clone()
    }

    fn draw_top_bar(&self, _state: &AppState) -> Paragraph<'_> {
        Paragraph::new(Line::from(vec![
            key_style("jk"),
            action_style(" - up/down, "),
            key_style("Enter"),
            action_style(" - select, "),
            key_style("Esc"),
            action_style(" - quit, "),
            key_style("t"),
            action_style(" - test, "),
            key_style("d"),
            action_style(" - delete, "),
            key_style("i"),
            action_style(" - import"),
        ]))
    }

    fn draw(&self, f: &mut Frame<B>, area: Rect, state: &AppState, view_state: &mut ViewState) {
        let view_state = ContextListViewState::from_view_state(view_state);
        let items: Vec<ListItem> = state
            .get_filtered_contexts(view_state.filter.as_str())
            .iter()
            .map(|c| self.render_context(c, state, &area))
            .collect();

        let list = styled_list("Kubernetes config contexts", items);
        f.render_stateful_widget(list, area, &mut view_state.list_state);
    }

    async fn handle_event(&self, event: KtxEvent, state: &AppState) -> HandleEventResult {
        let mut locked_state = self.state.lock().await;
        let view_state = ContextListViewState::from_view_state(&mut locked_state);
        match event {
            KtxEvent::TerminalEvent(evt) => self.handle_keyboard(evt, state, view_state).await,
            _ => self.handle_app_event(event, state, view_state).await,
        }
    }
}
