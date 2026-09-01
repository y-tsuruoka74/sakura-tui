//! ネットワークスイート (CR) の読み取り専用画面。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{DIM, accent, border_style, placeholder};
use crate::app::{App, Loadable, NetworkingSuiteTab};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.networking_suite.tab {
        NetworkingSuiteTab::Groups => draw_groups(frame, chunks[1], app),
        NetworkingSuiteTab::Subnets => draw_subnets(frame, chunks[1], app),
        NetworkingSuiteTab::Addresses => draw_addresses(frame, chunks[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = NetworkingSuiteTab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!("{} {}", index + 1, tab.title())))
        .collect();
    let selected = NetworkingSuiteTab::ALL
        .iter()
        .position(|tab| *tab == app.networking_suite.tab)
        .unwrap_or_default();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM))),
        area,
    );
}

fn draw_groups(frame: &mut Frame, area: Rect, app: &mut App) {
    // 受付ゾーンが固定でゾーン切り替えの対象外なので、問い合わせ先を必ず見せる。
    let title = format!("サブネットグループ — {}", app.networking_suite_zone());
    match app.visible_networking_suite_groups() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let id = item.id();
                    vec![item.name, item.cidr, item.region, item.description, id]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["名前", "アドレス範囲", "リージョン", "説明", "ID"],
                rows,
                [
                    Constraint::Percentage(24),
                    Constraint::Percentage(16),
                    Constraint::Length(10),
                    Constraint::Percentage(24),
                    Constraint::Min(12),
                ],
                &mut app.networking_suite.group_state,
            );
        }
    }
}

fn draw_subnets(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(group) = app.selected_networking_suite_group() else {
        draw_message(
            frame,
            area,
            "サブネット",
            "サブネットグループ タブで選択してください",
        );
        return;
    };
    let title = format!("サブネット — {}", group.name);
    match app.visible_networking_suite_subnets() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let id = item.id();
                    vec![item.name, item.cidr, item.zone, item.description, id]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["名前", "アドレス範囲", "ゾーン", "説明", "ID"],
                rows,
                [
                    Constraint::Percentage(24),
                    Constraint::Percentage(16),
                    Constraint::Length(8),
                    Constraint::Percentage(24),
                    Constraint::Min(12),
                ],
                &mut app.networking_suite.subnet_state,
            );
        }
    }
}

fn draw_addresses(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(subnet) = app.selected_networking_suite_subnet() else {
        draw_message(frame, area, "アドレス", "サブネット タブで選択してください");
        return;
    };
    let title = format!("アドレス — {}", subnet.name);
    match app.visible_networking_suite_addresses() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let kind = item.address_type_label();
                    let id = item.id();
                    vec![item.ip_address, item.ip_version, kind, id]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["IPアドレス", "バージョン", "種別", "ID"],
                rows,
                [
                    Constraint::Percentage(24),
                    Constraint::Length(12),
                    Constraint::Percentage(20),
                    Constraint::Min(12),
                ],
                &mut app.networking_suite.address_state,
            );
        }
    }
}

fn draw_table<const N: usize>(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    headers: [&str; N],
    rows: Vec<Vec<String>>,
    widths: [Constraint; N],
    state: &mut ratatui::widgets::TableState,
) {
    let block = Block::bordered()
        .title(format!(" {title} ({}) ", rows.len()))
        .border_style(border_style(true));
    if rows.is_empty() {
        frame.render_widget(placeholder("項目がありません").block(block), area);
        return;
    }
    let rows = rows
        .into_iter()
        .map(|values| Row::new(values.into_iter().map(Cell::from).collect::<Vec<_>>()));
    let table = Table::new(rows, widths)
        .header(Row::new(headers).style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)))
        .row_highlight_style(
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
        .block(block);
    frame.render_stateful_widget(table, area, state);
}

fn draw_pending(frame: &mut Frame, area: Rect, title: &str) {
    draw_message(frame, area, title, "読み込み中…");
}

fn draw_error(frame: &mut Frame, area: Rect, title: &str, err: &str) {
    frame.render_widget(
        Paragraph::new(err.to_string())
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(format!(" {title} "))),
        area,
    );
}

fn draw_message(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    frame.render_widget(
        placeholder(message).block(Block::bordered().title(format!(" {title} "))),
        area,
    );
}
