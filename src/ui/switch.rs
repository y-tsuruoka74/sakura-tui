//! スイッチ画面: 一覧と選択中スイッチの詳細。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};

use super::{DIM, accent, border_style, field, format_datetime, placeholder};
use crate::app::{App, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    if let Loadable::Failed(err) = app.visible_switches() {
        super::draw_full_width_error(frame, area, "スイッチ", &err);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    draw_list(frame, chunks[0], app);
    draw_detail(frame, chunks[1], app);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let switches = app.visible_switches();
    let count = switches.ready().map_or(0, Vec::len);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" スイッチ — {} ({count}) ", app.zone),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(true));

    match &switches {
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
            placeholder("このゾーンにスイッチがありません（z でゾーンを切り替え）").block(block),
            area,
        ),
        Loadable::Ready(items) => {
            let rows = items.iter().map(|switch| {
                Row::new(vec![
                    Cell::from(switch.name.clone()),
                    Cell::from(Span::styled(
                        switch.server_count.to_string(),
                        Style::default().fg(DIM),
                    )),
                    Cell::from(Span::styled(
                        switch.appliance_count.to_string(),
                        Style::default().fg(DIM),
                    )),
                    Cell::from(Span::styled(
                        switch.id.to_string(),
                        Style::default().fg(DIM),
                    )),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Min(16),
                    Constraint::Length(8),
                    Constraint::Length(12),
                    Constraint::Length(14),
                ],
            )
            .header(
                Row::new(vec!["名前", "サーバー", "アプライアンス", "ID"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.switch.switch_state);
        }
    }
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" スイッチの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let Some(switch) = app.selected_switch() else {
        frame.render_widget(placeholder("スイッチを選択してください").block(block), area);
        return;
    };

    let mut lines = vec![
        field("名前", &switch.name),
        field("ID", &switch.id.to_string()),
        field("ゾーン", &switch.zone),
        field("接続サーバー", &switch.server_count.to_string()),
        field("接続アプライアンス", &switch.appliance_count.to_string()),
    ];
    if !switch.description.is_empty() {
        lines.push(field("説明", &switch.description));
    }
    if !switch.tags.is_empty() {
        lines.push(field("タグ", &switch.tags.join(", ")));
    }
    if let Some(created) = &switch.created_at {
        lines.push(field("作成日時", &format_datetime(created)));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "y: スイッチIDをコピー",
        Style::default().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}
