//! API Gateway の読み取り専用画面。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{
    DIM, accent, border_style, draw_error, draw_message, draw_pending, field, placeholder,
};
use crate::app::{ApiGatewayTab, App, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.api_gateway.tab {
        ApiGatewayTab::Subscriptions => draw_subscriptions(frame, chunks[1], app),
        ApiGatewayTab::Services => draw_services(frame, chunks[1], app),
        ApiGatewayTab::Routes => draw_routes(frame, chunks[1], app),
        ApiGatewayTab::Users => draw_users(frame, chunks[1], app),
        ApiGatewayTab::Groups => draw_groups(frame, chunks[1], app),
        ApiGatewayTab::Domains => draw_domains(frame, chunks[1], app),
        ApiGatewayTab::Certificates => draw_certificates(frame, chunks[1], app),
        ApiGatewayTab::Oidc => draw_oidcs(frame, chunks[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = ApiGatewayTab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!("{} {}", index + 1, tab.title())))
        .collect();
    let selected = ApiGatewayTab::ALL
        .iter()
        .position(|tab| *tab == app.api_gateway.tab)
        .unwrap_or_default();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM))),
        area,
    );
}

fn draw_subscriptions(frame: &mut Frame, area: Rect, app: &mut App) {
    let items = app.visible_api_gateway_subscriptions();
    match items {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, "契約"),
        Loadable::Failed(err) => draw_error(frame, area, "契約", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        item.plan_id,
                        item.service_name,
                        item.monthly_request.to_string(),
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                "契約",
                ["名前", "プラン", "サービス", "今月のリクエスト", "ID"],
                rows,
                [
                    Constraint::Percentage(18),
                    Constraint::Percentage(16),
                    Constraint::Percentage(20),
                    Constraint::Length(18),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.subscription_state,
            );
        }
    }
}

fn draw_services(frame: &mut Frame, area: Rect, app: &mut App) {
    let items = app.visible_api_gateway_services();
    match items {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, "サービス"),
        Loadable::Failed(err) => draw_error(frame, area, "サービス", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let endpoint = if item.port.is_some() {
                        format!(
                            "{}://{}:{}{}",
                            item.protocol,
                            item.host,
                            item.port.unwrap_or_default(),
                            item.path
                        )
                    } else {
                        format!("{}://{}{}", item.protocol, item.host, item.path)
                    };
                    vec![
                        item.name,
                        endpoint,
                        item.authentication,
                        item.subscription_name,
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                "サービス",
                ["名前", "接続先", "認証", "契約", "ID"],
                rows,
                [
                    Constraint::Percentage(18),
                    Constraint::Percentage(34),
                    Constraint::Percentage(12),
                    Constraint::Percentage(16),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.service_state,
            );
        }
    }
}

fn draw_routes(frame: &mut Frame, area: Rect, app: &mut App) {
    let service_name = app
        .selected_api_gateway_service()
        .map(|service| service.name)
        .unwrap_or_default();
    let title = if service_name.is_empty() {
        "ルート".to_string()
    } else {
        format!("ルート — {service_name}")
    };
    match app.visible_api_gateway_routes() {
        Loadable::Idle if app.selected_api_gateway_service().is_none() => draw_message(
            frame,
            area,
            &title,
            "Services タブでサービスを選択してください",
        ),
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, &title),
        Loadable::Failed(err) => draw_error(frame, area, &title, &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        item.methods.join(", "),
                        item.protocols.join(", "),
                        item.path,
                        if item.hosts.is_empty() {
                            item.host
                        } else {
                            item.hosts.join(", ")
                        },
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                &title,
                ["名前", "メソッド", "プロトコル", "パス", "ホスト", "ID"],
                rows,
                [
                    Constraint::Percentage(16),
                    Constraint::Percentage(14),
                    Constraint::Percentage(12),
                    Constraint::Percentage(18),
                    Constraint::Percentage(22),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.route_state,
            );
        }
    }
}

