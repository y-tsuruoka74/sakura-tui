//! セキュリティコントロールの読み取り専用画面。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{DIM, accent, border_style, field, format_datetime, placeholder};
use crate::app::{App, Loadable, SecurityControlTab};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.security_control.tab {
        SecurityControlTab::Rules => draw_rules(frame, chunks[1], app),
        SecurityControlTab::Actions => draw_actions(frame, chunks[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = SecurityControlTab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!("{} {}", index + 1, tab.title())))
        .collect();
    let selected = SecurityControlTab::ALL
        .iter()
        .position(|tab| *tab == app.security_control.tab)
        .unwrap_or_default();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM))),
        area,
    );
}

fn draw_rules(frame: &mut Frame, area: Rect, app: &mut App) {
    // 有効化状態はプロジェクト単位の情報なので、既定タブの先頭に据える。
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(3)])
        .split(area);
    draw_activation(frame, chunks[0], app);

    match app.visible_security_control_rules() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[1], "評価ルール"),
        Loadable::Failed(err) => draw_error(frame, chunks[1], "評価ルール", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let status = item.status_label().to_string();
                    let scope = item.scope_label();
                    vec![
                        item.id,
                        status,
                        scope,
                        item.iam_roles_required.join(", "),
                        item.description,
                    ]
                })
                .collect();
            draw_table(
                frame,
                chunks[1],
                "評価ルール",
                ["ルール", "状態", "対象", "必要ロール", "説明"],
                rows,
                [
                    Constraint::Percentage(26),
                    Constraint::Length(6),
                    Constraint::Percentage(16),
                    Constraint::Percentage(16),
                    Constraint::Min(20),
                ],
                &mut app.security_control.rule_state,
            );
        }
    }
}

fn draw_activation(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" 有効化状態 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let lines = match app.security_control.activation.clone() {
        Loadable::Idle | Loadable::Loading => vec![Line::from("読み込み中…")],
        Loadable::Failed(err) => vec![Line::from(Span::styled(
            err,
            Style::default().fg(Color::Red),
        ))],
        Loadable::Ready(activation) => {
            let mut lines = vec![field("状態", activation.status_label())];
            if !activation.service_principal_id.is_empty() {
                lines.push(field(
                    "サービスプリンシパル",
                    &activation.service_principal_id,
                ));
            }
            if activation.automated_action_limit != 0 {
                lines.push(field(
                    "自動アクション上限",
                    &activation.automated_action_limit.to_string(),
                ));
            }
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

fn draw_actions(frame: &mut Frame, area: Rect, app: &mut App) {
    if let Loadable::Failed(err) = app.visible_security_control_actions() {
        draw_error(frame, area, "自動アクション", &err);
        return;
    }
    // 実行条件（CEL式）は長くなるので、詳細パネルへ回す。
    let horizontal = area.width >= 100;
    let chunks = Layout::default()
        .direction(if horizontal {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints(if horizontal {
            [Constraint::Percentage(58), Constraint::Min(34)]
        } else {
            [Constraint::Percentage(52), Constraint::Min(8)]
        })
        .split(area);

    match app.visible_security_control_actions() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], "自動アクション"),
        Loadable::Failed(err) => draw_error(frame, chunks[0], "自動アクション", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let kind = item.action_type_label();
                    let target = item.target_label();
                    let status = item.status_label().to_string();
                    vec![item.name, status, kind, target, item.id]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                "自動アクション",
                ["名前", "状態", "種別", "宛先", "ID"],
                rows,
                [
                    Constraint::Percentage(26),
                    Constraint::Length(6),
                    Constraint::Percentage(18),
                    Constraint::Percentage(20),
                    Constraint::Min(12),
                ],
                &mut app.security_control.action_state,
            );
        }
    }
    draw_action_detail(frame, chunks[1], app);
}

fn draw_action_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" アクションの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(action) = app.selected_security_control_action() else {
        frame.render_widget(
            placeholder("自動アクションを選択してください").block(block),
            area,
        );
        return;
    };

    let mut lines = vec![field("名前", &action.name)];
    let mut push = |label: &str, value: String| {
        if !value.is_empty() {
            lines.push(field(label, &value));
        }
    };
    push("状態", action.status_label().to_string());
    push("種別", action.action_type_label());
    push("宛先", action.target_label());
    push("サービスプリンシパル", action.service_principal_id.clone());
    push("説明", action.description.clone());
    if !action.created_at.is_empty() {
        push("作成日時", format_datetime(&action.created_at));
    }
    if !action.execution_condition.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "実行条件 (CEL)",
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::raw(action.execution_condition.clone()));
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
    frame.render_widget(
        placeholder("読み込み中…").block(Block::bordered().title(format!(" {title} "))),
        area,
    );
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
