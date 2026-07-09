//! Read-mode markdown rendering: tui-markdown with a glamour-"dark"-like
//! style sheet (pink headings, warm inline code, cyan links), matching the
//! look the Go TUI got from glamour's "dark" theme.
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use tui_markdown::{Options, StyleSheet};

#[derive(Clone)]
struct GlamourDark;

impl StyleSheet for GlamourDark {
    fn heading(&self, level: u8) -> Style {
        let pink = Color::Rgb(0xff, 0x87, 0xd7);
        match level {
            1 => Style::new()
                .fg(Color::Rgb(0x1d, 0x1d, 0x1d))
                .bg(pink)
                .add_modifier(Modifier::BOLD),
            2 => Style::new().fg(pink).add_modifier(Modifier::BOLD),
            _ => Style::new().fg(pink),
        }
    }

    fn code(&self) -> Style {
        Style::new()
            .fg(Color::Rgb(0xff, 0x5f, 0x87))
            .bg(Color::Rgb(0x2a, 0x2a, 0x2a))
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Gray).add_modifier(Modifier::ITALIC)
    }

    fn heading_meta(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }
}

/// Renders markdown to an owned Text so the app can cache it (tui-markdown
/// returns a Text borrowing the input string).
pub fn render(body: &str) -> Text<'static> {
    owned(tui_markdown::from_str_with_options(
        body,
        &Options::new(GlamourDark),
    ))
}

fn owned(t: Text<'_>) -> Text<'static> {
    let lines = t
        .lines
        .into_iter()
        .map(|l| {
            let spans: Vec<Span<'static>> = l
                .spans
                .into_iter()
                .map(|s| Span::styled(s.content.into_owned(), s.style))
                .collect();
            let mut line = Line::from(spans).style(l.style);
            line.alignment = l.alignment;
            line
        })
        .collect::<Vec<_>>();
    let mut out = Text::from(lines).style(t.style);
    out.alignment = t.alignment;
    out
}
