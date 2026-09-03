//! 画面描画。

mod account;
mod ai_engine;
mod api_gateway;
mod apprun;
mod billing;
mod cloud_resources;
mod cloudhsm;
mod dedicated;
mod detail;
mod managed_resources;
mod network_map;
mod networking_suite;
mod nosql;
mod observability;
mod overlay;
mod packet_filter;
mod registries;
mod security_control;
mod seg;
mod server;
mod ssh_key;
mod switch;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Tabs};

use crate::app::{AiEngineTab, App, Focus, Mode, Overlay, Service, StatusKind, Tab};
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

    // ヒントは幅次第で2行になるので、先に組み立てて高さを決める。
    let hints = fit_hints(&hints_for(app), frame.area().width as usize);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                  // ヘッダー
            Constraint::Min(3),                     // 本体
            Constraint::Length(1),                  // ステータス
            Constraint::Length(hints.len() as u16), // キーヒント
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], app);
    draw_body(frame, chunks[1], app);
    draw_status(frame, chunks[2], app);
    draw_hints(frame, chunks[3], &hints);

    overlay::draw(frame, app);
}

/// ヘッダーの部品。幅が足りないときは、この番号が大きいものから落とす。
///
/// 誰として・どのモードで操作しているかは、書き込みの前に必ず見える必要がある
/// ので落とさない（`KEEP`）。
struct HeaderPart {
    drop_order: u8,
    spans: Vec<Span<'static>>,
}

impl HeaderPart {
    /// 落としてはいけない部品。
    const KEEP: u8 = 0;

    fn new(drop_order: u8, spans: Vec<Span<'static>>) -> Self {
        Self { drop_order, spans }
    }

    /// 区切りを前に付けた部品。落とすと区切りも一緒に消える。
    fn after_separator(drop_order: u8, spans: Vec<Span<'static>>) -> Self {
        let mut with_sep = vec![separator()];
        with_sep.extend(spans);
        Self::new(drop_order, with_sep)
    }
}

