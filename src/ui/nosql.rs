//! NoSQL の読み取り専用画面。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{
    DIM, accent, border_style, draw_error, draw_message, draw_pending, field, format_datetime,
    placeholder,
};
use crate::app::{App, Loadable, NoSqlTab};
use crate::nosql::NoSqlDatabase;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.nosql.tab {
        NoSqlTab::Databases => draw_databases(frame, chunks[1], app),
        NoSqlTab::Nodes => draw_nodes(frame, chunks[1], app),
        NoSqlTab::Backups => draw_backups(frame, chunks[1], app),
        NoSqlTab::Parameters => draw_parameters(frame, chunks[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = NoSqlTab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!("{} {}", index + 1, tab.title())))
        .collect();
    let selected = NoSqlTab::ALL
        .iter()
        .position(|tab| *tab == app.nosql.tab)
        .unwrap_or_default();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM))),
        area,
    );
}

/// 一覧のタイトル。
///
/// NoSQL は東京第2ゾーン限定でゾーン切り替えの対象外なので、
/// どのゾーンへ問い合わせたのかをここで必ず見せる。
fn list_title(app: &App) -> String {
    format!("NoSQL — {}", app.nosql_zone())
}

fn draw_databases(frame: &mut Frame, area: Rect, app: &mut App) {
    let title = list_title(app);
    let items = app.visible_nosql_databases();
    // エラーは詳細パネルを出さず、幅いっぱいに理由を見せる。
    if let Loadable::Failed(err) = items {
        draw_error(frame, area, &title, &err);
        return;
    }
    // 詳細に出す項目が多いため、横幅があるときだけ左右に割る。
    let horizontal = area.width >= 100;
    let chunks = Layout::default()
        .direction(if horizontal {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints(if horizontal {
            [Constraint::Percentage(54), Constraint::Min(38)]
        } else {
            [Constraint::Percentage(50), Constraint::Min(10)]
        })
        .split(area);

    match app.visible_nosql_databases() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], &title),
        Loadable::Failed(err) => draw_error(frame, chunks[0], &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let status = item.status_label();
                    let plan = item.plan.label();
                    let nodes = if item.nodes == 0 {
                        String::new()
                    } else {
                        item.nodes.to_string()
                    };
                    vec![item.name, status, plan, nodes, item.version, item.id]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                &title,
                ["名前", "状態", "プラン", "ノード", "バージョン", "ID"],
                rows,
                [
                    Constraint::Percentage(24),
                    Constraint::Percentage(16),
                    Constraint::Percentage(14),
                    Constraint::Length(6),
                    Constraint::Length(10),
                    Constraint::Min(12),
                ],
                &mut app.nosql.database_state,
            );
        }
    }
    draw_database_detail(frame, chunks[1], app);
}

fn draw_database_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" NoSQLの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(db) = app.selected_nosql_database() else {
        frame.render_widget(placeholder("NoSQLを選択してください").block(block), area);
        return;
    };
    frame.render_widget(
        Paragraph::new(database_detail_lines(&db))
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// 詳細パネルの行。空の項目は落として詰める。
fn database_detail_lines(db: &NoSqlDatabase) -> Vec<Line<'static>> {
    let mut lines = vec![field("名前", &db.name)];
    let mut push = |label: &str, value: String| {
        if !value.is_empty() {
            lines.push(field(label, &value));
        }
    };

    push("状態", db.status_label());
    if !db.status_changed_at.is_empty() {
        push("状態変更", format_datetime(&db.status_changed_at));
    }
    let engine = match (db.engine.is_empty(), db.version.is_empty()) {
        (true, true) => String::new(),
        (true, false) => db.version.clone(),
        (false, true) => db.engine.clone(),
        (false, false) => format!("{} {}", db.engine, db.version),
    };
    push("エンジン", engine);
    push("プラン", db.plan.label());
    push("諸元", db.plan.spec_label());
    if db.nodes != 0 {
        push("ノード数", db.nodes.to_string());
    }
    push("ストレージ", db.storage.clone());
    if db.port != 0 {
        push("ポート", db.port.to_string());
    }
    push("既定ユーザー", db.default_user.clone());
    push("ゾーン", db.zone.clone());
    push("IPアドレス", db.ip_addresses.join(", "));
    if db.network_mask_len != 0 {
        push(
            "ネットワーク",
            format!("{} /{}", db.default_route, db.network_mask_len),
        );
    } else {
        push("デフォルトルート", db.default_route.clone());
    }
    push("接続元ネットワーク", db.source_networks.join(", "));
    push("予備IP", db.reserve_ip_address.clone());

    if let Some(backup) = &db.backup {
        let mut parts = Vec::new();
        if !backup.day_of_week.is_empty() {
            parts.push(backup.day_of_week.join(","));
        }
        if !backup.time.is_empty() {
            parts.push(backup.time.clone());
        }
        if backup.rotate != 0 {
            parts.push(format!("{}世代", backup.rotate));
        }
        push("バックアップ", parts.join(" "));
        push("バックアップ先", backup.connect.clone());
    }
    if let Some(repair) = &db.repair {
        if !repair.incremental_days.is_empty() || !repair.incremental_time.is_empty() {
            push(
                "増分リペア",
                format!(
                    "{} {}",
                    repair.incremental_days.join(","),
                    repair.incremental_time
                )
                .trim()
                .to_string(),
            );
        }
        if repair.full_interval != 0 || !repair.full_day.is_empty() {
            let interval = if repair.full_interval == 0 {
                String::new()
            } else {
                format!("{}日ごと", repair.full_interval)
            };
            push(
                "完全リペア",
                format!("{} {} {}", interval, repair.full_day, repair.full_time)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
    }

    push("ディスク暗号化", db.encryption_algorithm.clone());
    push("暗号化鍵", db.encryption_key_id.clone());
    push("タグ", db.tags.join(", "));
    push("説明", db.description.clone());
    if !db.created_at.is_empty() {
        push("作成日時", format_datetime(&db.created_at));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "y: リソースIDをコピー",
        Style::default().fg(DIM),
    )));
    lines
}

