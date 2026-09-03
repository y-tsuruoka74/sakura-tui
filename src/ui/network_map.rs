//! 接続マップ画面。
//!
//! ネットワークを見出しにして、繋がっている NIC を罫線でぶら下げる。
//! 端末では線を自由に引けないので、木構造で「どこに何が繋がっているか」を出す。

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Wrap};

use super::{DIM, accent, border_style, placeholder};
use crate::app::{App, Loadable, MapRow, NetworkKind};
use crate::iaas::PowerStatus;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let rows = app.visible_network_map();
    let block = Block::bordered()
        .title(Span::styled(
            format!(" 接続マップ — {} ", app.zone),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(true))
        .padding(ratatui::widgets::Padding::new(1, 1, 0, 0));

    match &rows {
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
        Loadable::Ready(items) if items.is_empty() => frame.render_widget(
            placeholder("このゾーンには繋がっているものがありません（z でゾーンを切り替え）")
                .block(block),
            area,
        ),
        Loadable::Ready(items) => {
            let table = Table::new(
                items.iter().map(map_row),
                [
                    // 罫線と名前、NIC、IP、補足。名前は伸ばしすぎると
                    // 右の情報と離れて読みにくいので、上限を決めておく。
                    Constraint::Max(34),
                    Constraint::Length(6),
                    Constraint::Length(17),
                    Constraint::Min(20),
                ],
            )
            .row_highlight_style(
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            )
            .block(block);
            frame.render_stateful_widget(table, area, &mut app.network_map.state);
        }
    }
}

/// 電源の印。一覧の色分けと同じ考え方で、起動中だけ目立たせる。
fn power_mark(power: PowerStatus) -> Span<'static> {
    let (mark, color) = match power {
        PowerStatus::Up => ("●", Color::Green),
        PowerStatus::Down => ("○", DIM),
        PowerStatus::Cleaning => ("◐", Color::Yellow),
        PowerStatus::Unknown => ("◌", Color::Magenta),
    };
    Span::styled(
        format!("{mark} {}", power.label()),
        Style::default().fg(color),
    )
}

fn map_row(row: &MapRow) -> Row<'static> {
    match row {
        MapRow::Network {
            kind,
            name,
            note,
            nics,
            appliances,
        } => {
            let color = match kind {
                // 外向きの入り口と未接続は、探すときの手がかりになるので色を変える。
                NetworkKind::Router => Color::Cyan,
                NetworkKind::Shared => accent(),
                NetworkKind::Switch => Color::Blue,
                NetworkKind::Unconnected => Color::Yellow,
            };
            let mut count = Vec::new();
            if *nics > 0 {
                count.push(format!("{nics} NIC"));
            }
            if *appliances > 0 {
                count.push(format!("アプライアンス {appliances}台"));
            }
            Row::new(vec![
                Cell::from(Span::styled(
                    format!("{} {name}", kind.mark()),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(count.join(" / "), Style::default().fg(DIM))),
                Cell::from(""),
                Cell::from(Span::styled(note.clone(), Style::default().fg(DIM))),
            ])
        }
        MapRow::Nic {
            server,
            power,
            nic,
            ip,
            filter,
            last,
            ..
        } => Row::new(vec![
            Cell::from(Line::from(vec![
                Span::styled(branch(*last), Style::default().fg(DIM)),
                Span::raw(server.clone()),
            ])),
            Cell::from(Span::styled(nic.clone(), Style::default().fg(DIM))),
            Cell::from(if ip.is_empty() {
                Span::styled("—", Style::default().fg(DIM))
            } else {
                Span::raw(ip.clone())
            }),
            Cell::from(Line::from(vec![
                power_mark(*power),
                Span::styled(
                    match filter {
                        Some(name) => format!("  フィルタ {name}"),
                        None => String::new(),
                    },
                    Style::default().fg(DIM),
                ),
            ])),
        ]),
        MapRow::Appliances { count, last } => Row::new(vec![Cell::from(Span::styled(
            format!("{}アプライアンス {count}台", branch(*last)),
            Style::default().fg(DIM),
        ))]),
        MapRow::Empty => Row::new(vec![Cell::from(Span::styled(
            "   （何も繋がっていません）",
            Style::default().fg(DIM),
        ))]),
        MapRow::Spacer => Row::new(vec![Cell::from("")]),
    }
}

/// 木の枝。最後の要素だけ形を変えて、まとまりの終わりが分かるようにする。
fn branch(last: bool) -> &'static str {
    if last { "  └─ " } else { "  ├─ " }
}