fn draw_users(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Min(28)])
        .split(area);
    match app.visible_api_gateway_users() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], "ユーザー"),
        Loadable::Failed(err) => draw_error(frame, chunks[0], "ユーザー", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        item.custom_id,
                        item.groups.join(", "),
                        item.tags.join(", "),
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                "ユーザー",
                ["名前", "カスタムID", "グループ", "タグ", "ID"],
                rows,
                [
                    Constraint::Percentage(18),
                    Constraint::Percentage(18),
                    Constraint::Percentage(20),
                    Constraint::Percentage(16),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.user_state,
            );
        }
    }
    draw_user_authentication(frame, chunks[1], app);
}

fn draw_user_authentication(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" 認証設定 ")
        .border_style(border_style(false));
    let Some(user) = app.selected_api_gateway_user() else {
        frame.render_widget(placeholder("ユーザーを選択してください").block(block), area);
        return;
    };
    let content = match app.selected_api_gateway_user_authentication() {
        Loadable::Idle | Loadable::Loading => vec![Line::from("読み込み中…")],
        Loadable::Failed(err) => vec![Line::from(Span::styled(
            err,
            Style::default().fg(Color::Red),
        ))],
        Loadable::Ready(auth) => {
            let mut lines = vec![field("ユーザー", &user.name)];
            if let Some(username) = auth.basic_username {
                lines.push(field("Basic", &username));
            }
            if let Some(algorithm) = auth.jwt_algorithm {
                lines.push(field("JWT", &algorithm));
            }
            if let Some(username) = auth.hmac_username {
                lines.push(field("HMAC", &username));
            }
            if lines.len() == 1 {
                lines.push(Line::from(Span::styled(
                    "認証設定なし",
                    Style::default().fg(DIM),
                )));
            }
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(content)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn draw_groups(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.visible_api_gateway_groups() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, "グループ"),
        Loadable::Failed(err) => draw_error(frame, area, "グループ", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| vec![item.name, item.tags.join(", "), item.id])
                .collect();
            draw_table(
                frame,
                area,
                "グループ",
                ["名前", "タグ", "ID"],
                rows,
                [
                    Constraint::Percentage(30),
                    Constraint::Percentage(30),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.group_state,
            );
        }
    }
}

fn draw_domains(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.visible_api_gateway_domains() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, "ドメイン"),
        Loadable::Failed(err) => draw_error(frame, area, "ドメイン", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        item.certificate_name,
                        item.certificate_id,
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                "ドメイン",
                ["ドメイン", "証明書", "証明書ID", "ID"],
                rows,
                [
                    Constraint::Percentage(28),
                    Constraint::Percentage(22),
                    Constraint::Percentage(24),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.domain_state,
            );
        }
    }
}

fn draw_certificates(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.visible_api_gateway_certificates() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, "証明書"),
        Loadable::Failed(err) => draw_error(frame, area, "証明書", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        item.rsa_expires_at.unwrap_or_default(),
                        item.ecdsa_expires_at.unwrap_or_default(),
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                "証明書",
                ["名前", "RSA期限", "ECDSA期限", "ID"],
                rows,
                [
                    Constraint::Percentage(24),
                    Constraint::Percentage(24),
                    Constraint::Percentage(24),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.certificate_state,
            );
        }
    }
}

fn draw_oidcs(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.visible_api_gateway_oidcs() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, area, "OIDC"),
        Loadable::Failed(err) => draw_error(frame, area, "OIDC", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    vec![
                        item.name,
                        item.issuer,
                        item.authentication_methods.join(", "),
                        item.scopes.join(", "),
                        item.id,
                    ]
                })
                .collect();
            draw_table(
                frame,
                area,
                "OIDC",
                ["名前", "Issuer", "認証方式", "スコープ", "ID"],
                rows,
                [
                    Constraint::Percentage(18),
                    Constraint::Percentage(28),
                    Constraint::Percentage(18),
                    Constraint::Percentage(16),
                    Constraint::Min(12),
                ],
                &mut app.api_gateway.oidc_state,
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
