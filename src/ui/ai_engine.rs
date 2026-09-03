//! AI Engine の読み取り専用画面。
//!
//! モデル・利用状況・請求・アカウントはコントロールパネルAPI、ドキュメントと
//! チャンクは RAG API から描く。コントロールパネルAPIが使えない資格情報のときだけ、
//! モデルはアカウントトークンで引ける推論API側（マネージドリソースの共通描画）に落とす。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{DIM, accent, border_style, field, format_datetime, placeholder};
use crate::ai_engine_cloud::CloudField;
use crate::app::{AiEngineTab, App, Loadable};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(area);
    draw_tabs(frame, chunks[0], app);

    match app.ai_engine.tab {
        AiEngineTab::Models => draw_models(frame, chunks[1], app),
        AiEngineTab::Documents => draw_documents(frame, chunks[1], app),
        AiEngineTab::Usage => draw_usage(frame, chunks[1], app),
        AiEngineTab::Billing => draw_billing(frame, chunks[1], app),
        AiEngineTab::Account => draw_account(frame, chunks[1], app),
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

/// 利用状況が対象とする期間。取得側（`usage_period`）と揃えて出す。
const USAGE_PERIOD: &str = "直近30日";

/// 日別の集計なので時刻は落とし、日付だけを出す。
fn format_date(raw: &str) -> String {
    let formatted = format_datetime(raw);
    formatted
        .split_whitespace()
        .next()
        .unwrap_or(&formatted)
        .to_string()
}

fn draw_models(frame: &mut Frame, area: Rect, app: &mut App) {
    // コントロールパネルAPIが使えない資格情報では、アカウントトークンで引ける
    // 推論API側の一覧に落とす。
    if !app.ai_engine_shows_cloud_models() {
        // 資格情報や権限の問題以外（障害など）は、落とさず原因を見せる。
        let failure = match (&app.ai_engine.cloud_models, &app.ai_engine.cloud_auth) {
            (Loadable::Failed(err), _) | (_, Loadable::Failed(err)) => Some(err),
            _ => None,
        };
        if let Some(err) = failure
            && !cloud_fallback_allowed(err)
        {
            super::draw_full_width_error(frame, area, "モデル", err);
            return;
        }
        super::managed_resources::draw(frame, area, app);
        return;
    }

    let chunks = split_with_detail(area);
    match app.visible_ai_engine_cloud_models() {
        Loadable::Idle | Loadable::Loading => draw_pending(frame, chunks[0], "モデル"),
        Loadable::Failed(err) => draw_error(frame, chunks[0], "モデル", &err),
        Loadable::Ready(models) => {
            let rows = models
                .iter()
                .map(|model| {
                    vec![
                        model.name.clone(),
                        model.status_label(),
                        model.features.join("/"),
                        model.tags.join(", "),
                    ]
                })
                .collect();
            draw_table(
                frame,
                chunks[0],
                "モデル",
                ["モデル", "状態", "用途", "タグ"],
                rows,
                [
                    // 名前は一覧の主役なので、切れないよう一番広く取る。
                    Constraint::Percentage(40),
                    Constraint::Length(12),
                    Constraint::Length(14),
                    Constraint::Min(10),
                ],
                &mut app.ai_engine.model_state,
            );
        }
    }
    draw_model_detail(frame, chunks[1], app);
}

fn draw_model_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" モデルの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(model) = app.selected_ai_engine_cloud_model() else {
        frame.render_widget(placeholder("モデルを選択してください").block(block), area);
        return;
    };

    let mut lines = vec![
        field("名前", &model.name),
        // 推論APIのリクエストに書くのはこの値。表示名とは別物。
        field("モデルID", &model.id),
        Line::from(vec![
            Span::styled(super::pad("状態", 14), Style::default().fg(DIM)),
            Span::styled(
                model.status_label(),
                Style::default().fg(model_status_color(&model.status)),
            ),
        ]),
    ];
    let mut push = |label: &str, value: String| {
        if !value.is_empty() {
            lines.push(field(label, &value));
        }
    };
    push("用途", model.features.join(", "));
    push("タグ", model.tags.join(", "));
    push("声色", model.styles.join(", "));
    push("番号", model.number.clone());
    push("利用規約", model.tos_link.clone());

    if model.status == "approval_required" || model.status == "tos_agreement_required" {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "コントロールパネルで申請・同意すると使えます",
            Style::default().fg(Color::Yellow),
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "y: モデルIDをコピー   t: トークン管理",
        Style::default().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        area,
    );
}

