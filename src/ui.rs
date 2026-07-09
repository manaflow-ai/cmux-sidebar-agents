use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Clear, Paragraph},
};

use crate::model::{AgentRow, RecentNotification};

pub struct View<'a> {
    pub agents: &'a [AgentRow],
    pub notifications: &'a [RecentNotification],
    pub selected: usize,
    pub status: ViewStatus<'a>,
}

pub enum ViewStatus<'a> {
    Ready,
    Reconnecting { message: &'a str },
}

pub fn draw(frame: &mut Frame<'_>, view: &View<'_>) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    draw_header(frame, chunks[0], view);
    match view.status {
        ViewStatus::Ready => draw_rows(frame, chunks[1], view),
        ViewStatus::Reconnecting { message } => draw_reconnect(frame, chunks[1], message),
    }
    draw_footer(frame, chunks[2]);
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
    let text = format!(
        "Agents {}  Notifications {}",
        view.agents.len(),
        view.notifications.len()
    );
    frame.render_widget(
        Paragraph::new(middle_truncate(&text, area.width as usize))
            .style(Style::new().add_modifier(Modifier::BOLD)),
        area,
    );
}

fn draw_rows(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
    if area.height == 0 {
        return;
    }

    let width = area.width as usize;
    let selected_style = Style::new().add_modifier(Modifier::REVERSED);
    let header_style = Style::new().add_modifier(Modifier::BOLD | Modifier::DIM);
    let mut lines = Vec::new();
    let mut selectable_positions = Vec::new();

    lines.push(Line::styled("AGENTS", header_style));
    if view.agents.is_empty() {
        lines.push(Line::raw(middle_truncate("No agents reported yet", width)));
        lines.push(Line::styled(
            middle_truncate("Agents appear via the report-agent verb.", width),
            Style::new().add_modifier(Modifier::DIM),
        ));
    } else {
        for row in view.agents {
            let position = lines.len();
            selectable_positions.push(position);
            let text = format!(
                "{} {} · {} · {}",
                row.status.glyph(),
                row.name,
                row.breadcrumb,
                row.age
            );
            let style = if selectable_positions.len() - 1 == view.selected {
                selected_style
            } else {
                Style::new()
            };
            lines.push(Line::styled(middle_truncate(&text, width), style));
        }
    }

    if !view.notifications.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::styled("NOTIFICATIONS", header_style));
        for notification in view.notifications {
            let position = lines.len();
            selectable_positions.push(position);
            let glyph = match notification.level.as_str() {
                "error" => "✖",
                "warning" => "⚠",
                _ => "•",
            };
            let text = format!(
                "{glyph} {} · {}",
                notification.title, notification.breadcrumb
            );
            let selectable_index = selectable_positions.len() - 1;
            let mut style = Style::new();
            if notification.unread {
                style = style.add_modifier(Modifier::BOLD);
            }
            if selectable_index == view.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            lines.push(Line::styled(middle_truncate(&text, width), style));
        }
    }

    let selected_position = selectable_positions
        .get(view.selected)
        .copied()
        .unwrap_or_default();
    let offset = scroll_offset(selected_position, area.height as usize, lines.len());
    for (line_index, line) in lines
        .into_iter()
        .skip(offset)
        .take(area.height as usize)
        .enumerate()
    {
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(area.x, area.y + line_index as u16, area.width, 1),
        );
    }
}

fn draw_reconnect(frame: &mut Frame<'_>, area: Rect, message: &str) {
    let lines = [
        "Reconnecting to cmux",
        message,
        "Set CMUX_TUI_SOCKET to the cmux-tui JSON-lines socket path.",
    ];
    for (index, line) in lines.iter().enumerate().take(area.height as usize) {
        frame.render_widget(
            Paragraph::new(middle_truncate(line, area.width as usize)),
            Rect::new(area.x, area.y + index as u16, area.width, 1),
        );
    }
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect) {
    let text = "↑↓/C-j/k move  Enter jump  r refresh  C-c quit";
    frame.render_widget(
        Paragraph::new(middle_truncate(text, area.width as usize))
            .style(Style::new().add_modifier(Modifier::DIM)),
        area,
    );
}

fn scroll_offset(selected: usize, visible_height: usize, total: usize) -> usize {
    if visible_height == 0 || total <= visible_height || selected < visible_height {
        return 0;
    }
    (selected + 1)
        .saturating_sub(visible_height)
        .min(total - visible_height)
}

fn middle_truncate(input: &str, max_chars: usize) -> String {
    let chars = input.chars().collect::<Vec<_>>();
    if chars.len() <= max_chars {
        return input.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let front = keep.div_ceil(2);
    let back = keep / 2;
    chars[..front]
        .iter()
        .chain(['.', '.', '.'].iter())
        .chain(chars[chars.len() - back..].iter())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_the_middle_and_preserves_both_ends() {
        assert_eq!(middle_truncate("abcdefghij", 7), "ab...ij");
        assert_eq!(middle_truncate("abcdef", 3), "...");
        assert_eq!(middle_truncate("abc", 8), "abc");
    }
}
