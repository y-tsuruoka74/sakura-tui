//! 権限画面: このAPIキーで何ができるか。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};

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
    // 列に収まらない値があるので、選択した行だけ下に全文を出す。
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(4)])
        .split(area);
    draw_table(frame, chunks[0], app);
    draw_selected(frame, chunks[1], app);
}

/// 選択中の行の値と説明を、切らずに出す。
fn draw_selected(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" 選択中の項目 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let rows = app.visible_account_rows();
    let selected = app
        .account
        .state
        .selected()
        .and_then(|index| rows.into_iter().nth(index));
    let lines = match selected {
        Some(row) => {
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    format!("{} / {}  ", row.section, row.label),
                    Style::default().fg(DIM),
                ),
                Span::styled(
                    row.value.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ])];
            if !row.note.is_empty() {
                lines.push(Line::from(Span::styled(
                    row.note.clone(),
                    Style::default().fg(if row.warn { Color::Yellow } else { DIM }),
                )));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "項目を選択してください",
            Style::default().fg(DIM),
        ))],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn draw_table(frame: &mut Frame, area: Rect, app: &mut App) {
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
