//! サーバー画面: 一覧と選択中サーバーの詳細。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};

use super::{DIM, accent, border_style, field, format_datetime, placeholder};
use crate::app::{App, Loadable};
use crate::iaas::PowerStatus;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    if let Loadable::Failed(err) = app.visible_servers() {
        super::draw_full_width_error(frame, area, "サーバー", &err);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(62),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    draw_list(frame, chunks[0], app);
    draw_detail(frame, chunks[2], app);
}

/// 電源状態の色。起動中は緑、停止は灰、処理中は黄。
fn power_style(power: PowerStatus) -> Style {
    match power {
        PowerStatus::Up => Style::default().fg(Color::Green),
        PowerStatus::Down => Style::default().fg(DIM),
        PowerStatus::Cleaning => Style::default().fg(Color::Yellow),
        PowerStatus::Unknown => Style::default().fg(Color::Magenta),
    }
}

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let servers = app.visible_servers();
    let count = servers.ready().map_or(0, Vec::len);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" サーバー — {} ({count}) ", app.zone),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(true));

    match &servers {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area)
        }
        Loadable::Failed(err) => frame.render_widget(
            Paragraph::new(err.clone())
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        ),
        Loadable::Ready(items) if items.is_empty() => frame.render_widget(
            placeholder("このゾーンにサーバーがありません（z でゾーンを切り替え）").block(block),
            area,
        ),
        Loadable::Ready(items) => {
            let rows: Vec<Row> = items
                .iter()
                .map(|server| {
                    Row::new(vec![
                        Cell::from(Span::styled(
                            server.power.label(),
                            power_style(server.power),
                        )),
                        Cell::from(server.name.clone()),
                        Cell::from(Span::styled(
                            format!("{}c / {:.0}GB", server.cpu, server.memory_gb()),
                            Style::default().fg(DIM),
                        )),
                        Cell::from(Span::styled(
                            server.ip_addresses.first().cloned().unwrap_or_default(),
                            Style::default().fg(DIM),
                        )),
                    ])
                })
                .collect();
            let table = Table::new(
                rows,
                [
                    Constraint::Length(8),
                    Constraint::Min(12),
                    Constraint::Length(14),
                    Constraint::Length(16),
                ],
            )
            .header(
                Row::new(vec!["電源", "名前", "プラン", "IPアドレス"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.server.server_state);
        }
    }
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" サーバーの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let Some(server) = app.selected_server() else {
        frame.render_widget(placeholder("サーバーを選択してください").block(block), area);
        return;
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(super::pad("電源", 14), Style::default().fg(DIM)),
            Span::styled(
                server.power.label(),
                power_style(server.power).add_modifier(Modifier::BOLD),
            ),
        ]),
        field("名前", &server.name),
        field("ID", &server.id.to_string()),
        field("ホスト名", &server.host_name),
        field("ゾーン", &server.zone),
        field(
            "プラン",
            &format!(
                "{} ({} コア / {:.0} GB)",
                server.plan_name,
                server.cpu,
                server.memory_gb()
            ),
        ),
        field("状態", &server.availability),
    ];
    for (i, ip) in server.ip_addresses.iter().enumerate() {
        lines.push(field(&format!("IP {}", i + 1), ip));
    }
    for (i, disk) in server.disk_names.iter().enumerate() {
        lines.push(field(&format!("ディスク {}", i + 1), disk));
    }
    if !server.description.is_empty() {
        lines.push(field("説明", &server.description));
    }
    if !server.tags.is_empty() {
        lines.push(field("タグ", &server.tags.join(", ")));
    }
    if let Some(created) = &server.created_at {
        lines.push(field("作成日時", &format_datetime(created)));
    }

    if let Some(ip) = server.ip_addresses.first() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  ssh root@{ip}"),
            Style::default().fg(Color::Cyan),
        )));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}
