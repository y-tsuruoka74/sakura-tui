//! 権限画面: このAPIキーで何ができるか。

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Cell, Row, Table};

use super::{DIM, accent, border_style, error_paragraph, format_datetime, placeholder};
use crate::app::{App, Loadable};

/// 区分ごとの色。表の左端で塊が分かるようにする。
fn section_style(section: &str) -> Style {
    match section {
        "権限" => Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        "使用量" => Style::default().fg(Color::Cyan),
        _ => Style::default().fg(DIM),
    }
}

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let rows = app.visible_account_rows();
    let block = Block::bordered()
        .title(Span::styled(
            " このAPIキーでできること ",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(true));

    match &app.account.status {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area);
            return;
        }
        Loadable::Failed(err) => {
            frame.render_widget(error_paragraph(err).block(block), area);
            return;
        }
        Loadable::Ready(_) if rows.is_empty() => {
            frame.render_widget(placeholder("該当する項目がありません").block(block), area);
            return;
        }
        Loadable::Ready(_) => {}
    }

    let table_rows: Vec<Row> = rows
        .iter()
        .map(|row| {
            // 日時はそのまま出すと読みにくいので整える。
            let value = if row.label == "作成日" {
                format_datetime(&row.value)
            } else {
                row.value.clone()
            };
            Row::new(vec![
                Cell::from(Span::styled(row.section, section_style(row.section))),
                Cell::from(row.label.clone()),
                Cell::from(Span::styled(
                    value,
                    if row.warn {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().add_modifier(Modifier::BOLD)
                    },
                )),
                Cell::from(Span::styled(
                    row.note.clone(),
                    Style::default().fg(if row.warn { Color::Yellow } else { DIM }),
                )),
            ])
        })
        .collect();

    let table = Table::new(
        table_rows,
        [
            Constraint::Length(10),
            Constraint::Length(16),
            Constraint::Length(28),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(vec!["区分", "項目", "値", "説明"])
            .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
    )
    .row_highlight_style(
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    )
    .block(block);
    frame.render_stateful_widget(table, area, &mut app.account.state);
}
