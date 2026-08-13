//! 画面描画。

mod account;
mod apprun;
mod billing;
mod dedicated;
mod detail;
mod observability;
mod overlay;
mod registries;
mod server;
mod switch;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs};

use crate::app::{App, Focus, Mode, Service, StatusKind, Tab};
use crate::config::CredentialSource;

/// 既定のアクセント色（さくらのピンク）。
pub const SAKURA: Color = Color::Rgb(0xE9, 0x54, 0x6B);

thread_local! {
    /// 描画中のアクセント色。
    ///
    /// 使用中の認証情報に色を割り当てていればその色を使う。どの契約を見ているかが
    /// 枠線や見出しの色で分かるようにするため。描画は単一スレッドで、フレームごとに
    /// `draw` の先頭で設定するだけなので、引数で持ち回らずここに置く。
    static ACCENT: std::cell::Cell<Color> = const { std::cell::Cell::new(SAKURA) };
}

/// 現在のアクセント色。
pub fn accent() -> Color {
    ACCENT.with(std::cell::Cell::get)
}
pub const DIM: Color = Color::DarkGray;

/// 読み込み中に回すスピナー。
const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub fn draw(frame: &mut Frame, app: &mut App) {
    // 認証情報に割り当てた色を、この画面全体のアクセント色にする。
    let color = app
        .config
        .profile_color(&app.credential_source)
        .and_then(parse_color)
        .unwrap_or(SAKURA);
    ACCENT.with(|current| current.set(color));

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
    let mut spans = vec![
        Span::styled(
            " sakura-tui ",
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        separator(),
        Span::styled(
            app.service.title(),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" (s)", Style::default().fg(DIM)),
        separator(),
        mode_badge(app.mode),
    ];
    // ゾーンはゾーン依存のサービスのときだけ出す。
    if app.service.is_zoned() {
        spans.push(separator());
        spans.push(Span::styled(
            format!("ゾーン {}", app.zone),
            Style::default().fg(Color::Cyan),
        ));
    }
    // 本番以外に繋いでいるときは、それが分かるようにする。
    if app.api_root != crate::config::DEFAULT_API_ROOT {
        spans.push(separator());
        spans.push(Span::styled(
            format!(" {} ", environment_label(&app.api_root)),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(separator());
    spans.extend(credential_badge(
        &app.credential_source,
        app.config
            .profile_color(&app.credential_source)
            .and_then(parse_color),
    ));
    spans.push(Span::styled(" (p)", Style::default().fg(DIM)));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(spinner, Style::default().fg(accent())));

    // 分類を添えて、今どのあたりを見ているのかが分かるようにする。
    // ただしプロファイル名やモードの方が大事なので、幅が足りなければ落とす。
    let category = Span::styled(
        format!("{} / ", app.service.category().title()),
        Style::default().fg(DIM),
    );
    if width(&spans) + width(std::slice::from_ref(&category)) <= area.width as usize {
        // サービス名の直前（"sakura-tui" と区切りの後ろ）に差し込む。
        spans.insert(2, category);
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// 並べたときの表示セル数。
pub fn width(spans: &[Span]) -> usize {
    use unicode_width::UnicodeWidthStr;
    spans.iter().map(|s| s.content.width()).sum()
}

/// 接続先の短い呼び名。本番以外に繋いでいることが一目で分かるようにする。
fn environment_label(api_root: &str) -> String {
    if api_root == crate::config::TEST_API_ROOT {
        return "cloud-test".to_string();
    }
    // `https://host/xxx/zone` の `xxx` を環境名とみなす。
    api_root
        .trim_end_matches("/zone")
        .rsplit('/')
        .next()
        .unwrap_or(api_root)
        .to_string()
}

/// ヘッダーの項目を区切る。
fn separator() -> Span<'static> {
    Span::styled(" │ ", Style::default().fg(DIM))
}

/// プロファイルに割り当てられる色。ピッカーの `c` キーで順に切り替える。
pub const PROFILE_COLORS: [&str; 6] = ["red", "yellow", "green", "cyan", "blue", "magenta"];

/// 設定ファイルの色名を実際の色にする。
///
/// 名前のほか `#RRGGBB` も受け取る。解釈できない値は既定色として扱う。
pub fn parse_color(name: &str) -> Option<Color> {
    let name = name.trim();
    if let Some(hex) = name.strip_prefix('#')
        && hex.len() == 6
        && let Ok(value) = u32::from_str_radix(hex, 16)
    {
        return Some(Color::Rgb(
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
        ));
    }
    match name.to_ascii_lowercase().as_str() {
        "red" => Some(Color::Red),
        "yellow" => Some(Color::Yellow),
        "green" => Some(Color::Green),
        "cyan" => Some(Color::Cyan),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "gray" | "grey" => Some(Color::Gray),
        "white" => Some(Color::White),
        _ => None,
    }
}

/// どの契約で見ているかを示すバッジ。
///
/// 色は設定ファイルで指定されたものを使う（ピッカーの `c` キーでも変えられる）。
/// dev と prod のように名前が似ている契約を、自分で決めた色で区別するため。
pub fn credential_badge(source: &CredentialSource, color: Option<Color>) -> Vec<Span<'static>> {
    let style = match color {
        Some(color) => Style::default().fg(color).add_modifier(Modifier::BOLD),
        None => Style::default().fg(Color::Gray),
    };
    vec![
        Span::styled("◆ ", style),
        Span::styled(source.label(), style),
    ]
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
        Service::Switch => switch::draw(frame, area, app),
        Service::Dns => observability::draw_dns(frame, area, app),
        Service::SimpleMonitor => observability::draw_simple_monitor(frame, area, app),
        Service::Secrets => observability::draw_secrets(frame, area, app),
        Service::Monitoring => observability::draw_monitoring(frame, area, app),
        Service::Account => account::draw(frame, area, app),
        Service::Billing => billing::draw(frame, area, app),
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
    let selected = Tab::ALL
        .iter()
        .position(|t| *t == app.registry.tab)
        .unwrap_or(0);
    let highlight = if app.registry.focus == Focus::Detail {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
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
            Span::styled(
                " /",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(app.active_filter().to_string()),
            Span::styled("▏", Style::default().fg(accent())),
            Span::styled("   Enter 確定 · Esc 解除", Style::default().fg(DIM)),
        ]);
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    // 絞り込みが効いているあいだは常に見えるようにしておく。
    if !app.active_filter().is_empty() {
        let line = Line::from(vec![
            Span::styled(
                format!(" 絞り込み /{}", app.active_filter()),
                Style::default().fg(accent()),
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
        // 以下はすべて閲覧のみなので書き込み系のヒントは無い。
        Service::Dedicated => hints.push("Tab タブ"),
        Service::Dns => hints.push(if app.dns.focus == crate::app::ListFocus::Left {
            "Enter レコードへ"
        } else {
            "Esc DNSゾーンへ"
        }),
        Service::SimpleMonitor | Service::Account => {}
        Service::Secrets => {
            hints.push("z ゾーン");
            hints.push(if app.secrets.focus == crate::app::ListFocus::Left {
                "Enter シークレットへ"
            } else {
                "Esc Vault へ"
            });
            hints.push("u 値を表示");
        }
        Service::Monitoring => {
            hints.push("z ゾーン");
            hints.push(if app.monitoring.focus == crate::app::ListFocus::Left {
                "Enter 中身へ"
            } else {
                "Esc プロジェクトへ"
            });
            hints.push("Tab タブ");
        }
        Service::Billing => {
            // 月一覧では ↑↓ が月、明細に入ると ↑↓ が明細になる。
            hints.extend(["↑↓ 月", "←→ 年"]);
            hints.push(if app.billing.focus == crate::app::BillingFocus::Bills {
                "Enter 明細へ"
            } else {
                "Esc 月一覧へ"
            });
            hints.push("Tab タブ");
        }
        Service::Server => {
            hints.push("z ゾーン");
            if app.mode == Mode::Write {
                hints.extend(["b 起動", "x 停止", "X 強制停止", "B 強制リセット"]);
            }
        }
        Service::Switch => {
            hints.push("z ゾーン");
            if app.mode == Mode::Write {
                hints.extend(["n 作成", "E 編集", "D 削除"]);
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
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
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
        Style::default().fg(accent())
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

#[cfg(test)]
mod color_tests {
    use super::*;

    #[test]
    fn parses_named_colors() {
        assert_eq!(parse_color("red"), Some(Color::Red));
        assert_eq!(parse_color("  Cyan "), Some(Color::Cyan));
        assert_eq!(parse_color("grey"), Some(Color::Gray));
    }

    #[test]
    fn parses_hex_colors() {
        assert_eq!(parse_color("#ff8800"), Some(Color::Rgb(0xFF, 0x88, 0x00)));
        assert_eq!(parse_color("#000000"), Some(Color::Rgb(0, 0, 0)));
    }

    /// 解釈できない値は既定色扱いにして、落とさない。
    #[test]
    fn unknown_values_fall_back_to_default() {
        for value in ["", "むらさき", "#fff", "#gggggg", "rgb(1,2,3)"] {
            assert_eq!(parse_color(value), None, "{value}");
        }
    }

    /// パレットは全て解釈できること（`c` キーで循環させるため）。
    #[test]
    fn palette_is_all_parseable() {
        for name in PROFILE_COLORS {
            assert!(parse_color(name).is_some(), "{name}");
        }
    }
}
