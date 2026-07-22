use std::env;

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy)]
pub struct Theme {
    pub fg: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub muted: Color,
    pub border: Color,
    pub selected_bg: Color,
}

impl Theme {
    pub fn detect() -> Self {
        if env::var_os("NO_COLOR").is_some() || env::var("TERM").is_ok_and(|v| v == "dumb") {
            return Self::monochrome();
        }
        Self {
            fg: Color::Rgb(211, 215, 218),
            accent: Color::Rgb(249, 248, 248),
            success: Color::Rgb(110, 158, 134),
            warning: Color::Rgb(196, 154, 91),
            danger: Color::Rgb(203, 93, 86),
            muted: Color::Rgb(115, 116, 117),
            border: Color::Rgb(48, 50, 54),
            selected_bg: Color::Rgb(19, 23, 27),
        }
    }

    pub fn monochrome() -> Self {
        Self {
            fg: Color::Reset,
            accent: Color::White,
            success: Color::White,
            warning: Color::White,
            danger: Color::White,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            selected_bg: Color::Reset,
        }
    }

    pub fn selected(self) -> Style {
        Style::default()
            .fg(self.accent)
            .bg(self.selected_bg)
            .add_modifier(Modifier::BOLD)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::detect()
    }
}
