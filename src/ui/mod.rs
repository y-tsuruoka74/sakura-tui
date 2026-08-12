//! 画面描画。

mod apprun;
mod dedicated;
mod detail;
mod overlay;
mod registries;
mod server;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs};

use crate::app::{App, Focus, Mode, Service, StatusKind, Tab};

/// さくらのピンク。
pub const SAKURA: Color = Color::Rgb(0xE9, 0x54, 0x6B);
pub const DIM: Color = Color::DarkGray;

/// 読み込み中に回すスピナー。
const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // ヘッダー
            Constraint::Min(3),    // 本体
            Constraint::Length(1), // ステータス
            Constraint::Length(1), // キーヒント
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], app);
    draw_body(frame, chunks[1], app);
    draw_status(frame, chunks[2], app);
    draw_hints(frame, chunks[3], app);

    overlay::draw(frame, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let spinner = if app.inflight > 0 {
        SPINNER[(app.tick as usize) % SPINNER.len()]
    } else {
        " "
    };
    let mut spans = vec![Span::styled(
        " 🌸 sakura-tui ",
        Style::default()
            .fg(SAKURA)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    )];
    for service in Service::ALL {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!(" {} ", service.title()),
            if service == app.service {
                Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ));
    }
    spans.push(Span::styled(spinner, Style::default().fg(SAKURA)));
    spans.push(Span::raw(" "));
    spans.push(mode_badge(app.mode));
    spans.push(Span::raw(" "));
    // ゾーンはゾーン依存のサービスのときだけ出す。
    if app.service.is_zoned() {
        spans.push(Span::styled(
            format!(" {} ", app.zone),
            Style::default().fg(Color::Cyan),
        ));
    }
    spans.push(Span::styled(
        format!(" {}", app.credential_source.label()),
        Style::default().fg(DIM),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// 現在のモードを示すバッジ。書き込み可のときは目立つようにする。
fn mode_badge(mode: Mode) -> Span<'static> {
    let style = match mode {
        Mode::ReadOnly => Style::default().fg(Color::Green),
        Mode::Write => Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED),
    };
    Span::styled(format!(" {} ", mode.label()), style)
}

fn draw_body(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.service {
        Service::Registry => draw_registry(frame, area, app),
        Service::AppRun => apprun::draw(frame, area, app),
        Service::Dedicated => dedicated::draw(frame, area, app),
        Service::Server => server::draw(frame, area, app),
    }
}

fn draw_registry(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);

    registries::draw(frame, chunks[0], app);

    let detail = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(chunks[1]);
    draw_tabs(frame, detail[0], app);
    detail::draw(frame, detail[1], app);
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| Line::from(format!("{} {}", i + 1, tab.title())))
        .collect();
    let selected = Tab::ALL.iter().position(|t| *t == app.registry.tab).unwrap_or(0);
    let highlight = if app.registry.focus == Focus::Detail {
        Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM).add_modifier(Modifier::BOLD)
    };
    let tabs = Tabs::new(titles)
        .select(selected)
        .highlight_style(highlight)
        .divider(Span::styled("│", Style::default().fg(DIM)))
        .block(Block::default().padding(ratatui::widgets::Padding::horizontal(1)));
    frame.render_widget(tabs, area);
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    // 絞り込み編集中はステータス行を入力欄として使う。
    if app.filtering {
        let line = Line::from(vec![
            Span::styled(" /", Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)),
            Span::raw(app.active_filter().to_string()),
            Span::styled("▏", Style::default().fg(SAKURA)),
            Span::styled(
                "   Enter 確定 · Esc 解除",
                Style::default().fg(DIM),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    // 絞り込みが効いているあいだは常に見えるようにしておく。
    if !app.active_filter().is_empty() {
        let line = Line::from(vec![
            Span::styled(
                format!(" 絞り込み /{}", app.active_filter()),
                Style::default().fg(SAKURA),
            ),
            Span::styled("   / で編集", Style::default().fg(DIM)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let (text, style) = match &app.status {
        Some((text, StatusKind::Error)) => (text.as_str(), Style::default().fg(Color::Red)),
        Some((text, StatusKind::Success)) => (text.as_str(), Style::default().fg(Color::Green)),
        Some((text, StatusKind::Info)) => (text.as_str(), Style::default().fg(DIM)),
        None => ("", Style::default()),
    };
    // 複数行のエラーはステータス行に収まらないので 1 行にまとめる。
    let text = text.replace('\n', " ");
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))),
        area,
    );
}

fn draw_hints(frame: &mut Frame, area: Rect, app: &App) {
    let mut hints: Vec<&str> = vec!["↑↓/jk 移動", "s サービス", "r 更新"];
    match app.service {
        Service::Registry => {
            hints.push("Tab タブ");
            // 書き込み系のキーは、書き込みモードのときだけ案内する。
            if app.mode == Mode::Write {
                match app.registry.tab {
                    Tab::Overview => hints.extend(["n 作成", "E 編集", "D 削除"]),
                    Tab::Users => hints.extend(["a 追加", "e 編集", "d 削除"]),
                    Tab::Images => hints.push("d イメージ削除"),
                }
            }
            if app.registry.tab == Tab::Images {
                if app.is_logged_in() {
                    hints.extend(["L ログイン変更", "O ログアウト"]);
                } else {
                    hints.push("L ログイン");
                }
            }
        }
        Service::AppRun => {
            if app.mode == Mode::Write {
                hints.push("t トラフィック切替");
            }
        }
        // 専有型は閲覧のみなので書き込み系のヒントは無い。
        Service::Dedicated => hints.push("Tab タブ"),
        Service::Server => {
            hints.push("z ゾーン");
            if app.mode == Mode::Write {
                hints.extend(["b 起動", "x 停止", "X 強制停止", "B 強制リセット"]);
            }
        }
    }
    hints.extend(["/ 絞込", "y コピー", "p 認証切替"]);
    hints.push(match app.mode {
        Mode::ReadOnly => "w 書込モードへ",
        Mode::Write => "w 読取専用へ",
    });
    hints.extend(["? ヘルプ", "q 終了"]);

    let mut spans = vec![Span::raw(" ")];
    for (i, hint) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(DIM)));
        }
        let (key, rest) = hint.split_once(' ').unwrap_or((hint, ""));
        spans.push(Span::styled(
            key.to_string(),
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {rest}"),
            Style::default().fg(Color::Gray),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// フォーカスの有無で枠線の色を変える。
pub fn border_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(SAKURA)
    } else {
        Style::default().fg(DIM)
    }
}

