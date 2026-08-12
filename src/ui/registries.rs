//! 左ペイン: コンテナレジストリ一覧。

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table};

use super::{DIM, SAKURA, border_style};
use crate::app::{App, Focus, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.registry.focus == Focus::Registries;
    let count = app.registry.registries.ready().map_or(0, Vec::len);
    let title = if count > 0 {
        format!(" レジストリ ({count}) ")
    } else {
        " レジストリ ".to_string()
    };
    let block = Block::bordered()
        .title(Span::styled(
            title,
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    match &app.registry.registries {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "読み込み中…",
                    Style::default().fg(DIM),
                )))
                .block(block),
                area,
            );
        }
        Loadable::Failed(err) => {
            frame.render_widget(
                Paragraph::new(err.as_str())
                    .style(Style::default().fg(ratatui::style::Color::Red))
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .block(block),
                area,
            );
        }
        Loadable::Ready(items) if items.is_empty() => {
            frame.render_widget(
                Paragraph::new("コンテナレジストリがありません")
                    .style(Style::default().fg(DIM))
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .block(block),
                area,
            );
        }
        Loadable::Ready(items) => {
            let rows: Vec<Row> = items
                .iter()
                .map(|registry| {
                    Row::new(vec![
                        Cell::from(registry.name.clone()),
                        Cell::from(Span::styled(
                            registry.host().to_string(),
                            Style::default().fg(DIM),
                        )),
                    ])
                })
                .collect();

            let table = Table::new(
                rows,
                [Constraint::Percentage(45), Constraint::Percentage(55)],
            )
            .header(
                Row::new(vec!["名前", "ホスト"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(if focused {
                Style::default()
                    .fg(SAKURA)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            })
            .highlight_symbol("")
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.registry.registry_state);
        }
    }
}
