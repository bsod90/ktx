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
