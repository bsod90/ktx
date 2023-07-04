use std::sync::Arc;

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kube::config::NamedContext;
use tokio::sync::{mpsc, Mutex};
use tui::{
    backend::Backend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::ui::app::{AppState, AppView, ViewState};
use crate::ui::types::{KtxEvent, KubeContextStatus};
use crate::ui::views::ui_utils::{action_style, key_style};

pub struct ContextListViewState {
    pub list_state: ListState,
    pub remembered_g: bool,
}

impl ContextListViewState {
    pub fn from(state: &mut ViewState) -> &mut Self {
        if let ViewState::ContextListView(state) = state {
            state
        } else {
            panic!("Invalid ViewState passed to ContextListView");
        }
    }
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

    async fn handle_list_navigation(
        &self,
        event: Event,
        state: &AppState,
        view_state: &mut ContextListViewState,
    ) -> Result<(), String> {
        let list_state = &view_state.list_state;
        let filtered_contexts = state.get_filtered_contexts();
        match event {
            Event::Key(KeyEvent {
                code, modifiers, ..
            }) => match (code, modifiers) {
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                    let _ = self.send_event(KtxEvent::ListOneUp).await;
                }
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                    let _ = self.send_event(KtxEvent::ListOneDown).await;
                }
                (KeyCode::PageUp, _) | (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                    let _ = self.send_event(KtxEvent::ListPageUp).await;
                }
                (KeyCode::PageDown, _) | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                    let _ = self.send_event(KtxEvent::ListPageDown).await;
                }
                (KeyCode::Home, _) | (KeyCode::Char('g'), _) => {
                    if (code == KeyCode::Char('g') && view_state.remembered_g)
                        || code == KeyCode::Home
                    {
                        view_state.remembered_g = false;
                        self.send_event(KtxEvent::ListTop).await;
                    } else {
                        view_state.remembered_g = true;
                    }
                }
                (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                    self.send_event(KtxEvent::ListBottom).await;
                }
                (KeyCode::Enter, _) => {
                    if let Some(selected) = list_state.selected() {
                        let name = filtered_contexts[selected].0.name.clone();
                        self.send_event(KtxEvent::SetContext(name)).await;
                    }
                }
                (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                    self.send_event(KtxEvent::PopView).await;
                }
                (KeyCode::Char('d'), _) => {
                    if let Some(selected) = list_state.selected() {
                        let _ = self
                            .send_event(KtxEvent::DeleteContext(
                                filtered_contexts[selected].0.name.clone(),
                            ))
                            .await;
                    }
                }
                (KeyCode::Char('t'), _) => {
                    self.send_event(KtxEvent::TestConnections).await;
                }
                _ => {
                    view_state.remembered_g = false;
                }
            },
            _ => {}
        };
        Ok(())
    }

    async fn handle_list_navigation_event(
        &self,
        event: KtxEvent,
        state: &AppState,
        view_state: &mut ContextListViewState,
    ) -> Result<(), String> {
        let filtered_contexts = state.get_filtered_contexts();
        let list_state = &mut view_state.list_state;
        match event {
            KtxEvent::ListSelect(pos) => {
                list_state.select(Some(pos));
            }
            KtxEvent::ListOneUp => {
                if let Some(current_selection) = list_state.selected() {
                    if current_selection > 0 {
                        list_state.select(Some(current_selection - 1));
                    }
                }
            }
            KtxEvent::ListOneDown => {
                if let Some(current_selection) = list_state.selected() {
                    if current_selection < filtered_contexts.len() - 1 {
                        list_state.select(Some(current_selection + 1));
                    }
                }
            }
            KtxEvent::ListPageUp => {
                if let Some(current_selection) = list_state.selected() {
                    if current_selection > 0 {
                        let new_selection = current_selection.saturating_sub(10);
                        list_state.select(Some(new_selection));
                    }
                }
            }
            KtxEvent::ListPageDown => {
                if let Some(current_selection) = list_state.selected() {
                    if current_selection < filtered_contexts.len() - 1 {
                        let new_selection =
                            usize::min(current_selection + 10, filtered_contexts.len() - 1);
                        list_state.select(Some(new_selection));
                    }
                }
            }
            KtxEvent::ListTop => {
                list_state.select(Some(0));
            }
            KtxEvent::ListBottom => {
                list_state.select(Some(filtered_contexts.len().saturating_sub(1)));
            }
            _ => {}
        };
        Ok(())
    }

    async fn handle_app_event(
        &self,
        event: KtxEvent,
        state: &AppState,
        view_state: &mut ContextListViewState,
    ) -> Result<(), String> {
        match event {
            KtxEvent::ListSelect(_)
            | KtxEvent::ListOneUp
            | KtxEvent::ListOneDown
            | KtxEvent::ListPageUp
            | KtxEvent::ListPageDown
            | KtxEvent::ListTop
            | KtxEvent::ListBottom => {
                let _ = self
                    .handle_list_navigation_event(event, state, view_state)
                    .await;
            }
            _ => {}
        };
        Ok(())
    }

    fn render_context(
        &self,
        c: &(NamedContext, KubeContextStatus),
        state: &AppState,
        view_state: &mut ContextListViewState,
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

    fn draw_top_bar(&self, _state: &AppState) -> Paragraph<'_> {
        Paragraph::new(Line::from(vec![
            key_style("jk".to_string()),
            action_style(" - up/down, ".to_string()),
            key_style("Enter".to_string()),
            action_style(" - select, ".to_string()),
            key_style("Esc".to_string()),
            action_style(" - quit, ".to_string()),
            key_style("t".to_string()),
            action_style(" - test, ".to_string()),
            key_style("d".to_string()),
            action_style(" - delete, ".to_string()),
            key_style("i".to_string()),
            action_style(" - import".to_string()),
        ]))
    }

    fn draw(&self, f: &mut Frame<B>, area: Rect, state: &AppState, view_state: &mut ViewState) {
        let view_state = ContextListViewState::from(view_state);
        let items: Vec<ListItem> = state
            .get_filtered_contexts()
            .iter()
            .map(|c| self.render_context(c, state, view_state, &area))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Contexts"))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray),
            )
            .highlight_symbol("> ");

        f.render_stateful_widget(list, area, &mut view_state.list_state);
    }

    async fn handle_event(
        &self,
        event: KtxEvent,
        state: &AppState,
        view_state: &mut ViewState,
    ) -> Result<(), String> {
        let view_state = ContextListViewState::from(view_state);
        match event {
            KtxEvent::TerminalEvent(evt) => {
                let _ = self.handle_list_navigation(evt, state, view_state).await;
            }
            _ => {
                let _ = self.handle_app_event(event, state, view_state).await;
            }
        };
        Ok(())
    }
}
