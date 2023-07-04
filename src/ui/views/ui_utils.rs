use tui::{
    style::{Color, Modifier, Style},
    text::Span,
};

pub fn key_style(s: String) -> Span<'static> {
    Span::styled(
        s,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}

pub fn action_style(s: String) -> Span<'static> {
    Span::styled(s, Style::default())
}

pub fn styled_button(label: &str, selected: bool) -> Span<'static> {
    let style = if selected {
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Gray)
    };
    Span::styled(String::from(label), style)
}
