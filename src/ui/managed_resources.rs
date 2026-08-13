//! アプリケーション連携・マネージドサービスの共通一覧・詳細画面。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};

use super::{DIM, accent, border_style, field, format_datetime, placeholder};
use crate::app::{App, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let title = app
        .managed_resource_kind()
        .map(|kind| kind.title())
        .unwrap_or("リソース");
    if let Loadable::Failed(err) = app.visible_managed_resources() {
        super::draw_full_width_error(frame, area, title, &err);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);
    draw_list(frame, chunks[0], app, title);
    draw_detail(frame, chunks[1], app, title);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App, title: &str) {
    let resources = app.visible_managed_resources();
    let count = resources.ready().map_or(0, Vec::len);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {title} ({count}) "),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(true));
    match resources {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area)
        }
        Loadable::Failed(err) => frame.render_widget(
            Paragraph::new(err)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        ),
        Loadable::Ready(items) if items.is_empty() => frame.render_widget(
            placeholder(&format!("{title}のリソースがありません")).block(block),
            area,
        ),
        Loadable::Ready(items) => {
            let rows = items.into_iter().map(|item| {
                Row::new(vec![
                    Cell::from(item.name),
                    Cell::from(item.resource_type),
                    Cell::from(Span::styled(
                        item.status.clone(),
                        Style::default().fg(status_color(&item.status)),
                    )),
                    Cell::from(Span::styled(item.plan, Style::default().fg(DIM))),
                    Cell::from(Span::styled(item.id, Style::default().fg(DIM))),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Min(16),
                    Constraint::Min(12),
                    Constraint::Length(10),
                    Constraint::Min(12),
                    Constraint::Min(14),
                ],
            )
            .header(
                Row::new(vec!["名前", "種別/サイト", "状態", "プラン/実行", "ID"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.managed_resources.state);
        }
    }
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App, title: &str) {
    let block = Block::bordered()
        .title(format!(" {title}の詳細 "))
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(resource) = app.selected_managed_resource() else {
        frame.render_widget(
            placeholder(&format!("{title}を選択してください")).block(block),
            area,
        );
        return;
    };
    let mut lines = vec![field("名前", &resource.name)];
    for (label, value) in &resource.details {
        let shown = if label.ends_with("日時") {
            format_datetime(value)
        } else {
            value.clone()
        };
        lines.push(field(label, &shown));
    }
    if !resource.description.is_empty() {
        lines.push(field("説明", &resource.description));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "y: リソースIDをコピー",
        Style::default().fg(DIM),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn status_color(status: &str) -> Color {
    match status.to_ascii_lowercase().as_str() {
        "available" | "up" | "公開" => Color::Green,
        "failed" | "down" => Color::Red,
        _ => DIM,
    }
}