fn draw_nodes(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(database) = app.selected_nosql_database() else {
        draw_message(frame, area, "ノード", "DB タブで NoSQL を選択してください");
        return;
    };
    let title = format!("ノード — {}", database.name);

    // ヘッダに全体の健全性とバージョンを出し、その下にノード表を置く。
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(3)])
        .split(area);
    draw_node_summary(frame, chunks[0], app);

    match app.visible_nosql_nodes() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[1], &title),
        Loadable::Failed(err) => draw_error(frame, chunks[1], &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|node| {
                    let index = node.index.to_string();
                    let node_type = node.node_type_label();
                    let group = node.group_label().to_string();
                    vec![
                        index,
                        node_type,
                        node.ip_address,
                        group,
                        node.availability,
                        node.appliance_id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                chunks[1],
                &title,
                ["通番", "種別", "IPアドレス", "区分", "可用性", "ID"],
                rows,
                [
                    Constraint::Length(6),
                    Constraint::Percentage(18),
                    Constraint::Percentage(22),
                    Constraint::Length(12),
                    Constraint::Percentage(14),
                    Constraint::Min(12),
                ],
                &mut app.nosql.node_state,
            );
        }
    }
}

fn draw_node_summary(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" 状態 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let health = match app.selected_nosql_node_health() {
        Loadable::Ready(health) => health.label(),
        Loadable::Failed(err) => err,
        Loadable::Idle | Loadable::Loading => "読み込み中…".to_string(),
    };
    let mut lines = vec![field("健全性", &health)];

    match app.selected_nosql_status() {
        Loadable::Ready(status) => {
            let version = if status.upgrade_available() {
                format!("{} → {} が利用可能", status.version, status.upgrade_version)
            } else {
                status.version.clone()
            };
            if !version.is_empty() {
                lines.push(field("バージョン", &version));
            }
            let jobs: Vec<String> = status
                .jobs
                .iter()
                .map(|job| {
                    format!("{} {}", job.job_type, job.status)
                        .trim()
                        .to_string()
                })
                .filter(|job| !job.is_empty())
                .collect();
            if !jobs.is_empty() {
                lines.push(field("ジョブ", &jobs.join(", ")));
            }
        }
        Loadable::Failed(err) => lines.push(Line::from(Span::styled(
            err,
            Style::default().fg(Color::Red),
        ))),
        Loadable::Idle | Loadable::Loading => {}
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn draw_backups(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(database) = app.selected_nosql_database() else {
        draw_message(
            frame,
            area,
            "バックアップ",
            "DB タブで NoSQL を選択してください",
        );
        return;
    };
    let title = format!("バックアップ — {}", database.name);
    match app.visible_nosql_backups() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        format_datetime(&item.backup_at),
                        // 仕様に単位の記載が無いため、数値のまま出す。
                        item.size.to_string(),
                        item.delete_status_label(),
                        item.restore_status_label(),
                        format_datetime(&item.restore_at),
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                [
                    "取得日時",
                    "サイズ",
                    "削除状態",
                    "復元状態",
                    "復元日時",
                    "ID",
                ],
                rows,
                [
                    Constraint::Percentage(18),
                    Constraint::Length(8),
                    Constraint::Length(10),
                    Constraint::Length(10),
                    Constraint::Percentage(18),
                    Constraint::Min(12),
                ],
                &mut app.nosql.backup_state,
            );
        }
    }
}

fn draw_parameters(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(database) = app.selected_nosql_database() else {
        draw_message(
            frame,
            area,
            "パラメータ",
            "DB タブで NoSQL を選択してください",
        );
        return;
    };
    let title = format!("パラメータ — {}", database.name);
    match app.visible_nosql_parameters() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    // 既定値のままなら現在値の列は空けて、変更点を目立たせる。
                    let value = if item.overridden() {
                        item.value.clone()
                    } else {
                        String::new()
                    };
                    vec![
                        item.name,
                        value,
                        item.default_value,
                        item.options.join(", "),
                        item.description,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["設定項目", "現在値", "既定値", "選択肢", "説明"],
                rows,
                [
                    Constraint::Percentage(24),
                    Constraint::Percentage(12),
                    Constraint::Percentage(12),
                    Constraint::Percentage(20),
                    Constraint::Min(16),
                ],
                &mut app.nosql.parameter_state,
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
