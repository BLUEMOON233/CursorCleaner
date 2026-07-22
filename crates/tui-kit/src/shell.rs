use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::Theme;

pub struct ShellAreas {
    pub body: Rect,
}

pub fn draw_shell(
    frame: &mut Frame<'_>,
    title: &str,
    page: &str,
    status: &str,
    footer: &str,
    theme: Theme,
) -> ShellAreas {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" · {page}"), Style::default().fg(theme.muted)),
        Span::styled(format!("  {status}"), Style::default().fg(theme.muted)),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border)),
    );
    frame.render_widget(header, chunks[0]);

    let footer = Paragraph::new(footer)
        .style(Style::default().fg(theme.muted))
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.border)),
        );
    frame.render_widget(footer, chunks[2]);

    ShellAreas { body: chunks[1] }
}
