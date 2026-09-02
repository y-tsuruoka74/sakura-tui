//! SSH公開鍵画面: 一覧と選択中の鍵の詳細。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};

use super::{DIM, accent, border_style, field, format_datetime, placeholder};
use crate::app::{App, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    if let Loadable::Failed(err) = app.visible_ssh_keys() {
        super::draw_full_width_error(frame, area, "SSH公開鍵", &err);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(52),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    draw_list(frame, chunks[0], app);
    draw_detail(frame, chunks[2], app);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let keys = app.visible_ssh_keys();
    let count = keys.ready().map_or(0, Vec::len);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" SSH公開鍵 ({count}) "),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(true));

    match &keys {
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
            placeholder("登録されている公開鍵がありません（書き込みモードで n）").block(block),
            area,
        ),
        Loadable::Ready(items) => {
            let rows = items.iter().map(|key| {
                Row::new(vec![
                    Cell::from(key.name.clone()),
                    Cell::from(Span::styled(key_type(key), Style::default().fg(DIM))),
                    Cell::from(Span::styled(
                        key.fingerprint.clone(),
                        Style::default().fg(DIM),
                    )),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Min(16),
                    Constraint::Length(14),
                    Constraint::Length(48),
                ],
            )
            .header(
                Row::new(vec!["名前", "種類", "フィンガープリント"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.ssh_key.state);
        }
    }
}

/// 鍵の種類（`ssh-ed25519` など）。取れなければ空。
fn key_type(key: &crate::iaas::SshKey) -> String {
    key.public_key
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" 公開鍵の詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let Some(key) = app.selected_ssh_key() else {
        frame.render_widget(placeholder("公開鍵を選択してください").block(block), area);
        return;
    };

    let mut lines = vec![
        field("名前", &key.name),
        field("ID", &key.id.to_string()),
        field("種類", &key_type(&key)),
    ];
    if !key.description.is_empty() {
        lines.push(field("説明", &key.description));
    }
    if let Some(created) = &key.created_at {
        lines.push(field("登録日時", &format_datetime(created)));
    }
    // ラベルが揃え幅より長いので、値は次の行に置く。
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "フィンガープリント",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(key.fingerprint.clone()));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("公開鍵", Style::default().fg(DIM))));
    lines.push(Line::raw(key.public_key.clone()));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "y: 公開鍵をコピー",
        Style::default().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}
