//! AppRun（共用型）画面: アプリ一覧と、選択中アプリのバージョン・トラフィック。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, List, ListItem, Paragraph, Row, Table, Wrap};

use super::{DIM, SAKURA, border_style, field, format_datetime, placeholder, status_color};
use crate::app::{App, AppRunPane, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    // アプリ一覧そのものが取れていないときは、案内を読めるよう全幅で出す。
    if let Loadable::Failed(err) = &app.apprun.applications {
        super::draw_full_width_error(frame, area, "AppRun（共用型）", err);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_applications(frame, chunks[0], app);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(10), Constraint::Min(4)])
        .split(chunks[1]);
    draw_detail(frame, right[0], app);
    draw_versions(frame, right[1], app);
}

fn draw_applications(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.apprun.pane == AppRunPane::Applications;
    let applications: Vec<_> = app
        .visible_applications()
        .into_iter()
        .map(|item| (item.name.clone(), item.status.clone()))
        .collect();
    let block = Block::bordered()
        .title(Span::styled(
            if applications.is_empty() {
                " アプリケーション ".to_string()
            } else {
                format!(" アプリケーション ({}) ", applications.len())
            },
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    match &app.apprun.applications {
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
        Loadable::Ready(_) if applications.is_empty() => frame.render_widget(
            placeholder("アプリケーションがありません").block(block),
            area,
        ),
        Loadable::Ready(_) => {
            let rows: Vec<Row> = applications
                .iter()
                .map(|(name, status)| {
                    Row::new(vec![
                        Cell::from(name.clone()),
                        Cell::from(Span::styled(
                            status.clone(),
                            Style::default().fg(status_color(status)),
                        )),
                    ])
                })
                .collect();
            let table = Table::new(
                rows,
                [Constraint::Percentage(65), Constraint::Percentage(35)],
            )
            .header(
                Row::new(vec!["名前", "状態"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(if focused {
                Style::default()
                    .fg(SAKURA)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            })
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.apprun.application_state);
        }
    }
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" アプリの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let Some(application) = app.selected_application() else {
        frame.render_widget(
            placeholder("アプリケーションを選択してください").block(block),
            area,
        );
        return;
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(super::pad("公開URL", 14), Style::default().fg(DIM)),
        Span::styled(
            application.public_url.clone(),
            Style::default().fg(Color::Cyan),
        ),
    ])];
    if let Some(created) = &application.created_at {
        lines.push(field("作成日時", &format_datetime(created)));
    }

    match app.selected_application_detail() {
        Loadable::Ready(detail) => {
            if let (Some(min), Some(max)) = (detail.min_scale, detail.max_scale) {
                lines.push(field("スケール", &format!("{min} 〜 {max}")));
            }
            if let Some(port) = detail.port {
                lines.push(field("ポート", &port.to_string()));
            }
            if let Some(timeout) = detail.timeout_seconds {
                lines.push(field("タイムアウト", &format!("{timeout} 秒")));
            }
            for image in &detail.images {
                lines.push(Line::from(vec![
                    Span::styled(super::pad("イメージ", 14), Style::default().fg(DIM)),
                    Span::styled(image.clone(), Style::default().fg(Color::Cyan)),
                ]));
            }
        }
        Loadable::Failed(err) => lines.push(Line::from(Span::styled(
            err,
            Style::default().fg(Color::Red),
        ))),
        Loadable::Idle | Loadable::Loading => lines.push(Line::from(Span::styled(
            "詳細を読み込み中…",
            Style::default().fg(DIM),
        ))),
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn draw_versions(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.apprun.pane == AppRunPane::Versions;
    let versions = app.visible_versions();
    // 行の組み立てに app の借用が必要なので、先に必要な値だけ取り出す。
    let rows: Vec<(String, String, Option<i32>, Option<String>)> = versions
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|v| {
                    (
                        if app.is_latest_version(&v.name) {
                            format!("{} (最新)", v.name)
                        } else {
                            v.name.clone()
                        },
                        v.status.clone(),
                        app.traffic_percent(&v.name),
                        v.created_at.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let block = Block::bordered()
        .title(Span::styled(
            if rows.is_empty() {
                " バージョン ".to_string()
            } else {
                format!(" バージョン ({}) ", rows.len())
            },
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused))
        .padding(ratatui::widgets::Padding::horizontal(1));

    match &versions {
        Loadable::Idle => frame.render_widget(
            placeholder("アプリケーションを選択してください").block(block),
            area,
        ),
        Loadable::Loading => frame.render_widget(placeholder("読み込み中…").block(block), area),
        Loadable::Failed(err) => frame.render_widget(
            Paragraph::new(err.clone())
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        ),
        Loadable::Ready(_) if rows.is_empty() => {
            frame.render_widget(placeholder("バージョンがありません").block(block), area)
        }
        Loadable::Ready(_) => {
            let items: Vec<ListItem> = rows
                .iter()
                .map(|(name, status, percent, created)| {
                    let mut spans = vec![
                        Span::styled(
                            format!("{name:<22}"),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{status:<12}"),
                            Style::default().fg(status_color(status)),
                        ),
                    ];
                    // トラフィックが向いているバージョンを目立たせる。
                    match percent {
                        Some(percent) if *percent > 0 => spans.push(Span::styled(
                            format!(" {percent:>3}% "),
                            Style::default()
                                .fg(SAKURA)
                                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                        )),
                        _ => spans.push(Span::raw("      ")),
                    }
                    if let Some(created) = created {
                        spans.push(Span::styled(
                            format!("  {}", format_datetime(created)),
                            Style::default().fg(DIM),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            let list = List::new(items)
                .block(block)
                .highlight_style(if focused {
                    Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                })
                .highlight_symbol("▌");
            frame.render_stateful_widget(list, area, &mut app.apprun.version_state);
        }
    }
}
