use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEvent};
use tokio::sync::mpsc;
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
    Frame,
};

use crate::ui::{app::AppState, AppView, KtxEvent};

use super::ui_utils::{action_style, key_style};

#[derive(Clone, Copy, Debug)]
pub enum ConfirmationDialogState {
    Confirm,
    Reject,
    None,
}

pub struct ConfirmationDialogView {
    event_bus_tx: mpsc::Sender<KtxEvent>,
    content: String,
    on_confirm_event: KtxEvent,
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
        }
    }

    async fn toggle_state(&self, state: &mut AppState, default: ConfirmationDialogState) {
        state.confirmation_selection = match state.confirmation_selection {
            ConfirmationDialogState::Confirm => ConfirmationDialogState::Reject,
            ConfirmationDialogState::Reject => ConfirmationDialogState::Confirm,
            _ => default,
        }
    }

    async fn accept(&self, state: &mut AppState) {
        state.confirmation_selection = ConfirmationDialogState::None;
        let _ = self.event_bus_tx.send(self.on_confirm_event.clone()).await;
        let _ = self.event_bus_tx.send(KtxEvent::DialogConfirm).await;
    }

    async fn reject(&self, state: &mut AppState) {
        state.confirmation_selection = ConfirmationDialogState::None;
        let _ = self.event_bus_tx.send(KtxEvent::DialogReject).await;
    }
}

#[async_trait]
impl<B> AppView<B> for ConfirmationDialogView
where
    B: Backend + Sync + Send,
{
    fn draw_top_bar(&self, _state: &mut AppState) -> Paragraph<'_> {
        Paragraph::new(Line::from(vec![
            key_style("y".to_string()),
            action_style(" - yes, ".to_string()),
            key_style("Esc, n".to_string()),
            action_style(" - no, ".to_string()),
        ]))
    }

    fn draw(&self, f: &mut Frame<B>, area: Rect, state: &mut AppState) {
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

        //TODO: This part is ugly, refactor it

        let button_style = Style::default().fg(Color::Gray);
        let selected_button_style = Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::REVERSED);

        let (yes, no) = match state.confirmation_selection {
            ConfirmationDialogState::Confirm => (
                Span::styled("Yes", selected_button_style),
                Span::styled("No", button_style),
            ),
            ConfirmationDialogState::Reject => (
                Span::styled("Yes", button_style),
                Span::styled("No", selected_button_style),
            ),
            _ => (
                Span::styled("Yes", button_style),
                Span::styled("No", button_style),
            ),
        };

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

    async fn handle_event(&self, event: KtxEvent, state: &mut AppState) -> Result<(), String> {
        match event {
            KtxEvent::TerminalEvent(evt) => match evt {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('y'),
                    ..
                }) => {
                    self.accept(state).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Esc | KeyCode::Char('n'),
                    ..
                }) => {
                    self.reject(state).await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Left | KeyCode::Char('h'),
                    ..
                }) => {
                    self.toggle_state(state, ConfirmationDialogState::Confirm)
                        .await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Right | KeyCode::Char('l'),
                    ..
                }) => {
                    self.toggle_state(state, ConfirmationDialogState::Reject)
                        .await;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => match state.confirmation_selection {
                    ConfirmationDialogState::Confirm => {
                        self.accept(state).await;
                    }
                    ConfirmationDialogState::Reject => {
                        self.reject(state).await;
                    }
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }
}