/// 幅に収まるまで、優先度の低い部品から落とす。
fn fit_header(parts: Vec<HeaderPart>, area_width: usize) -> Vec<Span<'static>> {
    let mut parts = parts;
    loop {
        let total: usize = parts.iter().map(|p| width(&p.spans)).sum();
        if total <= area_width {
            break;
        }
        // 落としてよいもののうち、いちばん優先度が低いものを1つ外す。
        let Some((index, _)) = parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.drop_order != HeaderPart::KEEP)
            .max_by_key(|(_, p)| p.drop_order)
        else {
            break;
        };
        parts.remove(index);
    }
    let mut spans: Vec<Span<'static>> = parts.into_iter().flat_map(|p| p.spans).collect();
    // 先頭の部品を落とすと、次の部品が持つ区切りが行頭に残る。
    if spans
        .first()
        .is_some_and(|span| span.content == separator().content)
    {
        spans.remove(0);
    }
    spans
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let choosing_service = matches!(
        app.overlay,
        Some(Overlay::ServicePicker { initial: true, .. })
    );
    let spinner = if app.inflight > 0 {
        SPINNER[(app.tick as usize) % SPINNER.len()]
    } else {
        " "
    };

    let mut parts = vec![HeaderPart::new(
        3,
        vec![Span::styled(
            " sakura-tui ",
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )],
    )];

    // 分類はサービス名の頭に付ける。別の部品にすると区切りが between に
    // 入ってしまい「ネットワーク / │ 接続マップ」のように読めなくなる。
    let service_title = if choosing_service {
        "サービスを選択"
    } else {
        app.service.title()
    };
    parts.push(HeaderPart::after_separator(
        2,
        vec![Span::styled(
            service_title,
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        )],
    ));
    parts.push(HeaderPart::new(
        5,
        vec![Span::styled(" (s)", Style::default().fg(DIM))],
    ));

    parts.push(HeaderPart::after_separator(
        HeaderPart::KEEP,
        vec![mode_badge(app.mode)],
    ));

    // ゾーンはゾーン依存のサービスのときだけ出す。
    if !choosing_service && app.service.is_zoned() {
        parts.push(HeaderPart::after_separator(
            2,
            vec![Span::styled(
                format!("ゾーン {}", app.zone),
                Style::default().fg(Color::Cyan),
            )],
        ));
    }
    // 本番以外に繋いでいるときは、それが分かるようにする。落とさない。
    if app.api_root != crate::config::DEFAULT_API_ROOT {
        parts.push(HeaderPart::after_separator(
            HeaderPart::KEEP,
            vec![Span::styled(
                format!(" {} ", environment_label(&app.api_root)),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )],
        ));
    }

    if app.has_credentials {
        parts.push(HeaderPart::after_separator(
            HeaderPart::KEEP,
            credential_badge(
                &app.credential_source,
                app.config
                    .profile_color(&app.credential_source)
                    .and_then(parse_color),
            ),
        ));
        parts.push(HeaderPart::new(
            5,
            vec![Span::styled(" (p)", Style::default().fg(DIM))],
        ));
    } else {
        parts.push(HeaderPart::after_separator(
            HeaderPart::KEEP,
            vec![Span::styled(
                " 認証情報未設定 ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )],
        ));
    }
    parts.push(HeaderPart::new(
        4,
        vec![
            Span::raw("  "),
            Span::styled(spinner, Style::default().fg(accent())),
        ],
    ));

    let mut spans = fit_header(parts, area.width as usize);
    // 分類は「今どのあたりを見ているか」の手がかりだが、プロファイル名や
    // モードほど大事ではない。他が収まったうえで余りがあるときだけ添える。
    if !choosing_service {
        let category = Span::styled(
            format!("{} / ", app.service.category().title()),
            Style::default().fg(DIM),
        );
        if let Some(at) = spans.iter().position(|span| span.content == service_title)
            && width(&spans) + width(std::slice::from_ref(&category)) <= area.width as usize
        {
            spans.insert(at, category);
        }
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
    let area = service_content_area(area);
    match app.service {
        Service::Registry => draw_registry(frame, area, app),
        Service::AppRun => apprun::draw(frame, area, app),
        Service::ApiGateway => api_gateway::draw(frame, area, app),
        Service::NoSql => nosql::draw(frame, area, app),
        Service::Seg => seg::draw(frame, area, app),
        Service::SecurityControl => security_control::draw(frame, area, app),
        Service::CloudHsm => cloudhsm::draw(frame, area, app),
        Service::NetworkingSuite => networking_suite::draw(frame, area, app),
        Service::AiEngine => ai_engine::draw(frame, area, app),
        Service::Dedicated => dedicated::draw(frame, area, app),
        Service::Server => server::draw(frame, area, app),
        Service::SshKey => ssh_key::draw(frame, area, app),
        Service::Switch => switch::draw(frame, area, app),
        Service::NetworkMap => network_map::draw(frame, area, app),
        Service::PacketFilter => packet_filter::draw(frame, area, app),
        Service::Disk
        | Service::Archive
        | Service::IsoImage
        | Service::Internet
        | Service::Bridge
        | Service::LoadBalancer
        | Service::VpcRouter
        | Service::MobileGateway
        | Service::Database
        | Service::Nfs => cloud_resources::draw(frame, area, app),
        Service::ObjectStorage
        | Service::SimpleMq
        | Service::SimpleNotification
        | Service::EventBus
        | Service::Workflows
        | Service::WebAccel
        | Service::EnhancedLoadBalancer
        | Service::LocalRouter
        | Service::Gslb
        | Service::Kms
        | Service::Iam
        | Service::AutoScale
        | Service::EnhancedDb
        | Service::AutoBackup => managed_resources::draw(frame, area, app),
        Service::Dns => observability::draw_dns(frame, area, app),
        Service::SimpleMonitor => observability::draw_simple_monitor(frame, area, app),
        Service::Secrets => observability::draw_secrets(frame, area, app),
        Service::Monitoring => observability::draw_monitoring(frame, area, app),
        Service::Account => account::draw(frame, area, app),
        Service::Billing => billing::draw(frame, area, app),
    }
}

/// 各サービス画面に共通の外側余白を設ける。
///
/// 幅や高さが足りない端末では段階的に余白を落とし、情報そのものを優先する。
fn service_content_area(area: Rect) -> Rect {
    let horizontal = if area.width >= 72 {
        2
    } else if area.width >= 40 {
        1
    } else {
        0
    };
    let vertical = u16::from(area.height >= 16);
    Rect {
        x: area.x.saturating_add(horizontal),
        y: area.y.saturating_add(vertical),
        width: area.width.saturating_sub(horizontal * 2),
        height: area.height.saturating_sub(vertical * 2),
    }
}

fn draw_registry(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(38),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    registries::draw(frame, chunks[0], app);

    let detail = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(chunks[2]);
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
    // 行を増やすと本文の高さがメッセージのたびに変わって落ち着かないので、
    // 1 行のまま切る。切ったことは … で分かるようにする。
    let text = clip(&text, area.width.saturating_sub(1) as usize);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))),
        area,
    );
}

