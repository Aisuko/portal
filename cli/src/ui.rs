use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::log::LogLevel;
use crate::steps::Step;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    draw_header(frame, app, root[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(46), Constraint::Min(0)])
        .split(root[1]);

    draw_menu(frame, app, body[0]);
    draw_log(frame, app, body[1]);
    draw_footer(frame, app, root[2]);

    if let Some(form) = &app.cred_form {
        draw_cred_form(frame, form, area);
    } else if let Some(confirm) = &app.confirm {
        draw_confirm(frame, confirm, area);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled("cluster-ctl  ", Style::default().add_modifier(Modifier::BOLD))];
    for node in &app.config.nodes {
        let dot = match app.node_status.get(&node.name).copied().flatten() {
            Some(true) => Span::styled("\u{25cf} ", Style::default().fg(Color::Green)),
            Some(false) => Span::styled("\u{25cf} ", Style::default().fg(Color::Red)),
            None => Span::styled("\u{25cb} ", Style::default().fg(Color::DarkGray)),
        };
        spans.push(dot);
        spans.push(Span::raw(format!("{}  ", node.name)));
    }
    let title = match app.running {
        Some(step) => format!(" running: {} ", step.title()),
        None => " idle ".to_string(),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    frame.render_widget(Paragraph::new(Line::from(spans)).block(block).wrap(Wrap { trim: true }), area);
}

fn draw_menu(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" steps ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = Step::MENU
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let mut style = Style::default();
            if step.destructive() {
                style = style.fg(Color::Red);
            }
            if Some(*step) == app.running {
                style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
            }
            let prefix = if i == app.selected { "\u{25b8} " } else { "  " };
            ListItem::new(Line::from(Span::styled(format!("{prefix}{}", step.title()), style)))
        })
        .collect();
    frame.render_widget(List::new(items), inner);
}

fn draw_log(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" log (PgUp/PgDn to scroll, End to follow) ");
    let inner_height = area.height.saturating_sub(2) as usize;

    let total = app.log.len();
    let end = total.saturating_sub(app.scroll_up.min(total));
    let start = end.saturating_sub(inner_height.max(1));

    let lines: Vec<Line> = app
        .log
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|l| {
            let color = match l.level {
                LogLevel::Info => Color::Gray,
                LogLevel::Stdout => Color::White,
                LogLevel::Stderr => Color::Yellow,
                LogLevel::Error => Color::Red,
                LogLevel::Success => Color::Green,
            };
            let prefix = if l.node.is_empty() { String::new() } else { format!("[{}] ", l.node) };
            Line::from(Span::styled(format!("{prefix}{}", l.text), Style::default().fg(color)))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let text = if app.cred_form.is_some() {
        "Tab: switch field   Enter: continue (blank = key-only auth / passwordless sudo)"
    } else if app.confirm.is_some() {
        "Type UNINSTALL and press Enter to confirm   Esc: cancel"
    } else {
        "\u{2191}/\u{2193} select   Enter: run   r: refresh status   PgUp/PgDn: scroll log   q: quit"
    };
    frame.render_widget(Paragraph::new(text).style(Style::default().fg(Color::DarkGray)), area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}

fn draw_cred_form(frame: &mut Frame, form: &crate::app::CredForm, area: Rect) {
    let rect = centered(area, 60, 9);
    frame.render_widget(Clear, rect);
    let block = Block::default().borders(Borders::ALL).title(" session credentials ");

    let mask = |s: &str| "*".repeat(s.chars().count());
    let ssh_style = if matches!(form.focus, crate::app::CredField::Ssh) {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let sudo_style = if matches!(form.focus, crate::app::CredField::Sudo) {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };

    let text = vec![
        Line::from("Key auth is tried first for every node. These are only used as a"),
        Line::from("fallback / for sudo — nothing is written to disk. Leave blank to"),
        Line::from("rely on SSH keys and/or NOPASSWD sudo."),
        Line::from(""),
        Line::from(vec![Span::raw("SSH password:  "), Span::styled(mask(&form.ssh_password), ssh_style)]),
        Line::from(vec![Span::raw("sudo password: "), Span::styled(mask(&form.sudo_password), sudo_style)]),
        Line::from(""),
        Line::from("Tab: switch field    Enter: continue"),
    ];
    frame.render_widget(Paragraph::new(text).block(block).alignment(Alignment::Left), rect);
}

fn draw_confirm(frame: &mut Frame, confirm: &crate::app::ConfirmState, area: Rect) {
    let rect = centered(area, 66, 8);
    frame.render_widget(Clear, rect);
    let block = Block::default().borders(Borders::ALL).title(" confirm destructive action ").style(Style::default().fg(Color::Red));
    let text = vec![
        Line::from(format!("This will run: {}", confirm.step.title())),
        Line::from("This removes k3s and all of its data from every master node."),
        Line::from(""),
        Line::from(vec![Span::raw("Type "), Span::styled(crate::app::CONFIRM_WORD, Style::default().add_modifier(Modifier::BOLD)), Span::raw(" to continue:")]),
        Line::from(Span::styled(&confirm.input, Style::default().add_modifier(Modifier::REVERSED))),
    ];
    frame.render_widget(Paragraph::new(text).block(block), rect);
}
