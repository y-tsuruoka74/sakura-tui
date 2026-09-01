//! AI Engine の読み取り専用画面。
//!
//! モデル一覧はマネージドリソースの共通描画を借りる。RAG のドキュメントと
//! チャンクはこのファイルで描く。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{DIM, accent, border_style, field, format_datetime, placeholder};
use crate::app::{AiEngineTab, App, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.ai_engine.tab {
        AiEngineTab::Models => super::managed_resources::draw(frame, chunks[1], app),
        AiEngineTab::Documents => draw_documents(frame, chunks[1], app),
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = AiEngineTab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!("{} {}", index + 1, tab.title())))
        .collect();
    let selected = AiEngineTab::ALL
        .iter()
        .position(|tab| *tab == app.ai_engine.tab)
        .unwrap_or_default();
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM))),
        area,
    );
}

fn draw_documents(frame: &mut Frame, area: Rect, app: &mut App) {
    if let Loadable::Failed(err) = app.visible_ai_engine_documents() {
        draw_error(frame, area, "ドキュメント", &err);
        return;
    }
    // 上段に一覧と詳細、下段にチャンク本文。タブを跨がずに中身を追えるようにする。
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(45), Constraint::Min(6)])
        .split(area);
    let chunks = split_with_detail(rows[0]);

    match app.visible_ai_engine_documents() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], "ドキュメント"),
        Loadable::Failed(err) => draw_error(frame, chunks[0], "ドキュメント", &err),
        Loadable::Ready(items) => {
            let rows = items
                .into_iter()
                .map(|item| {
                    let status = item.status_label();
                    vec![
                        item.name,
                        status,
                        item.model,
                        item.chunk_count.to_string(),
                        item.tags.join(", "),
                    ]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                "ドキュメント",
                ["名前", "状態", "モデル", "チャンク", "タグ"],
                rows,
                [
                    Constraint::Percentage(28),
                    Constraint::Percentage(14),
                    Constraint::Percentage(24),
                    Constraint::Length(8),
                    Constraint::Min(10),
                ],
                &mut app.ai_engine.document_state,
            );
        }
    }
    draw_document_detail(frame, chunks[1], app);
    draw_chunk_body(frame, rows[1], app);
}

fn draw_document_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" ドキュメントの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(document) = app.selected_ai_engine_document() else {
        frame.render_widget(
            placeholder("ドキュメントを選択してください").block(block),
            area,
        );
        return;
    };

    let mut lines = vec![
        field("名前", &document.name),
        field("状態", &document.status_label()),
    ];
    let mut push = |label: &str, value: String| {
        if !value.is_empty() {
            lines.push(field(label, &value));
        }
    };
    push("モデル", document.model.clone());
    if document.chunk_size != 0 {
        // ラベルは全角7文字で pad(14) を使い切り、値と詰まって見えるため短くする。
        push("分割サイズ", document.chunk_size.to_string());
    }
    push("チャンク数", document.chunk_count.to_string());
    push("タグ", document.tags.join(", "));
    push("ID", document.id.clone());
    if !document.created_at.is_empty() {
        push("作成日時", format_datetime(&document.created_at));
    }

    // 取り込みに失敗した理由は埋もれないよう赤で最後に出す。
    if document.failed() && !document.error_message.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            document.error_message.clone(),
            Style::default().fg(Color::Red),
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "本文は下のチャンク欄（J/K でスクロール）",
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "y: リソースIDをコピー   t: トークン管理",
        Style::default().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// 選択中ドキュメントのチャンクを、区切り付きの本文として1つのペインに流す。
///
/// チャンク単位で選ぶより通して読めた方が中身を把握しやすいので、
/// 一覧ではなくスクロールする本文として見せる。
fn draw_chunk_body(frame: &mut Frame, area: Rect, app: &App) {
    let Some(document) = app.selected_ai_engine_document() else {
        frame.render_widget(
            placeholder("ドキュメントを選択してください")
                .block(Block::bordered().title(" チャンク ")),
            area,
        );
        return;
    };

    let chunks = match app.visible_ai_engine_chunks() {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(
                placeholder("読み込み中…").block(Block::bordered().title(" チャンク ")),
                area,
            );
            return;
        }
        Loadable::Failed(err) => {
            draw_error(frame, area, "チャンク", &err);
            return;
        }
        Loadable::Ready(chunks) => chunks,
    };

    let lines = chunk_body_lines(&chunks);
    // 本文の行数を超えてスクロールしても空白だけにならないよう頭打ちにする。
    let viewport = area.height.saturating_sub(2);
    let max_scroll = (lines.len() as u16).saturating_sub(viewport);
    let scroll = app.ai_engine.chunk_scroll.min(max_scroll);

    let title = if chunks.is_empty() {
        format!(" チャンク — {} ", document.name)
    } else {
        format!(
            " チャンク ({}) — {}行目/{}  J/K スクロール ",
            chunks.len(),
            scroll + 1,
            lines.len()
        )
    };
    let block = Block::bordered()
        .title(title)
        .border_style(border_style(true))
        .padding(ratatui::widgets::Padding::horizontal(1));

    if lines.is_empty() {
        frame.render_widget(placeholder("チャンクがありません").block(block), area);
        return;
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(block),
        area,
    );
}

/// チャンクを区切り見出し付きの行へ並べる。
fn chunk_body_lines(chunks: &[crate::ai_rag::RagChunk]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for chunk in chunks {
        let mut heading = format!("── チャンク {} ", chunk.index);
        if !chunk.metadata.is_empty() {
            heading.push_str(&format!("（{}）", chunk.metadata));
        }
        lines.push(Line::from(Span::styled(
            heading,
            Style::default().fg(DIM).add_modifier(Modifier::BOLD),
        )));
        for line in chunk.content.lines() {
            lines.push(Line::raw(line.to_string()));
        }
        lines.push(Line::raw(""));
    }
    lines
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_rag::RagChunk;

    /// チャンクは区切り見出し付きで1本の本文に並べる。
    /// メタデータがあれば見出しに添える。
    #[test]
    fn chunk_body_joins_chunks_with_headings() {
        let chunks = vec![
            RagChunk {
                index: 0,
                content: "一行目\n二行目".to_string(),
                metadata: "page=3".to_string(),
                document_id: "d".to_string(),
            },
            RagChunk {
                index: 1,
                content: "つづき".to_string(),
                metadata: String::new(),
                document_id: "d".to_string(),
            },
        ];
        let lines: Vec<String> = chunk_body_lines(&chunks)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect();

        assert_eq!(
            lines,
            vec![
                "── チャンク 0 （page=3）",
                "一行目",
                "二行目",
                "",
                "── チャンク 1 ",
                "つづき",
                "",
            ]
        );
    }

    /// チャンクが無ければ行も作らない。呼び出し側で「ありません」を出す。
    #[test]
    fn chunk_body_is_empty_without_chunks() {
        assert!(chunk_body_lines(&[]).is_empty());
    }
}
