use crate::app::{App, Mode, Pane};
use crate::service::Status;

use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

fn status_style(status: &Status) -> Style {
    match status {
        Status::Running => Style::default().fg(Color::Green),
        Status::Starting => Style::default().fg(Color::Yellow),
        Status::Stopping => Style::default().fg(Color::Yellow),
        Status::Stopped => Style::default().fg(Color::DarkGray),
        Status::Failed(_) => Style::default().fg(Color::Red),
    }
}

fn status_dot(status: &Status) -> &str {
    match status {
        Status::Running => "✓",
        Status::Starting => "◐",
        Status::Stopping => "↓",
        Status::Stopped => "○",
        Status::Failed(_) => "✕",
    }
}

fn render_header(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Rudder ");
    frame.render_widget(block, area);

    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });

    let mode_label = match app.mode {
        Mode::Normal => "NORMAL",
        Mode::Command => "COMMAND",
    };

    let focus = match app.focused_pane {
        Pane::Browser => "[Projects]",
        Pane::Services => "[Services]",
    };

    let path = app.browser.current_dir.to_string_lossy();
    let header_text = format!(" {} {}  │  {}", mode_label, focus, path);
    frame.render_widget(Paragraph::new(header_text), inner);
}

fn render_browser(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let focused = app.focused_pane == Pane::Browser;

    let title = if focused { " Projects ▌" } else { " Projects " };
    let mut block = Block::default().borders(Borders::ALL).title(title);
    if focused {
        block = block.border_style(Style::default().fg(Color::Yellow));
    }

    if let Some(ref err) = app.browser.error {
        let para = Paragraph::new(err.as_str())
            .style(Style::default().fg(Color::Red))
            .block(block);
        frame.render_widget(para, area);
        return;
    }

    let items: Vec<ListItem> = app
        .browser
        .entries
        .iter()
        .map(|e| {
            let label = if e.is_dir {
                format!(" {}/", e.name)
            } else {
                format!(" {}", e.name)
            };
            ListItem::new(label)
        })
        .collect();

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );

    let mut state =
        ratatui::widgets::ListState::default().with_selected(Some(app.browser.selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_services(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let focused = app.focused_pane == Pane::Services;

    let areas = Layout::vertical([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).split(area);

    let items: Vec<ListItem> = if app.services.is_empty() {
        vec![ListItem::new(" No services detected.").style(
            Style::default().fg(Color::DarkGray),
        )]
    } else {
        app.services
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let dot = status_dot(&s.status);
                let label = match &s.url {
                    Some(u) => format!(" {}  {}  →  {}", dot, s.name, u),
                    None => format!(" {}  {}", dot, s.name),
                };
                let item = ListItem::new(label).style(status_style(&s.status));
                if focused && i == app.service_selected {
                    item.style(
                        status_style(&s.status)
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    item
                }
            })
            .collect()
    };

    let title = if focused { " Services ▌" } else { " Services " };
    let mut block = Block::default().borders(Borders::ALL).title(title);
    if focused {
        block = block.border_style(Style::default().fg(Color::Yellow));
    }

    let list = List::new(items).block(block);
    frame.render_widget(list, areas[0]);

    if let Some(service) = app.services.get(app.service_selected) {
        let log_text = match service.log.lock() {
            Ok(guard) => {
                let start = guard.len().saturating_sub(200);
                guard[start..].join("\n")
            }
            Err(poisoned) => {
                let inner = poisoned.into_inner();
                let start = inner.len().saturating_sub(200);
                inner[start..].join("\n")
            }
        };
        let line_count = log_text.chars().filter(|&c| c == '\n').count() + 1;
        let visible = (areas[1].height as usize).saturating_sub(2);
        let max_scroll = line_count.saturating_sub(visible);
        let scroll = (max_scroll as i32 - app.log_scroll).max(0) as u16;
        let log_para = Paragraph::new(log_text)
            .block(Block::default().borders(Borders::ALL).title(" Log "))
            .scroll((scroll, 0));
        frame.render_widget(log_para, areas[1]);
    } else {
        let msg = Paragraph::new("No services detected for this directory")
            .block(Block::default().borders(Borders::ALL).title(" Log "));
        frame.render_widget(msg, areas[1]);
    }
}

fn render_command_area(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let project = app
        .browser
        .current_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty() && n != ".")
        .unwrap_or_else(|| "project".to_string());

    let content = match app.mode {
        Mode::Command => format!(":{}", app.command),
        Mode::Normal => {
            if app.services.is_empty() {
                format!(":init —  generate config for  {}", project)
            } else {
                format!(":start / :stop / :cd  —  {}", project)
            }
        }
    };

    let block = Block::default().borders(Borders::ALL);
    let paragraph = Paragraph::new(content).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render(frame: &mut ratatui::Frame, app: &App) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(frame.area());

    render_header(frame, areas[0], app);

    let main_areas =
        Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)]).split(areas[1]);
    render_browser(frame, main_areas[0], app);
    render_services(frame, main_areas[1], app);

    render_command_area(frame, areas[2], app);
}