/// その画面で使えるキーの一覧。
///
/// 見ているペインで使えないキーは出さない。行に収まる数が限られるので、
/// 出しても押せないキーに桁を使わない。
fn hints_for(app: &App) -> Vec<&'static str> {
    let mut hints: Vec<&'static str> = vec!["↑↓/jk 移動", "s サービス", "r 更新"];
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
        Service::Dedicated => hints.push("Tab タブ"),
        Service::ApiGateway => hints.push("←→/hl タブ"),
        Service::NoSql => hints.push("←→/hl タブ"),
        Service::Seg => hints.push("←→/hl タブ"),
        Service::SecurityControl => hints.push("←→/hl タブ"),
        Service::CloudHsm => hints.push("←→/hl タブ"),
        Service::NetworkingSuite => {
            hints.push("←→/hl タブ");
        }
        Service::AiEngine => {
            hints.extend(["←→/hl・1-5 タブ", "t トークン管理"]);
            if app.ai_engine.tab == AiEngineTab::Documents {
                hints.push("J/K 本文");
            }
            if app.ai_engine.tab == AiEngineTab::Billing {
                hints.push("[/] 請求月");
            }
            // 書き込み系のキーは、書き込みモードのときだけ案内する。
            if app.mode == Mode::Write && app.ai_engine.tab == AiEngineTab::Documents {
                hints.extend(["n アップロード", "e 編集", "d 削除"]);
            }
        }
        Service::Dns => {
            hints.push(if app.dns.focus == crate::app::ListFocus::Left {
                "Enter レコードへ"
            } else {
                "Esc DNSゾーンへ"
            });
            if app.mode == Mode::Write {
                if app.dns.focus == crate::app::ListFocus::Left {
                    hints.extend(["n ゾーン作成", "E 編集", "D 削除"]);
                } else {
                    hints.push("a レコード追加");
                    hints.extend(["e 編集", "d 削除"]);
                }
            }
        }
        Service::SimpleMonitor => {
            if app.mode == Mode::Write {
                hints.extend(["n 作成", "E 編集", "t 有効/停止", "D 削除"]);
            }
        }
        Service::Account => {}
        Service::Secrets => {
            hints.push(if app.secrets.focus == crate::app::ListFocus::Left {
                "Enter シークレットへ"
            } else {
                "Esc Vault へ"
            });
            hints.push("u 値を表示");
            if app.mode == Mode::Write {
                if app.secrets.focus == crate::app::ListFocus::Left {
                    hints.extend(["n Vault作成", "E 編集", "D 削除"]);
                } else {
                    hints.extend(["a 登録", "e 新バージョン", "d 削除"]);
                }
            }
        }
        Service::Monitoring => {
            hints.push("z ゾーン");
            hints.push(if app.monitoring.focus == crate::app::ListFocus::Left {
                "Enter 中身へ"
            } else if app.monitoring.tab == crate::app::MonitoringTab::Storages {
                "Esc ストレージへ"
            } else if matches!(
                app.monitoring.tab,
                crate::app::MonitoringTab::LogRoutings
                    | crate::app::MonitoringTab::MetricsRoutings
                    | crate::app::MonitoringTab::Dashboards
            ) {
                "Esc"
            } else {
                "Esc プロジェクトへ"
            });
            hints.push("Tab タブ");
            if app.mode == Mode::Write
                && app.monitoring.focus == crate::app::ListFocus::Left
                && !matches!(
                    app.monitoring.tab,
                    crate::app::MonitoringTab::Storages
                        | crate::app::MonitoringTab::LogRoutings
                        | crate::app::MonitoringTab::MetricsRoutings
                        | crate::app::MonitoringTab::Dashboards
                )
            {
                hints.extend(["n プロジェクト作成", "E 編集", "D 削除"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::Rules
            {
                hints.extend(["a ルール作成", "e 編集", "d 削除"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::LogMeasureRules
            {
                hints.extend(["a ログ計測作成", "e 編集", "d 削除"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::LogRoutings
            {
                hints.extend(["a ログ転送作成", "e 編集", "d 削除"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::MetricsRoutings
            {
                hints.extend(["a メトリクス転送作成", "e 編集", "d 削除"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::Dashboards
            {
                hints.extend(["a ダッシュボード作成", "e 編集", "d 削除"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::NotificationTargets
            {
                hints.extend(["a 通知先作成", "e 編集", "d 削除"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::NotificationRoutings
            {
                hints.extend(["a 通知経路作成", "e 編集", "d 削除", "[ ] 並べ替え"]);
            } else if app.mode == Mode::Write
                && app.monitoring.tab == crate::app::MonitoringTab::Storages
                && app.monitoring.focus == crate::app::ListFocus::Left
            {
                hints.extend(["n ストレージ作成", "E 編集", "D 削除", "t 保持期間"]);
            } else if app.monitoring.tab == crate::app::MonitoringTab::Storages
                && app.monitoring.focus == crate::app::ListFocus::Right
                && app
                    .selected_storage()
                    .is_some_and(|storage| storage.supports_access_keys())
            {
                hints.push("u シークレット表示");
                if app.mode == Mode::Write {
                    hints.extend(["a キー作成", "e 説明編集", "d 削除"]);
                }
            }
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
        Service::SshKey => {
            if app.mode == Mode::Write {
                hints.extend(["n 登録", "E 編集", "D 削除"]);
            }
        }
        Service::Server => {
            hints.push("z ゾーン");
            hints.push("Tab サーバー/NIC");
            if app.mode == Mode::Write {
                // NIC を見ているときはサーバーの電源操作を出さない。
                // 同じ n / D が別のものを指すので、今の相手だけ案内する。
                if app.server.focus == crate::app::ListFocus::Right {
                    hints.extend(["n NIC追加", "D NIC削除", "c 接続先", "f フィルタ"]);
                } else {
                    hints.extend([
                        "n 作成",
                        "D 削除",
                        "P プラン変更",
                        "b 起動",
                        "x 停止",
                        "X 強制停止",
                        "B 強制リセット",
                    ]);
                }
            }
        }
        Service::NetworkMap => {
            hints.push("z ゾーン");
            hints.push("Enter サーバーへ");
        }
        Service::Switch => {
            hints.push("z ゾーン");
            if app.mode == Mode::Write {
                hints.extend(["n 作成", "E 編集", "D 削除"]);
            }
        }
        Service::Disk => {
            hints.push("z ゾーン");
            if app.mode == Mode::Write {
                hints.extend(["n 作成", "D 削除", "c 接続", "C 切断"]);
            }
        }
        Service::PacketFilter => {
            hints.push("z ゾーン");
            hints.push("Tab フィルタ/ルール");
            if app.mode == Mode::Write {
                if app.packet_filter.focus == crate::app::ListFocus::Right {
                    hints.extend(["n ルール追加", "E 編集", "D 削除", "[ ] 並べ替え"]);
                } else {
                    hints.extend(["n フィルタ作成", "E 編集", "D 削除"]);
                }
            }
        }
        Service::Archive => {
            hints.push("z ゾーン");
            if app.mode == Mode::Write {
                hints.extend(["n ディスクから作成", "D 削除"]);
            }
        }
        Service::AutoBackup => {
            hints.push("z ゾーン");
            if app.mode == Mode::Write {
                hints.extend(["n 作成", "E 編集", "D 削除"]);
            }
        }
        Service::IsoImage
        | Service::Internet
        | Service::Bridge
        | Service::LoadBalancer
        | Service::VpcRouter
        | Service::MobileGateway
        | Service::Database
        | Service::Nfs => hints.push("z ゾーン"),
        Service::ObjectStorage
        | Service::SimpleMq
        | Service::SimpleNotification
        | Service::EventBus
        | Service::Workflows
        | Service::WebAccel
        | Service::EnhancedLoadBalancer
        | Service::LocalRouter
        | Service::Gslb
        | Service::Kms
        | Service::Iam
        | Service::AutoScale
        | Service::EnhancedDb => {
            if app.service == Service::Iam && app.mode == Mode::Write {
                hints.extend([
                    "u ユーザー作成",
                    "U グループ作成",
                    "P プロジェクト作成",
                    "N SP作成",
                    "E 編集",
                    "D 削除",
                    "g ロール付与",
                    "G ロール解除",
                ]);
            }
            if app.service == Service::Iam {
                hints.push("a IAM認証");
            }
        }
    }
    hints.extend(["/ 絞込", "y コピー", "p 認証切替"]);
    hints.push(match app.mode {
        Mode::ReadOnly => "w 書込モードへ",
        Mode::Write => "w 読取専用へ",
    });
    hints.extend(HINT_TAIL);
    hints
}

/// 何があっても最後まで残すキー。
///
/// 隠れたキーの調べ方（`?`）と抜け方（`q`）が消えると詰むため。
const HINT_TAIL: [&str; 2] = ["? ヘルプ", "q 終了"];
/// ヒントに使ってよい行数の上限。これを超える分は削る。
const HINT_MAX_ROWS: usize = 2;
/// 削ったことを示す印。押せるキーではないので、右隣の `? ヘルプ` に任せる。
const HINT_ELLIPSIS: &str = "…";

/// ヒントを行幅に詰める。入り切らない分は捨てるが、末尾は必ず残す。
///
/// 返すのは行ごとのヒントの並び。1行では収まらないときだけ2行にする。
fn fit_hints(hints: &[&'static str], width: usize) -> Vec<Vec<&'static str>> {
    use unicode_width::UnicodeWidthStr;
    // 先頭の余白1桁ぶんを引いた、実際に書ける桁数。
    let usable = width.saturating_sub(1);
    let (body, tail) = hints.split_at(hints.len().saturating_sub(HINT_TAIL.len()));

    // 最後の行には、末尾と「削った」印のぶんを空けておく。
    let reserve: usize = HINT_ELLIPSIS.width()
        + SEPARATOR_WIDTH
        + tail
            .iter()
            .map(|h| h.width() + SEPARATOR_WIDTH)
            .sum::<usize>();

    let row_width = |row: &Vec<&str>| -> usize {
        row.iter().map(|h| h.width()).sum::<usize>() + SEPARATOR_WIDTH * row.len().saturating_sub(1)
    };
    let fits = |row: &Vec<&str>, hint: &str, limit: usize| -> bool {
        let sep = if row.is_empty() { 0 } else { SEPARATOR_WIDTH };
        row_width(row) + sep + hint.width() <= limit
    };
    // その行が最後の行なら、末尾のぶんを空けた幅を返す。
    let limit_of = |rows: usize| {
        if rows == HINT_MAX_ROWS {
            usable.saturating_sub(reserve)
        } else {
            usable
        }
    };

    let mut lines: Vec<Vec<&'static str>> = vec![Vec::new()];
    let mut dropped = false;
    for hint in body {
        let rows = lines.len();
        let current = lines.last().expect("行は必ず1つある");
        if fits(current, hint, limit_of(rows)) {
            lines.last_mut().expect("行は必ず1つある").push(hint);
            continue;
        }
        // 行を増やせるなら次の行へ送る。増やせなければここで打ち切る。
        if rows < HINT_MAX_ROWS && hint.width() <= limit_of(rows + 1) {
            lines.push(vec![hint]);
            continue;
        }
        dropped = true;
        break;
    }
    let mut trailing: Vec<&'static str> = Vec::new();
    if dropped {
        trailing.push(HINT_ELLIPSIS);
    }
    trailing.extend_from_slice(tail);

    // 末尾が今の行に入らないなら行を増やす。増やせないときはそのまま置く
    // （端末が極端に狭い場合で、切れても末尾を出すほうがまし）。
    let needed: usize = trailing
        .iter()
        .map(|h| h.width() + SEPARATOR_WIDTH)
        .sum::<usize>();
    if row_width(lines.last().expect("行は必ず1つある")) + needed > usable
        && lines.len() < HINT_MAX_ROWS
    {
        lines.push(Vec::new());
    }
    for hint in trailing {
        lines.last_mut().expect("行は必ず1つある").push(hint);
    }
    lines
}

/// ヒントの区切り ` · ` の桁数。
const SEPARATOR_WIDTH: usize = 3;

fn draw_hints(frame: &mut Frame, area: Rect, lines: &[Vec<&'static str>]) {
    let rendered: Vec<Line> = lines
        .iter()
        .map(|row| {
            let mut spans = vec![Span::raw(" ")];
            for (i, hint) in row.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled(" · ", Style::default().fg(DIM)));
                }
                // 省略の印はキーではないので、キーと同じ色にしない。
                if *hint == HINT_ELLIPSIS {
                    spans.push(Span::styled(HINT_ELLIPSIS, Style::default().fg(DIM)));
                    continue;
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
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(rendered), area);
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

/// 表示セル数で切り詰める。切ったことが分かるよう末尾に … を付ける。
///
/// 端末の右端で黙って切れると、続きがあることに気づけない。
pub fn clip(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        if out.width() + c.to_string().width() > width.saturating_sub(1) {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// ラベル幅を表示セル数で揃えた `ラベル  値` の行。
pub fn field(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(pad(label, 14), Style::default().fg(DIM)),
        Span::raw(value.to_string()),
    ])
}

/// 金額を 3 桁区切りで円表示にする。
pub fn yen(amount: i64) -> String {
    let negative = amount < 0;
    let digits = amount.abs().to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if negative {
        format!("-¥{grouped}")
    } else {
        format!("¥{grouped}")
    }
}

pub fn placeholder(text: &str) -> Paragraph<'static> {
    Paragraph::new(text.to_string())
        .style(Style::default().fg(DIM))
        .wrap(ratatui::widgets::Wrap { trim: false })
}

/// ペインが読み込み中のときの表示。枠は読み込み後と同じ色にして、
/// 読み終わった瞬間に色が飛ばないようにする。
pub fn draw_pending(frame: &mut Frame, area: Rect, title: &str) {
    draw_message(frame, area, title, "読み込み中…");
}

/// ペインに案内文だけを出す（親が未選択のときなど）。
pub fn draw_message(frame: &mut Frame, area: Rect, title: &str, message: &str) {
    frame.render_widget(
        placeholder(message).block(
            Block::bordered()
                .title(format!(" {title} "))
                .border_style(border_style(true)),
        ),
        area,
    );
}

/// ペインの中に収まる失敗表示。全幅版と同じ赤で揃える。
pub fn draw_error(frame: &mut Frame, area: Rect, title: &str, err: &str) {
    frame.render_widget(
        error_paragraph(err).block(
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

#[cfg(test)]
mod header_tests {
    use super::*;

    fn part(order: u8, text: &'static str) -> HeaderPart {
        HeaderPart::after_separator(order, vec![Span::raw(text)])
    }

    /// 落としてよい部品だけを、優先度の低い順に落とすこと。
    #[test]
    fn the_identity_and_mode_survive_a_narrow_terminal() {
        let parts = vec![
            HeaderPart::new(3, vec![Span::raw(" sakura-tui ")]),
            part(4, "コンピュート / "),
            part(2, "サーバー"),
            part(5, " (s)"),
            part(HeaderPart::KEEP, " 書込可 "),
            part(2, "ゾーン is1a"),
            part(HeaderPart::KEEP, "◆ crane74"),
        ];
        for width in [80usize, 50, 40, 30, 20] {
            let spans = fit_header(
                vec![
                    HeaderPart::new(3, vec![Span::raw(" sakura-tui ")]),
                    part(4, "コンピュート / "),
                    part(2, "サーバー"),
                    part(5, " (s)"),
                    part(HeaderPart::KEEP, " 書込可 "),
                    part(2, "ゾーン is1a"),
                    part(HeaderPart::KEEP, "◆ crane74"),
                ],
                width,
            );
            let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(text.contains("crane74"), "幅 {width} で認証情報が消えた");
            assert!(text.contains("書込可"), "幅 {width} でモードが消えた");
        }
        // 十分な幅なら何も落とさない。
        let total: usize = parts.iter().map(|p| width(&p.spans)).sum();
        let spans = fit_header(parts, total);
        assert_eq!(width(&spans), total);
    }

    /// 先頭の部品を落としたとき、区切りが行頭に残らないこと。
    #[test]
    fn dropping_the_title_does_not_leave_a_separator() {
        let spans = fit_header(
            vec![
                HeaderPart::new(3, vec![Span::raw(" sakura-tui ")]),
                part(2, "サーバー"),
                part(HeaderPart::KEEP, "◆ crane74"),
            ],
            // "サーバー │ ◆ crane74" がぎりぎり入る幅。
            22,
        );
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(!text.starts_with(" │ "), "行頭に区切りが残った: {text:?}");
        assert!(text.contains("crane74"));
    }

    /// 落とせるものが尽きても止まること（無限ループにしない）。
    #[test]
    fn it_stops_when_nothing_more_can_be_dropped() {
        let spans = fit_header(
            vec![
                part(HeaderPart::KEEP, "とても長い認証情報の名前"),
                part(HeaderPart::KEEP, " 書込可 "),
            ],
            10,
        );
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("認証情報"));
    }
}

#[cfg(test)]
mod hint_tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn width_of(row: &[&str]) -> usize {
        // 先頭の余白1桁 + ヒント + 区切り ` · `。
        1 + row.iter().map(|h| h.width()).sum::<usize>() + 3 * row.len().saturating_sub(1)
    }

    const MANY: [&str; 12] = [
        "↑↓/jk 移動",
        "s サービス",
        "r 更新",
        "z ゾーン",
        "n 作成",
        "D 削除",
        "P プラン変更",
        "b 起動",
        "x 停止",
        "w 読取専用へ",
        "? ヘルプ",
        "q 終了",
    ];

    /// どの幅でも行が溢れないこと。溢れると端が黙って切れる。
    #[test]
    fn rows_never_exceed_the_terminal_width() {
        for width in [40usize, 60, 80, 100, 140, 200] {
            let rows = fit_hints(&MANY, width);
            assert!(rows.len() <= HINT_MAX_ROWS, "幅 {width} で行が増えすぎた");
            for row in &rows {
                let cells = width_of(row);
                assert!(cells <= width, "幅 {width} に {cells} 桁の行が入らない");
            }
        }
    }

    /// 狭くても `? ヘルプ` と `q 終了` は残ること。
    /// これが消えると、隠れたキーの調べ方も抜け方も分からなくなる。
    #[test]
    fn the_help_key_always_survives() {
        for width in [40usize, 60, 80, 120] {
            let rows = fit_hints(&MANY, width);
            let shown: Vec<&str> = rows.concat();
            for keep in HINT_TAIL {
                assert!(shown.contains(&keep), "幅 {width} で {keep} が消えた");
            }
        }
    }

    /// 削ったときは、削ったと分かる印を出すこと。
    #[test]
    fn dropping_hints_is_visible() {
        let narrow = fit_hints(&MANY, 40).concat();
        assert!(narrow.contains(&HINT_ELLIPSIS), "削ったのに印が無い");
        // 全部入る幅では印を出さない。
        let wide = fit_hints(&MANY, 200).concat();
        assert!(!wide.contains(&HINT_ELLIPSIS));
        assert_eq!(wide.len(), MANY.len());
    }

    /// 全部入るなら1行のままにすること。無駄に本体を狭めない。
    #[test]
    fn a_short_list_stays_on_one_line() {
        let rows = fit_hints(&["? ヘルプ", "q 終了"], 80);
        assert_eq!(rows.len(), 1);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn spacious_service_area_has_balanced_margins() {
        let area = service_content_area(Rect::new(0, 1, 120, 30));
        assert_eq!(area, Rect::new(2, 2, 116, 28));
    }

    #[test]
    fn compact_service_area_keeps_all_available_space() {
        let area = service_content_area(Rect::new(3, 4, 30, 10));
        assert_eq!(area, Rect::new(3, 4, 30, 10));
    }
}