/// モデルの状態を色分けする。すぐ使えるものと、ひと手間必要なものを見分ける。
fn model_status_color(status: &str) -> Color {
    match status {
        "available" => Color::Green,
        "approval_required" | "tos_agreement_required" => Color::Yellow,
        "deprecated" => DIM,
        _ => Color::Gray,
    }
}

fn draw_usage(frame: &mut Frame, area: Rect, app: &App) {
    if draw_cloud_auth_error(frame, area, app) {
        return;
    }
    // 列の少ない表を2つ縦に積むと横が余るので、広いときは左右に並べる。
    let areas = split_with_detail(area);
    let request_title = format!("リクエスト数（{USAGE_PERIOD}）");
    match &app.ai_engine.usages {
        Loadable::Ready(items) => draw_static_table(
            frame,
            areas[0],
            &request_title,
            ["日付", "合計", "内訳"],
            items
                .iter()
                .map(|item| {
                    vec![
                        format_date(&item.time),
                        item.total.to_string(),
                        details_text(&item.details),
                    ]
                })
                .collect(),
            [
                Constraint::Length(10),
                Constraint::Length(6),
                Constraint::Min(20),
            ],
        ),
        Loadable::Failed(err) => draw_error(frame, areas[0], &request_title, err),
        Loadable::Idle | Loadable::Loading => draw_pending(frame, areas[0], &request_title),
    }
    let document_title = format!("ドキュメント（{USAGE_PERIOD}）");
    match &app.ai_engine.document_usages {
        Loadable::Ready(items) => draw_static_table(
            frame,
            areas[1],
            &document_title,
            ["日付", "チャンク数"],
            items
                .iter()
                .map(|item| vec![format_date(&item.time), item.chunk_count.to_string()])
                .collect(),
            [Constraint::Length(10), Constraint::Min(10)],
        ),
        Loadable::Failed(err) => draw_error(frame, areas[1], &document_title, err),
        Loadable::Idle | Loadable::Loading => draw_pending(frame, areas[1], &document_title),
    }
}

