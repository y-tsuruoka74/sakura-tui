//! クラウドHSMの読み取り専用画面。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{
    DIM, accent, border_style, draw_error, draw_message, draw_pending, field, format_datetime,
    placeholder,
};
use crate::app::{App, CloudHsmTab, Loadable};
use crate::cloudhsm::availability_label;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.cloudhsm.tab {
        CloudHsmTab::Hsms => draw_hsms(frame, chunks[1], app),
        CloudHsmTab::Clients => draw_clients(frame, chunks[1], app),
        CloudHsmTab::Licenses => draw_licenses(frame, chunks[1], app),
        CloudHsmTab::Documents => draw_documents(frame, chunks[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = CloudHsmTab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!("{} {}", index + 1, tab.title())))
        .collect();
    let selected = CloudHsmTab::ALL
        .iter()
        .position(|tab| *tab == app.cloudhsm.tab)
        .unwrap_or_default();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM))),
        area,
    );
}

fn draw_hsms(frame: &mut Frame, area: Rect, app: &mut App) {
    let title = format!("HSM — {}", app.zone);
    if let Loadable::Failed(err) = app.visible_cloudhsm_hsms() {
        draw_error(frame, area, &title, &err);
        return;
    }
    let chunks = split_with_detail(area);

    match app.visible_cloudhsm_hsms() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], &title),
        Loadable::Failed(err) => draw_error(frame, chunks[0], &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let status = availability_label(&item.availability);
                    let network = item.network_label();
                    vec![item.name, status, item.ipv4_address, network, item.id]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                &title,
                ["名前", "状態", "IPアドレス", "ネットワーク", "ID"],
                rows,
                [
                    Constraint::Percentage(24),
                    Constraint::Percentage(12),
                    Constraint::Percentage(18),
                    Constraint::Percentage(18),
                    Constraint::Min(12),
                ],
                &mut app.cloudhsm.hsm_state,
            );
        }
    }
    draw_hsm_detail(frame, chunks[1], app);
}

fn draw_hsm_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" HSMの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(hsm) = app.selected_cloudhsm_hsm() else {
        frame.render_widget(placeholder("HSMを選択してください").block(block), area);
        return;
    };

    let mut lines = vec![field("名前", &hsm.name)];
    let mut push = |label: &str, value: String| {
        if !value.is_empty() {
            lines.push(field(label, &value));
        }
    };
    push("状態", availability_label(&hsm.availability));
    push("サービスクラス", hsm.service_class.clone());
    push("IPアドレス", hsm.ipv4_address.clone());
    push("ネットワーク", hsm.network_label());
    push("ローカルルータ", hsm.local_router.clone());
    push("タグ", hsm.tags.join(", "));
    push("説明", hsm.description.clone());
    if !hsm.created_at.is_empty() {
        push("作成日時", format_datetime(&hsm.created_at));
    }
    if !hsm.modified_at.is_empty() {
        push("更新日時", format_datetime(&hsm.modified_at));
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

fn draw_clients(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(hsm) = app.selected_cloudhsm_hsm() else {
        draw_message(frame, area, "クライアント", "HSM タブで選択してください");
        return;
    };
    let title = format!("クライアント — {}", hsm.name);
    if let Loadable::Failed(err) = app.visible_cloudhsm_clients() {
        draw_error(frame, area, &title, &err);
        return;
    }
    // 証明書(PEM)は長いので詳細パネルへ回す。
    let chunks = split_with_detail(area);

    match app.visible_cloudhsm_clients() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], &title),
        Loadable::Failed(err) => draw_error(frame, chunks[0], &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let status = availability_label(&item.availability);
                    let certificate = item.certificate_label().to_string();
                    vec![item.name, status, certificate, item.id]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                &title,
                ["名前", "状態", "証明書", "ID"],
                rows,
                [
                    Constraint::Percentage(28),
                    Constraint::Percentage(14),
                    Constraint::Length(8),
                    Constraint::Min(20),
                ],
                &mut app.cloudhsm.client_state,
            );
        }
    }
    draw_client_detail(frame, chunks[1], app);
}

fn draw_client_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" クライアントの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(client) = app.selected_cloudhsm_client() else {
        frame.render_widget(
            placeholder("クライアントを選択してください").block(block),
            area,
        );
        return;
    };

    let mut lines = vec![
        field("名前", &client.name),
        field("状態", &availability_label(&client.availability)),
        field("ID", &client.id),
    ];
    if !client.created_at.is_empty() {
        lines.push(field("作成日時", &format_datetime(&client.created_at)));
    }
    if !client.certificate.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "証明書",
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        )));
        // 公開鍵側の証明書なので伏せる必要はない。
        for line in client.certificate.lines() {
            lines.push(Line::raw(line.to_string()));
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn draw_licenses(frame: &mut Frame, area: Rect, app: &mut App) {
    let title = format!("ライセンス — {}", app.zone);
    match app.visible_cloudhsm_licenses() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        item.service_class,
                        item.tags.join(", "),
                        item.description,
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["名前", "サービスクラス", "タグ", "説明", "ID"],
                rows,
                [
                    Constraint::Percentage(22),
                    Constraint::Percentage(24),
                    Constraint::Percentage(14),
                    Constraint::Percentage(20),
                    Constraint::Min(12),
                ],
                &mut app.cloudhsm.license_state,
            );
        }
    }
}

fn draw_documents(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(license) = app.selected_cloudhsm_license() else {
        draw_message(
            frame,
            area,
            "ドキュメント",
            "ライセンス タブで選択してください",
        );
        return;
    };
    let title = format!("ドキュメント — {}", license.name);
    match app.visible_cloudhsm_documents() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        format_datetime(&item.created_at),
                        format_datetime(&item.modified_at),
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["名前", "作成日時", "更新日時", "ID"],
                rows,
                [
                    Constraint::Percentage(34),
                    Constraint::Percentage(20),
                    Constraint::Percentage(20),
                    Constraint::Min(12),
                ],
                &mut app.cloudhsm.document_state,
            );
        }
    }
}

/// 一覧と詳細パネルに割る。横幅が足りないときは上下に積む。
fn split_with_detail(area: Rect) -> std::rc::Rc<[Rect]> {
    let horizontal = area.width >= 100;
    Layout::default()
        .direction(if horizontal {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints(if horizontal {
            [Constraint::Percentage(56), Constraint::Min(34)]
        } else {
            [Constraint::Percentage(50), Constraint::Min(8)]
        })
        .split(area)
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
