//! 請求画面: 月ごとの請求と、その明細・集計。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Row, Table, Tabs};

use super::{DIM, accent, border_style, error_paragraph, format_datetime, placeholder, yen};
use crate::app::{App, BillingFocus, BillingTab, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    // 請求一覧そのものが取れないときは、案内を読めるよう全幅で出す。
    if let Loadable::Failed(err) = &app.billing.identity {
        super::draw_full_width_error(frame, area, "請求", err);
        return;
    }
    if let Loadable::Failed(err) = &app.billing.bills {
        super::draw_full_width_error(frame, area, "請求", err);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(30),
            Constraint::Length(1),
            Constraint::Min(30),
        ])
        .split(area);

    draw_bills(frame, chunks[0], app);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(chunks[2]);
    draw_tabs(frame, right[0], app);

    match app.billing.tab {
        BillingTab::Details => draw_details(frame, right[1], app),
        _ => draw_summary(frame, right[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = BillingTab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| Line::from(format!("{} {}", i + 1, tab.title())))
        .collect();
    let selected = BillingTab::ALL
        .iter()
        .position(|t| *t == app.billing.tab)
        .unwrap_or(0);
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM)))
            .block(Block::default().padding(ratatui::widgets::Padding::horizontal(1))),
        area,
    );
}

fn draw_bills(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.billing.focus == BillingFocus::Bills;
    let bills: Vec<(String, i64, bool)> = app
        .visible_bills()
        .into_iter()
        .map(|b| {
            // 請求日は「2026-08-31」のうち年月だけ出せば十分。
            let month = b
                .date
                .as_deref()
                .map(format_datetime)
                .map(|d| d.chars().take(7).collect::<String>())
                .unwrap_or_default();
            (month, b.amount, b.paid)
        })
        .collect();

    let block = Block::bordered()
        .title(Span::styled(
            // 年は矢印つきで出して、動かせることが分かるようにする。
            if bills.is_empty() {
                format!(" ← {}年 → ", app.billing_year())
            } else {
                format!(" ← {}年 → ({}) ", app.billing_year(), bills.len())
            },
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    match &app.billing.bills {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area)
        }
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(_) if bills.is_empty() => frame.render_widget(
            placeholder("この年の請求はありません（← → で年を移動）").block(block),
            area,
        ),
        Loadable::Ready(_) => {
            let rows: Vec<Row> = bills
                .iter()
                .map(|(month, amount, paid)| {
                    Row::new(vec![
                        Cell::from(month.clone()),
                        Cell::from(Span::styled(
                            yen(*amount),
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                        // 未払いは目立たせる。
                        Cell::from(Span::styled(
                            if *paid { "済" } else { "未" },
                            if *paid {
                                Style::default().fg(DIM)
                            } else {
                                Style::default().fg(Color::Yellow)
                            },
                        )),
                    ])
                })
                .collect();
            let table = Table::new(
                rows,
                [
                    Constraint::Length(8),
                    Constraint::Min(10),
                    Constraint::Length(3),
                ],
            )
            .header(
                Row::new(vec!["年月", "金額", ""])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.billing.bill_state);
        }
    }
}

fn draw_details(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.billing.focus == BillingFocus::Detail;
    let details = app.visible_bill_details();
    let rows: Vec<Row> = details
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|d| {
                    Row::new(vec![
                        Cell::from(d.description.clone()),
                        Cell::from(Span::styled(d.service_label(), Style::default().fg(DIM))),
                        Cell::from(Span::styled(
                            d.zone.clone(),
                            Style::default().fg(Color::Cyan),
                        )),
                        Cell::from(Span::styled(d.usage.clone(), Style::default().fg(DIM))),
                        Cell::from(Span::styled(
                            yen(d.amount),
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let total: i64 = details
        .ready()
        .map(|items| items.iter().map(|d| d.amount).sum())
        .unwrap_or(0);
    let block = Block::bordered()
        .title(Span::styled(
            if rows.is_empty() {
                " 明細 ".to_string()
            } else {
                format!(" 明細 ({} 件 / 合計 {}) ", rows.len(), yen(total))
            },
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    match &details {
        Loadable::Idle => {
            frame.render_widget(placeholder("請求を選択してください").block(block), area)
        }
        Loadable::Loading => frame.render_widget(placeholder("読み込み中…").block(block), area),
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(_) if rows.is_empty() => {
            frame.render_widget(placeholder("明細がありません").block(block), area)
        }
        Loadable::Ready(_) => {
            let table = Table::new(
                rows,
                [
                    Constraint::Min(16),
                    Constraint::Length(24),
                    Constraint::Length(6),
                    Constraint::Length(12),
                    Constraint::Length(12),
                ],
            )
            .header(
                Row::new(vec!["名前", "種別", "ゾーン", "利用量", "金額"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.billing.detail_state);
        }
    }
}

fn draw_summary(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.billing.focus == BillingFocus::Detail;
    let summary = app.current_summary();
    let total: i64 = summary.iter().map(|(_, amount, _)| amount).sum();
    let heading = match app.billing.tab {
        BillingTab::ByZone => "ゾーン",
        _ => "種別",
    };

    let block = Block::bordered()
        .title(Span::styled(
            if summary.is_empty() {
                format!(" {heading}ごと ")
            } else {
                format!(" {heading}ごと (合計 {}) ", yen(total))
            },
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    if summary.is_empty() {
        let message = match app.current_bill_details() {
            Loadable::Idle => "請求を選択してください",
            Loadable::Loading => "読み込み中…",
            _ => "集計できる明細がありません",
        };
        frame.render_widget(placeholder(message).block(block), area);
        return;
    }

    let rows: Vec<Row> = summary
        .iter()
        .map(|(name, amount, count)| {
            // 全体に占める割合を棒で見せる。
            let ratio = if total > 0 {
                (*amount as f64 / total as f64 * 20.0).round() as usize
            } else {
                0
            };
            Row::new(vec![
                Cell::from(name.clone()),
                Cell::from(Span::styled(
                    format!("{count} 件"),
                    Style::default().fg(DIM),
                )),
                Cell::from(Span::styled(
                    yen(*amount),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    "█".repeat(ratio),
                    Style::default().fg(accent()),
                )),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Min(14),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(21),
        ],
    )
    .header(
        Row::new(vec![heading, "件数", "金額", "割合"])
            .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
    )
    .row_highlight_style(
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    )
    .block(block);
    frame.render_stateful_widget(table, area, &mut app.billing.summary_state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_yen_with_separators() {
        assert_eq!(yen(0), "¥0");
        assert_eq!(yen(980), "¥980");
        assert_eq!(yen(1_234), "¥1,234");
        assert_eq!(yen(128_400), "¥128,400");
        assert_eq!(yen(1_000_000), "¥1,000,000");
    }

    /// 返金などで負になっても壊れないこと。
    #[test]
    fn formats_negative_amounts() {
        assert_eq!(yen(-1_500), "-¥1,500");
    }
}
