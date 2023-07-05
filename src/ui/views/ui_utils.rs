use tui::{
    style::{Color, Modifier, Style},
    text::Span,
};

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
