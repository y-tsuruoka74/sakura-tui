//! サービスエンドポイントゲートウェイの読み取り専用画面。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{
    DIM, accent, border_style, draw_error, draw_message, draw_pending, field, format_datetime,
    placeholder,
};
use crate::app::{App, Loadable, SegTab};
use crate::seg::Seg;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.seg.tab {
        SegTab::Gateways => draw_gateways(frame, chunks[1], app),
        SegTab::Services => draw_services(frame, chunks[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = SegTab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!("{} {}", index + 1, tab.title())))
        .collect();
    let selected = SegTab::ALL
        .iter()
        .position(|tab| *tab == app.seg.tab)
        .unwrap_or_default();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM))),
        area,
    );
}

fn draw_gateways(frame: &mut Frame, area: Rect, app: &mut App) {
    // ゾーン依存なので、どのゾーンを見ているかをタイトルに出す。
    let title = format!("サービスエンドポイントゲートウェイ — {}", app.zone);
    if let Loadable::Failed(err) = app.visible_seg_gateways() {
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

    match app.visible_seg_gateways() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], &title),
        Loadable::Failed(err) => draw_error(frame, chunks[0], &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let status = item.status_label();
                    let ip = item.ip_label();
                    vec![item.name, status, item.switch_name, ip, item.id]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                &title,
                ["名前", "状態", "スイッチ", "IPアドレス", "ID"],
                rows,
                [
                    Constraint::Percentage(26),
                    Constraint::Percentage(16),
                    Constraint::Percentage(18),
                    Constraint::Percentage(20),
                    Constraint::Min(12),
                ],
                &mut app.seg.gateway_state,
            );
        }
    }
    draw_gateway_detail(frame, chunks[1], app);
}

fn draw_gateway_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" ゲートウェイの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(gateway) = app.selected_seg_gateway() else {
        frame.render_widget(
            placeholder("ゲートウェイを選択してください").block(block),
            area,
        );
        return;
    };
    frame.render_widget(
        Paragraph::new(gateway_detail_lines(&gateway))
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// 詳細パネルの行。空の項目は落として詰める。
fn gateway_detail_lines(gateway: &Seg) -> Vec<Line<'static>> {
    let mut lines = vec![field("名前", &gateway.name)];
    let mut push = |label: &str, value: String| {
        if !value.is_empty() {
            lines.push(field(label, &value));
        }
    };

    push("状態", gateway.status_label());
    if !gateway.status_changed_at.is_empty() {
        push("状態変更", format_datetime(&gateway.status_changed_at));
    }
    let switch = match (
        gateway.switch_name.is_empty(),
        gateway.switch_scope.is_empty(),
    ) {
        (true, _) => String::new(),
        (false, true) => gateway.switch_name.clone(),
        (false, false) => format!(
            "{} ({})",
            gateway.switch_name,
            switch_scope_label(&gateway.switch_scope)
        ),
    };
    push("スイッチ", switch);
    push("スイッチID", gateway.switch_id.clone());
    push("IPアドレス", gateway.ip_addresses.join(", "));
    push("ユーザーIP", gateway.user_ip_addresses.join(", "));
    if gateway.network_mask_len != 0 {
        push("マスク長", format!("/{}", gateway.network_mask_len));
    }
    push("接続元サーバー", gateway.server_ip_addresses.join(", "));
    push("ゾーン", gateway.zone.clone());

    push(
        "接続先サービス",
        if gateway.services.is_empty() {
            String::new()
        } else {
            format!("{} 件（2 のタブ）", gateway.services.len())
        },
    );
    push(
        "モニタリングスイート連携",
        enabled_label(gateway.monitoring_suite_enabled).to_string(),
    );
    if let Some(dns) = &gateway.dns_forwarding {
        push("DNS転送", enabled_label(dns.enabled).to_string());
        push("ホストゾーン", dns.private_hosted_zone.clone());
        push("フォワード先", dns.upstream_dns.join(", "));
    }

    push("タグ", gateway.tags.join(", "));
    push("説明", gateway.description.clone());
    if !gateway.created_at.is_empty() {
        push("作成日時", format_datetime(&gateway.created_at));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "y: リソースIDをコピー",
        Style::default().fg(DIM),
    )));
    lines
}

fn switch_scope_label(raw: &str) -> String {
    match raw {
        "user" => "ユーザー".to_string(),
        "shared" => "共有".to_string(),
        other => other.to_string(),
    }
}

fn enabled_label(enabled: bool) -> &'static str {
    if enabled { "有効" } else { "無効" }
}

fn draw_services(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(gateway) = app.selected_seg_gateway() else {
        draw_message(
            frame,
            area,
            "接続先サービス",
            "ゲートウェイ タブで選択してください",
        );
        return;
    };
    let title = format!("接続先サービス — {}", gateway.name);
    match app.visible_seg_services() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let kind = item.kind_label();
                    let mode = item.mode_label().to_string();
                    vec![kind, item.endpoint, mode]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["種別", "エンドポイント", "設定方法"],
                rows,
                [
                    Constraint::Percentage(32),
                    Constraint::Min(24),
                    Constraint::Length(10),
                ],
                &mut app.seg.service_state,
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