/// 全角を含むラベルを表示セル数で右詰めする（`format!("{:width$}")` は
/// 文字数で数えるため日本語ラベルだと桁がずれる）。
pub fn pad(label: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let pad = width.saturating_sub(label.width());
    format!("{label}{}", " ".repeat(pad))
}

/// ラベル幅を表示セル数で揃えた `ラベル  値` の行。
pub fn field(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(pad(label, 14), Style::default().fg(DIM)),
        Span::raw(value.to_string()),
    ])
}

pub fn placeholder(text: &str) -> Paragraph<'static> {
    Paragraph::new(text.to_string())
        .style(Style::default().fg(DIM))
        .wrap(ratatui::widgets::Wrap { trim: false })
}

/// 一覧そのものの取得に失敗したときは、狭いペインに押し込めず画面幅いっぱいに出す。
/// （権限エラーの案内など、複数行のメッセージが読めなくなるため）
pub fn draw_full_width_error(frame: &mut Frame, area: Rect, title: &str, err: &str) {
    frame.render_widget(
        Paragraph::new(err.to_string())
            .style(Style::default().fg(Color::Red))
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(
                Block::bordered()
                    .title(Span::styled(
                        format!(" {title} "),
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ))
                    .border_style(Style::default().fg(Color::Red))
                    .padding(ratatui::widgets::Padding::new(1, 1, 0, 0)),
            ),
        area,
    );
}

/// AppRun のステータス文字列を色分けする。
pub fn status_color(status: &str) -> Color {
    match status.to_ascii_lowercase().as_str() {
        s if s.contains("success") || s.contains("running") || s.contains("healthy") => {
            Color::Green
        }
        s if s.contains("fail") || s.contains("error") => Color::Red,
        s if s.contains("progress") || s.contains("pending") || s.contains("deploy") => {
            Color::Yellow
        }
        _ => Color::Gray,
    }
}

pub fn error_paragraph(err: &str) -> Paragraph<'static> {
    Paragraph::new(err.to_string())
        .style(Style::default().fg(Color::Red))
        .wrap(ratatui::widgets::Wrap { trim: false })
}

/// Unix 秒を `YYYY-MM-DD HH:MM` に整形する。
/// （専有型 API は日時を文字列ではなく整数で返す）
pub fn format_unix(seconds: i64) -> String {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// 日時文字列を `YYYY-MM-DD HH:MM` に整形する。解析できなければそのまま返す。
pub fn format_datetime(raw: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| raw.to_string())
}
