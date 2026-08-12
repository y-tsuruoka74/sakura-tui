//! AppRun 専有型画面: クラスタ一覧と、選択中クラスタのアプリ・ASG・証明書。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, List, ListItem, Paragraph, Row, Table, Tabs, Wrap};

use super::{
    DIM, accent, border_style, error_paragraph, field, format_unix, placeholder, status_color,
};
use crate::app::{App, DedicatedFocus, DedicatedTab, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    // クラスタ一覧そのものが取れないときは、案内を読めるよう全幅で出す。
    if let Loadable::Failed(err) = &app.dedicated.clusters {
        super::draw_full_width_error(frame, area, "AppRun（専有型）", err);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(area);

    draw_clusters(frame, chunks[0], app);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(chunks[1]);
    draw_tabs(frame, right[0], app);

    match app.dedicated.tab {
        DedicatedTab::Overview => draw_overview(frame, right[1], app),
        DedicatedTab::Applications => draw_applications(frame, right[1], app),
        DedicatedTab::ScalingGroups => draw_scaling_groups(frame, right[1], app),
        DedicatedTab::Certificates => draw_certificates(frame, right[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = DedicatedTab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| Line::from(format!("{} {}", i + 1, tab.title())))
        .collect();
    let selected = DedicatedTab::ALL
        .iter()
        .position(|t| *t == app.dedicated.tab)
        .unwrap_or(0);
    let highlight = if app.dedicated.focus == DedicatedFocus::Detail {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM).add_modifier(Modifier::BOLD)
    };
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(highlight)
            .divider(Span::styled("│", Style::default().fg(DIM)))
            .block(Block::default().padding(ratatui::widgets::Padding::horizontal(1))),
        area,
    );
}

fn draw_clusters(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.dedicated.focus == DedicatedFocus::Clusters;
    let clusters: Vec<(String, Option<i64>)> = app
        .visible_clusters()
        .into_iter()
        .map(|c| (c.name.clone(), c.created))
        .collect();
    let block = Block::bordered()
        .title(Span::styled(
            if clusters.is_empty() {
                " クラスタ ".to_string()
            } else {
                format!(" クラスタ ({}) ", clusters.len())
            },
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    match &app.dedicated.clusters {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area)
        }
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(_) if clusters.is_empty() => {
            frame.render_widget(placeholder("クラスタがありません").block(block), area)
        }
        Loadable::Ready(_) => {
            let rows: Vec<Row> = clusters
                .iter()
                .map(|(name, created)| {
                    Row::new(vec![
                        Cell::from(name.clone()),
                        Cell::from(Span::styled(
                            created.map(format_unix).unwrap_or_default(),
                            Style::default().fg(DIM),
                        )),
                    ])
                })
                .collect();
            let table = Table::new(rows, [Constraint::Min(10), Constraint::Length(17)])
                .header(
                    Row::new(vec!["名前", "作成日時"])
                        .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
                )
                .row_highlight_style(highlight(focused))
                .block(block);
            frame.render_stateful_widget(table, area, &mut app.dedicated.cluster_state);
        }
    }
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" 概要 ")
        .border_style(border_style(app.dedicated.focus == DedicatedFocus::Detail))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let Some(cluster) = app.selected_cluster() else {
        frame.render_widget(placeholder("クラスタを選択してください").block(block), area);
        return;
    };

    let mut lines = vec![
        field("名前", &cluster.name),
        field("クラスタID", &cluster.id),
    ];
    if let Some(created) = cluster.created {
        lines.push(field("作成日時", &format_unix(created)));
    }

    // ポートやサービスプリンシパルは一覧に含まれないので、詳細取得を待つ。
    match app.selected_cluster_detail() {
        Loadable::Ready(detail) => {
            let ports: Vec<String> = detail.ports.iter().map(ToString::to_string).collect();
            lines.push(field(
                "ポート",
                &if ports.is_empty() {
                    "(なし)".to_string()
                } else {
                    ports.join(", ")
                },
            ));
            lines.push(field("サービスプリンシパル", &detail.service_principal_id));
            lines.push(field(
                "Let's Encrypt",
                if detail.has_lets_encrypt_email {
                    "メール設定あり"
                } else {
                    "未設定"
                },
            ));
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

fn draw_applications(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.dedicated.focus == DedicatedFocus::Detail;
    let applications = app.visible_dedicated_applications();
    let block = pane_block(
        " アプリケーション ",
        applications.ready().map_or(0, Vec::len),
        focused,
    );

    match &applications {
        Loadable::Idle => {
            frame.render_widget(placeholder("クラスタを選択してください").block(block), area)
        }
        Loadable::Loading => frame.render_widget(placeholder("読み込み中…").block(block), area),
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(items) if items.is_empty() => frame.render_widget(
            placeholder("このクラスタにアプリケーションがありません").block(block),
            area,
        ),
        Loadable::Ready(items) => {
            let rows: Vec<Row> = items
                .iter()
                .map(|app| {
                    Row::new(vec![
                        Cell::from(app.name.clone()),
                        Cell::from(Span::styled(
                            app.active_version
                                .map(|v| format!("v{v}"))
                                .unwrap_or_default(),
                            Style::default().fg(DIM),
                        )),
                        Cell::from(Span::styled(
                            app.desired_count
                                .map(|c| format!("{c} 台"))
                                .unwrap_or_default(),
                            Style::default().fg(DIM),
                        )),
                        Cell::from(Span::styled(
                            app.scaling_cooldown_seconds
                                .map(|s| format!("{s}s"))
                                .unwrap_or_default(),
                            Style::default().fg(DIM),
                        )),
                    ])
                })
                .collect();
            let table = Table::new(
                rows,
                [
                    Constraint::Min(12),
                    Constraint::Length(10),
                    Constraint::Length(8),
                    Constraint::Length(10),
                ],
            )
            .header(
                Row::new(vec!["名前", "稼働中", "台数", "クールダウン"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(highlight(focused))
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.dedicated.application_state);
        }
    }
}

fn draw_scaling_groups(frame: &mut Frame, area: Rect, app: &mut App) {
    // ASG 一覧と、選択中 ASG のワーカーノードを縦に並べる。
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let focused = app.dedicated.focus == DedicatedFocus::Detail;
    let groups = app.visible_scaling_groups();
    let block = pane_block(
        " オートスケーリンググループ ",
        groups.ready().map_or(0, Vec::len),
        focused,
    );

    match &groups {
        Loadable::Idle => frame.render_widget(
            placeholder("クラスタを選択してください").block(block),
            chunks[0],
        ),
        Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), chunks[0])
        }
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), chunks[0]),
        Loadable::Ready(items) if items.is_empty() => {
            frame.render_widget(placeholder("ASG がありません").block(block), chunks[0])
        }
        Loadable::Ready(items) => {
            let rows: Vec<Row> = items
                .iter()
                .map(|group| {
                    let scale = match (group.min_nodes, group.max_nodes) {
                        (Some(min), Some(max)) => format!("{min}〜{max}"),
                        _ => String::new(),
                    };
                    Row::new(vec![
                        Cell::from(if group.deleting {
                            // 削除中は名前だけだと分からないので添える。
                            format!("{} (削除中)", group.name)
                        } else {
                            group.name.clone()
                        }),
                        Cell::from(Span::styled(group.zone.clone(), Style::default().fg(DIM))),
                        Cell::from(Span::styled(scale, Style::default().fg(DIM))),
                        Cell::from(Span::styled(
                            group
                                .worker_node_count
                                .map(|c| format!("{c} 台"))
                                .unwrap_or_default(),
                            Style::default().fg(DIM),
                        )),
                    ])
                })
                .collect();
            let table = Table::new(
                rows,
                [
                    Constraint::Min(12),
                    Constraint::Length(8),
                    Constraint::Length(9),
                    Constraint::Length(7),
                ],
            )
            .header(
                Row::new(vec!["名前", "ゾーン", "台数範囲", "現在"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(highlight(focused))
            .block(block);
            frame.render_stateful_widget(table, chunks[0], &mut app.dedicated.scaling_group_state);
        }
    }

    draw_worker_nodes(frame, chunks[1], app);
}

fn draw_worker_nodes(frame: &mut Frame, area: Rect, app: &mut App) {
    let nodes = app.current_worker_nodes();
    let block = Block::bordered()
        .title(match app.selected_scaling_group() {
            Some(group) => format!(" ワーカーノード: {} ", group.name),
            None => " ワーカーノード ".to_string(),
        })
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));

    match &nodes {
        Loadable::Idle => {
            frame.render_widget(placeholder("ASG を選択してください").block(block), area)
        }
        Loadable::Loading => frame.render_widget(placeholder("読み込み中…").block(block), area),
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(items) if items.is_empty() => {
            frame.render_widget(placeholder("ワーカーノードがありません").block(block), area)
        }
        Loadable::Ready(items) => {
            let list: Vec<ListItem> = items
                .iter()
                .map(|node| {
                    let mut spans = vec![
                        Span::styled(
                            format!("{:<38}", node.id),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{:<10}", node.status),
                            Style::default().fg(status_color(&node.status)),
                        ),
                    ];
                    if node.draining {
                        spans.push(Span::styled(
                            " draining ",
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    if !node.archive_version.is_empty() {
                        spans.push(Span::styled(
                            format!(" {}", node.archive_version),
                            Style::default().fg(DIM),
                        ));
                    }
                    if let Some(created) = node.created {
                        spans.push(Span::styled(
                            format!("  {}", format_unix(created)),
                            Style::default().fg(DIM),
                        ));
                    }
                    if !node.error_message.is_empty() {
                        spans.push(Span::styled(
                            format!("  {}", node.error_message),
                            Style::default().fg(Color::Red),
                        ));
                    }
                    ListItem::new(Line::from(spans))
                })
                .collect();
            frame.render_stateful_widget(
                List::new(list)
                    .block(block)
                    .highlight_style(Style::default().add_modifier(Modifier::BOLD))
                    .highlight_symbol("▌"),
                area,
                &mut app.dedicated.worker_node_state,
            );
        }
    }
}

fn draw_certificates(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.dedicated.focus == DedicatedFocus::Detail;
    let certificates = app.visible_certificates();
    let block = pane_block(
        " 証明書 ",
        certificates.ready().map_or(0, Vec::len),
        focused,
    );

    match &certificates {
        Loadable::Idle => {
            frame.render_widget(placeholder("クラスタを選択してください").block(block), area)
        }
        Loadable::Loading => frame.render_widget(placeholder("読み込み中…").block(block), area),
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(items) if items.is_empty() => {
            frame.render_widget(placeholder("証明書がありません").block(block), area)
        }
        Loadable::Ready(items) => {
            let rows: Vec<Row> = items
                .iter()
                .map(|cert| {
                    Row::new(vec![
                        Cell::from(cert.name.clone()),
                        Cell::from(cert.common_name.clone()),
                        Cell::from(Span::styled(
                            cert.not_after.map(format_unix).unwrap_or_default(),
                            Style::default().fg(DIM),
                        )),
                    ])
                })
                .collect();
            let table = Table::new(
                rows,
                [
                    Constraint::Length(16),
                    Constraint::Min(14),
                    Constraint::Length(17),
                ],
            )
            .header(
                Row::new(vec!["名前", "コモンネーム", "有効期限"])
                    .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(highlight(focused))
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.dedicated.certificate_state);
        }
    }
}

fn pane_block(title: &str, count: usize, focused: bool) -> Block<'static> {
    Block::bordered()
        .title(Span::styled(
            if count > 0 {
                format!("{}({count}) ", title)
            } else {
                title.to_string()
            },
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused))
}

fn highlight(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}
