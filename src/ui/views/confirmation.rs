use std::sync::Arc;

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent};
use tokio::sync::{mpsc, Mutex};
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
    Frame,
};

use crate::ui::{app::AppState, types::ViewState, AppView, KtxEvent};

use super::ui_utils::{action_style, key_style, styled_button};

#[derive(Clone, Copy, Debug)]
pub enum ConfirmationDialogSelection {
    Confirm,
    Reject,
    None,
}

pub struct ConfirmationDialogView {
    event_bus_tx: mpsc::Sender<KtxEvent>,
    content: String,
    on_confirm_event: KtxEvent,
    state: Arc<Mutex<ViewState>>,
}

pub struct ConfirmationDialogViewState {
    pub selection: ConfirmationDialogSelection,
}

impl ConfirmationDialogView {
    pub fn new<B: Backend>(
        event_bus_tx: mpsc::Sender<KtxEvent>,
        content: String,
        on_confirm_event: KtxEvent,
    ) -> Self {
        Self {
            event_bus_tx,
            content,
            on_confirm_event,
            state: Arc::new(Mutex::new(ViewState::ConfirmationDialogView(
                ConfirmationDialogViewState {
                    selection: ConfirmationDialogSelection::None,
                },
            ))),
        }
    }

    async fn toggle_state(
        &self,
        state: &mut ConfirmationDialogViewState,
        default: ConfirmationDialogSelection,
    ) {
        state.selection = match state.selection {
            ConfirmationDialogSelection::Confirm => ConfirmationDialogSelection::Reject,
            ConfirmationDialogSelection::Reject => ConfirmationDialogSelection::Confirm,
            _ => default,
        }
    }

    async fn accept(&self, state: &mut ConfirmationDialogViewState) {
        state.selection = ConfirmationDialogSelection::None;
        let _ = self.event_bus_tx.send(self.on_confirm_event.clone()).await;
        let _ = self.event_bus_tx.send(KtxEvent::DialogConfirm).await;
    }

    async fn reject(&self, state: &mut ConfirmationDialogViewState) {
        state.selection = ConfirmationDialogSelection::None;
        let _ = self.event_bus_tx.send(KtxEvent::DialogReject).await;
    }
}

#[async_trait]
impl<B> AppView<B> for ConfirmationDialogView
where
    B: Backend + Sync + Send,
{
    fn get_state_mutex(&self) -> Arc<Mutex<ViewState>> {
        self.state.clone()
    }

    fn draw_top_bar(&self, _state: &AppState) -> Paragraph<'_> {
        Paragraph::new(Line::from(vec![
            key_style("y"),
            action_style(" - yes, "),
            key_style("Esc, n"),
            action_style(" - no, "),
        ]))
    }

    fn draw(&self, f: &mut Frame<B>, area: Rect, _state: &AppState, view_state: &mut ViewState) {
        let state = ConfirmationDialogViewState::from_view_state(view_state);
        let dialog_width = (area.width as f32 * 0.4) as u16;
        let dialog_height = (area.height as f32 * 0.4) as u16;

        let dialog_left = (area.width - dialog_width) / 2;
        let dialog_top = (area.height - dialog_height) / 2;

        let dialog = Rect::new(dialog_left, dialog_top, dialog_width, dialog_height);

        // Create a layout inside the dialog
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .margin(1) // Add a margin if you want
            .constraints(
                [
                    Constraint::Min(0),    // Content part
                    Constraint::Length(3), // Buttons
                ]
                .as_ref(),
            )
            .split(dialog);

        let (yes_selected, no_selected) = match state.selection {
            ConfirmationDialogSelection::Confirm => (true, false),
            ConfirmationDialogSelection::Reject => (false, true),
            _ => (false, false),
        };

        let yes = styled_button("Yes", yes_selected);
        let no = styled_button("No", no_selected);

        let buttons = Paragraph::new(Line::from(vec![
            yes,
            Span::styled("                                     ", Style::default()),
            no,
        ]))
        .block(Block::default().borders(Borders::ALL))
        .alignment(tui::layout::Alignment::Center);

        let content = Paragraph::new(self.content.as_str())
            .block(
                Block::default()
                    .title("Confirmation")
                    .borders(Borders::ALL)
                    .padding(Padding::new(1, 1, 1, 1)),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(Clear, dialog);
        f.render_widget(content, layout[0]);
        f.render_widget(buttons, layout[1]);
    }

    async fn handle_event(
        &self,
        event: KtxEvent,
        _state: &AppState,
        view_state: &mut ViewState,
    ) -> Option<KtxEvent> {
        let view_state = ConfirmationDialogViewState::from_view_state(view_state);
        match event {
            KtxEvent::TerminalEvent(evt) => match evt {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('y'),
                    ..
                }) => {
                    self.accept(view_state).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Esc | KeyCode::Char('n'),
                    ..
                }) => {
                    self.reject(view_state).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Left | KeyCode::Char('h'),
                    ..
                }) => {
                    self.toggle_state(view_state, ConfirmationDialogSelection::Confirm)
                        .await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Right | KeyCode::Char('l'),
                    ..
                }) => {
                    self.toggle_state(view_state, ConfirmationDialogSelection::Reject)
                        .await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => match view_state.selection {
                    ConfirmationDialogSelection::Confirm => {
                        self.accept(view_state).await;
                    }
                    ConfirmationDialogSelection::Reject => {
                        self.reject(view_state).await;
                    }
                    _ => {
                        return Some(KtxEvent::TerminalEvent(evt));
                    }
                },
                _ => {
                    return Some(KtxEvent::TerminalEvent(evt));
                }
            },
            _ => {
                return Some(event);
            }
        };
        None
    }
}
