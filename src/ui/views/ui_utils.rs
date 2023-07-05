use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;
use tui::{
    style::{Color, Modifier, Style},
    text::Span, widgets::ListState,
};

use crate::ui::KtxEvent;

pub fn key_style(s: &str) -> Span<'static> {
    Span::styled(
        s.to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}

pub fn action_style(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default())
}

pub fn styled_button(label: &str, selected: bool) -> Span<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Gray)
    };
    Span::styled(label.to_string(), style)
}

pub async fn handle_list_navigation_keyboard_event(
    event: Event,
    event_bus: mpsc::Sender<KtxEvent>,
    g_mem: &mut bool,
) -> Option<Event> {
    match event {
        Event::Key(KeyEvent {
            code, modifiers, ..
        }) => match (code, modifiers) {
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                let _ = event_bus.send(KtxEvent::ListOneUp).await;
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                let _ = event_bus.send(KtxEvent::ListOneDown).await;
            }
            (KeyCode::PageUp, _) | (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                let _ = event_bus.send(KtxEvent::ListPageUp).await;
            }
            (KeyCode::PageDown, _) | (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                let _ = event_bus.send(KtxEvent::ListPageDown).await;
            }
            (KeyCode::Home, _) | (KeyCode::Char('g'), _) => {
                if (code == KeyCode::Char('g') && *g_mem) || code == KeyCode::Home {
                    *g_mem = false;
                    let _ = event_bus.send(KtxEvent::ListTop).await;
                } else {
                    *g_mem = true;
                }
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                let _ = event_bus.send(KtxEvent::ListBottom).await;
            }
            _ => {
                return Some(event);
            }
        },
        _ => {
            return Some(event);
        }
    };
    None
}

pub async fn handle_list_app_event(
    event: KtxEvent,
    list_state: &mut ListState,
    max_len: usize,
) -> Option<KtxEvent> {
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
                if current_selection < max_len - 1 {
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
                if current_selection < max_len - 1 {
                    let new_selection = usize::min(current_selection + 10, max_len - 1);
                    list_state.select(Some(new_selection));
                }
            }
        }
        KtxEvent::ListTop => {
            list_state.select(Some(0));
        }
        KtxEvent::ListBottom => {
            list_state.select(Some(max_len.saturating_sub(1)));
        }
        _ => {
            return Some(event);
        }
    };
    None
}