fn draw_billing(frame: &mut Frame, area: Rect, app: &App) {
    if draw_cloud_auth_error(frame, area, app) {
        return;
    }
    let month = &app.ai_engine.billing_month;
    let Some(Loadable::Ready(bill)) = app.ai_engine.bills.get(month) else {
        match app.ai_engine.bills.get(month) {
            Some(Loadable::Failed(err)) => draw_error(frame, area, "請求", err),
            // 月を移した直後はまだ要求も出ていない。
            _ => draw_pending(frame, area, &format!("請求 {month}")),
        }
        return;
    };

    // 月移動のキーはヒント行が案内するので、見出しには金額の要約だけを置く。
    let mut title = format!(
        "請求 {month} — 合計 {}",
        super::yen(bill.total().round() as i64)
    );
    if !bill.close_date.is_empty() {
        title.push_str(&format!("（締日 {}）", bill.close_date));
    }
    draw_static_table(
        frame,
        area,
        &title,
        ["No", "内容", "利用量", "金額"],
        bill.details
            .iter()
            .map(|detail| {
                vec![
                    detail.no.to_string(),
                    detail.description.clone(),
                    crate::ai_engine_cloud::format_amount(detail.usage),
                    super::yen(detail.amount.round() as i64),
                ]
            })
            .collect(),
        [
            Constraint::Length(4),
            Constraint::Percentage(50),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    );
}

fn draw_account(frame: &mut Frame, area: Rect, app: &App) {
    // 1枚に積むと横が余るので、アカウントと契約プランを左右に分ける。
    let areas = split_with_detail(area);
    let auth = match &app.ai_engine.cloud_auth {
        Loadable::Ready(auth) => auth,
        Loadable::Failed(err) => {
            draw_error(frame, areas[0], "アカウント", err);
            draw_error(frame, areas[1], "契約プラン", err);
            return;
        }
        Loadable::Idle | Loadable::Loading => {
            draw_pending(frame, areas[0], "アカウント");
            draw_pending(frame, areas[1], "契約プラン");
            return;
        }
    };

    draw_fields(
        frame,
        areas[0],
        "アカウント",
        &[
            CloudField {
                label: "アカウントID".to_string(),
                value: auth.account_id.clone(),
            },
            CloudField {
                label: "コード".to_string(),
                value: auth.account_code.clone(),
            },
            CloudField {
                label: "名前".to_string(),
                value: auth.account_name.clone(),
            },
            CloudField {
                label: "会員ID".to_string(),
                value: auth.member_id.clone(),
            },
            CloudField {
                label: "規約同意".to_string(),
                value: format_datetime(&auth.tos_agreed_at),
            },
            CloudField {
                label: "利用開始".to_string(),
                value: format_datetime(&auth.created_at),
            },
        ],
    );

    let mut plan = vec![CloudField {
        label: "プラン".to_string(),
        value: auth.plan.clone(),
    }];
    plan.extend(auth.plan_details.clone());
    draw_fields(frame, areas[1], "契約プラン", &plan);
}

fn draw_cloud_auth_error(frame: &mut Frame, area: Rect, app: &App) -> bool {
    if let Loadable::Failed(err) = &app.ai_engine.cloud_auth {
        draw_error(frame, area, "Cloud API認証", err);
        true
    } else {
        false
    }
}

/// コントロールパネルAPIが使えないときは、アカウントトークンで引ける
/// 推論API側のモデル一覧に落とす。資格情報や権限の問題だけを対象にして、
/// 障害（5xx）はそのまま見せる。
fn cloud_fallback_allowed(error: &str) -> bool {
    error.contains("401 Unauthorized")
        || error.contains("403 Forbidden")
        || error.contains("404 Not Found")
        || error.contains("クラウドAPIキーが設定されていません")
}

fn details_text(details: &[CloudField]) -> String {
    details
        .iter()
        .filter(|field| !field.value.is_empty())
        .map(|field| format!("{}: {}", field.label, field.value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn draw_fields(frame: &mut Frame, area: Rect, title: &str, fields: &[CloudField]) {
    let lines = fields
        .iter()
        .filter(|item| !item.value.is_empty())
        .map(|item| field(&item.label, &item.value))
        .collect::<Vec<_>>();
    let block = Block::bordered()
        .title(format!(" {title} "))
        .border_style(border_style(true))
        .padding(ratatui::widgets::Padding::horizontal(1));
    if lines.is_empty() {
        frame.render_widget(placeholder("項目がありません").block(block), area);
    } else {
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
    }
}

fn draw_static_table<const N: usize>(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    headers: [&str; N],
    rows: Vec<Vec<String>>,
    widths: [Constraint; N],
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
    frame.render_widget(
        Table::new(rows, widths)
            .header(Row::new(headers).style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)))
            .block(block),
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
            placeholder("ドキュメントを選択してください").block(
                Block::bordered()
                    .title(" チャンク ")
                    .border_style(border_style(true)),
            ),
            area,
        );
        return;
    };

    let chunks = match app.visible_ai_engine_chunks() {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(
                placeholder("読み込み中…").block(
                    Block::bordered()
                        .title(" チャンク ")
                        .border_style(border_style(true)),
                ),
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
    // 全幅版（`draw_full_width_error`）と同じ見た目にして、失敗だと一目で分かるようにする。
    frame.render_widget(
        Paragraph::new(err.to_string())
            .style(Style::default().fg(Color::Red))
            .wrap(Wrap { trim: false })
            .block(
                Block::bordered()
                    .title(Span::styled(
                        format!(" {title} "),
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ))
                    .border_style(Style::default().fg(Color::Red))
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            ),
        area,
    );
}

fn draw_message(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    frame.render_widget(
        placeholder(message).block(
            Block::bordered()
                .title(format!(" {title} "))
                .border_style(border_style(true)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_engine_cloud::CloudField;
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

    #[test]
    fn cloud_model_fallback_is_limited_to_auth_or_unavailable_api() {
        assert!(cloud_fallback_allowed(
            "AI Engine コントロールパネルAPIエラー (401 Unauthorized)"
        ));
        assert!(cloud_fallback_allowed(
            "AI Engine コントロールパネルAPIエラー (403 Forbidden)"
        ));
        assert!(cloud_fallback_allowed(
            "AI Engine コントロールパネルAPIエラー (404 Not Found)"
        ));
        assert!(cloud_fallback_allowed(
            "クラウドAPIキーが設定されていません"
        ));
        assert!(!cloud_fallback_allowed(
            "AI Engine コントロールパネルAPIエラー (500 Internal Server Error)"
        ));
    }

    #[test]
    fn cloud_details_skip_empty_values() {
        let details = vec![
            CloudField {
                label: "provider".to_string(),
                value: "sakura".to_string(),
            },
            CloudField {
                label: "empty".to_string(),
                value: String::new(),
            },
        ];
        assert_eq!(details_text(&details), "provider: sakura");
    }
}
