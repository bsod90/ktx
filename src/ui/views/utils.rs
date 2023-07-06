use std::error::Error;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;
use tui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::ui::{app::HandleEventResult, KtxEvent};

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

pub fn styled_list<'a>(label: &str, items: Vec<ListItem<'a>>) -> List<'a> {
    List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(label.to_string()),
        )
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(Color::DarkGray),
        )
        .highlight_symbol("> ")
}

pub async fn handle_list_navigation_keyboard_event(
    event: Event,
    event_bus: mpsc::Sender<KtxEvent>,
    g_mem: &mut bool,
) -> Result<Option<Event>, Box<dyn Error + Send + Sync>> {
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
            (KeyCode::Char('/'), _) => {
                let _ = event_bus.send(KtxEvent::EnterFilterMode).await;
            }
            _ => {
                return Ok(Some(event));
            }
        },
        _ => {
            return Ok(Some(event));
        }
    };
    Ok(None)
}

pub async fn handle_list_navigation_event(
    event: KtxEvent,
    list_state: &mut ListState,
    max_len: usize,
) -> HandleEventResult {
    // Hack: fixup list state if it's out of bounds
    if let Some(current_selection) = list_state.selected() {
        if current_selection >= max_len {
            list_state.select(Some(max_len.saturating_sub(1)));
        }
    }
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
            return Ok(Some(event));
        }
    };
    Ok(None)
}
