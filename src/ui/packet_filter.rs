//! パケットフィルタ画面: フィルタ一覧とルール一覧。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};

use super::{DIM, accent, border_style, placeholder};
use crate::app::{App, ListFocus, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    if let Loadable::Failed(err) = app.visible_packet_filters() {
        super::draw_full_width_error(frame, area, "パケットフィルタ", &err);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(38),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    draw_filters(frame, chunks[0], app);
    draw_rules(frame, chunks[2], app);
}

fn draw_filters(frame: &mut Frame, area: Rect, app: &mut App) {
    let filters = app.visible_packet_filters();
    let count = filters.ready().map_or(0, Vec::len);
    let focused = app.packet_filter.focus == ListFocus::Left;
    let block = Block::bordered()
        .title(Span::styled(
            format!(" パケットフィルタ — {} ({count}) ", app.zone),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    match &filters {
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
            placeholder("このゾーンにパケットフィルタがありません（z でゾーンを切り替え）")
                .block(block),
            area,
        ),
        Loadable::Ready(items) => {
            let rows = items.iter().map(|filter| {
                Row::new(vec![
                    Cell::from(filter.name.clone()),
                    Cell::from(Span::styled(
                        format!("{} ルール", filter.rules.len()),
                        Style::default().fg(DIM),
                    )),
                ])
            });
            let table = Table::new(rows, [Constraint::Min(16), Constraint::Length(10)])
                .header(
                    Row::new(vec!["名前", "ルール"])
                        .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
                )
                .row_highlight_style(highlight(focused))
                .block(block);
            frame.render_stateful_widget(table, area, &mut app.packet_filter.filter_state);
        }
    }
}

fn draw_rules(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.packet_filter.focus == ListFocus::Right;
    let name = app
        .selected_packet_filter()
        .map(|f| f.name)
        .unwrap_or_default();
    let rules = app.visible_packet_filter_rules();
    let block = Block::bordered()
        .title(Span::styled(
            if name.is_empty() {
                " ルール ".to_string()
            } else {
                format!(" ルール — {name} ({}) ", rules.len())
            },
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    if app.selected_packet_filter().is_none() {
        frame.render_widget(
            placeholder("パケットフィルタを選択してください").block(block),
            area,
        );
        return;
    }
    if rules.is_empty() {
        frame.render_widget(
            placeholder(
                "ルールがありません。この状態ではすべての通信が通ります。\n\
                 Tab でこちらに移り、書き込みモードで n を押すと追加できます。",
            )
            .block(block),
            area,
        );
        return;
    }

    let rows = rules.iter().enumerate().map(|(i, rule)| {
        // 拒否は目立たせる。上から順に評価されるので、順番も出す。
        let action_style = if rule.is_allow() {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        Row::new(vec![
            Cell::from(Span::styled((i + 1).to_string(), Style::default().fg(DIM))),
            Cell::from(rule.protocol.clone()),
            Cell::from(rule.source()),
            Cell::from(rule.destination()),
            Cell::from(Span::styled(rule.action.clone(), action_style)),
            Cell::from(Span::styled(
                rule.description.clone(),
                Style::default().fg(DIM),
            )),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Length(11),
            Constraint::Length(24),
            Constraint::Length(12),
            Constraint::Length(7),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["#", "プロトコル", "送信元", "宛先", "動作", "説明"])
            .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
    )
    .row_highlight_style(highlight(focused))
    .block(block);
    frame.render_stateful_widget(table, area, &mut app.packet_filter.rule_state);
}

/// 選択行の見せ方。見ていない側は控えめにする。
fn highlight(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    }
}
