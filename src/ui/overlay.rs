//! 前面に重ねるダイアログ（ヘルプ・確認・入力フォーム）。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::{DIM, accent};
use crate::app::{
    AiEngineTokenForm, AlertProjectForm, AlertProjectFormMode, AlertRuleForm, AlertRuleFormMode,
    App, Availability, Category, DashboardForm, DashboardFormMode, DnsRecordForm,
    DnsRecordFormMode, DnsZoneForm, DnsZoneFormMode, LogMeasureRuleForm, LogMeasureRuleFormMode,
    LogRoutingForm, LogRoutingFormMode, LoginForm, MetricsRoutingForm, MetricsRoutingFormMode,
    NotificationRoutingForm, NotificationRoutingFormMode, NotificationTargetForm,
    NotificationTargetFormMode, Overlay, ProfileForm, ProfileStorage, RegistryForm,
    RegistryFormMode, SecretForm, SecretFormMode, SimpleMonitorForm, SimpleMonitorFormMode,
    StatusKind, StorageAccessKeyForm, StorageAccessKeyFormMode, StorageForm, StorageFormMode,
    StorageRetentionForm, SwitchForm, SwitchFormMode, UserForm, UserFormMode, VaultForm,
    VaultFormMode,
};
use crate::app::{Loadable, Service};
use crate::config::CredentialSource;
use crate::iaas::Zone;
use crate::sacloud::Permission;

pub fn draw(frame: &mut Frame, app: &App) {
    let Some(overlay) = &app.overlay else {
        return;
    };
    match overlay {
        Overlay::Help => draw_help(frame),
        Overlay::Message {
            title,
            body,
            kind,
            scroll,
        } => draw_message(frame, title, body, *kind, *scroll),
        Overlay::Confirm {
            title,
            body,
            verify,
            typed,
            ..
        } => draw_confirm(frame, title, body, verify.as_deref(), typed),
        Overlay::UserForm(form) => draw_user_form(frame, form),
        Overlay::RegistryForm(form) => draw_registry_form(frame, form),
        Overlay::SwitchForm(form) => draw_switch_form(frame, form),
        Overlay::DnsRecordForm(form) => draw_dns_record_form(frame, form),
        Overlay::DnsZoneForm(form) => draw_dns_zone_form(frame, form),
        Overlay::SimpleMonitorForm(form) => draw_simple_monitor_form(frame, form),
        Overlay::VaultForm(form) => draw_vault_form(frame, form),
        Overlay::SecretForm(form) => draw_secret_form(frame, form),
        Overlay::AlertProjectForm(form) => draw_alert_project_form(frame, form),
        Overlay::AlertRuleForm(form) => draw_alert_rule_form(frame, form),
        Overlay::LogMeasureRuleForm(form) => draw_log_measure_rule_form(frame, form),
        Overlay::LogRoutingForm(form) => draw_log_routing_form(frame, form),
        Overlay::MetricsRoutingForm(form) => draw_metrics_routing_form(frame, form),
        Overlay::DashboardForm(form) => draw_dashboard_form(frame, form),
        Overlay::NotificationTargetForm(form) => draw_notification_target_form(frame, form),
        Overlay::NotificationRoutingForm(form) => draw_notification_routing_form(frame, form),
        Overlay::StorageForm(form) => draw_storage_form(frame, form),
        Overlay::StorageRetentionForm(form) => draw_storage_retention_form(frame, form),
        Overlay::StorageAccessKeyForm(form) => draw_storage_access_key_form(frame, form),
        Overlay::Login(form) => draw_login_form(frame, form),
        Overlay::ProfilePicker { sources, index } => {
            draw_profile_picker(frame, app, sources, *index)
        }
        Overlay::ZonePicker { zones, index } => draw_zone_picker(frame, app, zones, *index),
        Overlay::ServicePicker { index, initial } => {
            draw_service_picker(frame, app, *index, *initial)
        }
        Overlay::ProfileForm(form) => draw_profile_form(frame, app, form),
        Overlay::AiEngineTokenForm(form) => draw_ai_engine_token_form(frame, form),
    }
}

/// 選択肢を横に並べた行を作る。
fn choice_line<T: Copy + PartialEq>(
    label: &str,
    options: &[T],
    selected: T,
    focused: bool,
    title: impl Fn(T) -> String,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        super::pad(label, 14),
        if focused {
            Style::default().fg(accent()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        },
    )];
    for (i, option) in options.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            format!(" {} ", title(*option)),
            if *option == selected {
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(DIM)
            },
        ));
    }
    Line::from(spans)
}

fn draw_profile_form(frame: &mut Frame, app: &App, form: &ProfileForm) {
    let mut lines = Vec::new();
    if !app.has_credentials {
        lines.push(Line::from(Span::styled(
            "利用を始めるため、さくらのクラウドAPI認証情報を設定します。",
            Style::default().fg(DIM),
        )));
        lines.push(Line::raw(""));
    }
    lines.extend((0..ProfileForm::ZONE_FIELD).map(|i| {
        input_line(
            ProfileForm::label(i),
            form.value(i),
            form.field == i,
            ProfileForm::is_secret(i),
        )
    }));

    // ゾーンは選択式。接続先を変えると選択肢も入れ替わる。
    let zone_names: Vec<&str> = form.zones.iter().map(|z| z.name.as_str()).collect();
    lines.push(choice_line(
        ProfileForm::label(ProfileForm::ZONE_FIELD),
        &zone_names,
        form.zone().name.as_str(),
        form.field == ProfileForm::ZONE_FIELD,
        |name| name.to_string(),
    ));
    lines.push(Line::from(Span::styled(
        format!("{}{}", " ".repeat(14), form.zone().label()),
        Style::default().fg(DIM),
    )));

    // 接続先（本番 / 社内テスト）。URL も添えて取り違えを防ぐ。
    let root_labels: Vec<&str> = form.api_roots.iter().map(|r| r.label).collect();
    lines.push(choice_line(
        ProfileForm::label(ProfileForm::ROOT_FIELD),
        &root_labels,
        form.api_root().label,
        form.field == ProfileForm::ROOT_FIELD,
        |label| label.to_string(),
    ));
    lines.push(Line::from(Span::styled(
        format!("{}{}", " ".repeat(14), form.api_root().url),
        Style::default().fg(Color::Cyan),
    )));

    // 保存先も同じ見た目で選ばせる。
    lines.push(choice_line(
        ProfileForm::label(ProfileForm::STORAGE_FIELD),
        &ProfileStorage::ALL,
        form.storage,
        form.field == ProfileForm::STORAGE_FIELD,
        |storage| storage.title().to_string(),
    ));
    lines.push(Line::from(Span::styled(
        format!("{}{}", " ".repeat(14), form.storage.description()),
        Style::default().fg(DIM),
    )));

    lines.push(Line::raw(""));
    if form.verifying {
        lines.push(Line::from(Span::styled(
            "トークンを検証しています…",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "保存前に API を 1 回呼んでトークンが通ることを確かめます。",
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(vec![
            Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 項目移動   "),
            Span::styled("← →", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 選択   "),
            Span::styled(
                "Enter",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" 作成   "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(if app.has_credentials {
                " 戻る"
            } else {
                " 終了"
            }),
        ]));
    }

    let area = centered(frame, 74, dialog_height(&lines, 74));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(
                if app.has_credentials {
                    "資格情報の新規作成"
                } else {
                    "初期設定 — API認証情報"
                },
                accent(),
            )),
        area,
    );
}

fn draw_ai_engine_token_form(frame: &mut Frame, form: &AiEngineTokenForm) {
    let mut lines = Vec::new();
    if form.adding {
        lines.push(Line::from(Span::styled(
            "トークン名と、コントロールパネルで発行した値を入力します。",
            Style::default().fg(DIM),
        )));
        lines.push(Line::raw(""));
        lines.push(input_line("名前", &form.name, form.field == 0, false));
        lines.push(input_line("トークン", &form.token, form.field == 1, true));
        lines.push(Line::from(Span::styled(
            format!(
                "{}UUID:シークレット 形式。OSのキーチェーンへ保存します。",
                " ".repeat(14)
            ),
            Style::default().fg(DIM),
        )));
        lines.push(Line::raw(""));
        if form.verifying {
            lines.push(Line::from(Span::styled(
                "モデル一覧を取得してトークンを検証しています…",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" 項目移動   "),
                Span::styled(
                    "Enter",
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 検証して保存   "),
                Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" 一覧へ"),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "使用するトークンを選択します。値は画面や設定ファイルに表示しません。",
            Style::default().fg(DIM),
        )));
        lines.push(Line::raw(""));
        if form.entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "保存済みトークンはありません。n キーで追加できます。",
                Style::default().fg(DIM),
            )));
        } else {
            let visible = 10usize;
            let start = form
                .index
                .saturating_sub(visible / 2)
                .min(form.entries.len().saturating_sub(visible));
            let end = (start + visible).min(form.entries.len());
            if start > 0 {
                lines.push(Line::from(Span::styled(
                    "   ↑ さらにあります",
                    Style::default().fg(DIM),
                )));
            }
            for (index, entry) in form.entries[start..end].iter().enumerate() {
                let index = start + index;
                let selected = index == form.index;
                lines.push(Line::from(vec![
                    Span::styled(
                        if selected { "▌  " } else { "   " },
                        Style::default().fg(accent()),
                    ),
                    Span::styled(
                        if entry.active { "● " } else { "○ " },
                        Style::default().fg(if entry.active { Color::Green } else { DIM }),
                    ),
                    Span::styled(
                        super::pad(&entry.name, 34),
                        if selected {
                            Style::default().fg(accent()).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        },
                    ),
                    Span::styled(
                        if entry.from_env {
                            "環境変数"
                        } else {
                            "キーチェーン"
                        },
                        Style::default().fg(DIM),
                    ),
                ]));
            }
            if end < form.entries.len() {
                lines.push(Line::from(Span::styled(
                    "   ↓ さらにあります",
                    Style::default().fg(DIM),
                )));
            }
        }
        lines.push(Line::raw(""));
        let selected_is_local = form
            .entries
            .get(form.index)
            .is_some_and(|entry| !entry.from_env);
        let mut hints = vec![
            Span::styled("n", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 追加   "),
        ];
        if !form.entries.is_empty() {
            hints.extend([
                Span::styled(
                    "Enter",
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 使用   "),
                Span::styled("y", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" コピー   "),
            ]);
        }
        if selected_is_local {
            hints.extend([
                Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" 更新   "),
                Span::styled("d", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" 削除   "),
            ]);
        }
        hints.extend([
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 戻る"),
        ]);
        lines.push(Line::from(hints));
    }

    let area = centered(frame, 76, dialog_height(&lines, 76));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("AI Engineアカウントトークン", accent())),
        area,
    );
}

/// サービス一覧を分類ごとの見出し付きで組む。
///
/// `spacious` が偽なら分類の間の空行を省く。分類の数だけ行が増えるので、
/// 低い端末では空行を落とさないと下のキーヒントが枠外に出てしまう。
fn service_picker_lines(
    app: &App,
    index: usize,
    current: Option<Service>,
    spacious: bool,
    content_width: usize,
) -> (Vec<Line<'static>>, usize) {
    let mut lines = Vec::new();
    let mut selected_line = 0;
    let selected_category = Service::ALL[index].category();
    let category_services: Vec<(usize, &Service)> = Service::ALL
        .iter()
        .enumerate()
        .filter(|(_, service)| service.category() == selected_category)
        .collect();
    let category_len = category_services.len();
    for (row, (i, service)) in category_services.into_iter().enumerate() {
        let selected = i == index;
        if selected {
            selected_line = lines.len();
        }
        let is_current = current == Some(*service);
        let availability = app.service_availability(*service);
        let unusable = matches!(availability, Availability::Unusable(_));
        // 使えないサービスは印を変えて、名前も沈める。
        let marker = if is_current {
            "●"
        } else if unusable {
            "✕"
        } else {
            "○"
        };
        let name_style = if selected {
            Style::default().fg(accent()).add_modifier(Modifier::BOLD)
        } else if unusable {
            Style::default().fg(DIM)
        } else {
            Style::default()
        };
        let mut spans = vec![
            Span::styled(
                if selected { "▌  " } else { "   " },
                Style::default().fg(accent()),
            ),
            Span::styled(format!("{marker} {}", service.title()), name_style),
        ];
        // 件数列は右端を基準に揃える。現在表示を固定幅の別列にすることで、
        // サービス名や桁数が変わっても数字の位置が動かない。
        let (count_text, count_style) = if let Availability::Unusable(reason) = availability {
            (
                format!("利用不可: {reason}"),
                Style::default().fg(Color::Red),
            )
        } else if let Some(label) = service.count_label() {
            service_count_text(app, *service, label)
        } else {
            (String::new(), Style::default())
        };
        let current_width = if current.is_some() { 8 } else { 0 };
        let used = super::width(&spans);
        let count_width = unicode_width::UnicodeWidthStr::width(count_text.as_str());
        let padding = aligned_padding(content_width, used, count_width, current_width);
        spans.push(Span::raw(" ".repeat(padding)));
        spans.push(Span::styled(count_text, count_style));
        if current.is_some() {
            spans.push(Span::styled(
                if is_current { "  現在" } else { "      " },
                if is_current {
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ));
        }
        lines.push(Line::from(spans));
        if spacious && row + 1 < category_len {
            lines.push(Line::raw(""));
        }
    }
    (lines, selected_line)
}

fn aligned_padding(
    content_width: usize,
    used_width: usize,
    value_width: usize,
    trailing_width: usize,
) -> usize {
    content_width
        .saturating_sub(trailing_width)
        .saturating_sub(used_width)
        .saturating_sub(value_width)
        .max(2)
}

/// カテゴリ名を左、件数を枠の右端に揃えた行を作る。
///
/// 日本語は文字数と端末上の表示幅が異なるため、固定文字数ではなく表示セル数から
/// 余白を計算する。
fn category_picker_line(category: Category, selected: bool, content_width: usize) -> Line<'static> {
    let marker = if selected { "▌  " } else { "   " };
    let count = category.services().count().to_string();
    let name = category.title();
    let used_width =
        unicode_width::UnicodeWidthStr::width(marker) + unicode_width::UnicodeWidthStr::width(name);
    let count_width = unicode_width::UnicodeWidthStr::width(count.as_str());
    let padding = aligned_padding(content_width, used_width, count_width, 0);

    Line::from(vec![
        Span::styled(marker, Style::default().fg(accent())),
        Span::styled(
            name.to_string(),
            if selected {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ),
        Span::raw(" ".repeat(padding)),
        Span::styled(
            count,
            Style::default().fg(if selected { Color::Cyan } else { DIM }),
        ),
    ])
}

/// サービスごとのリソース数。0 件は薄く、取得中は「…」で出す。
fn service_count_text(app: &App, service: Service, label: &str) -> (String, Style) {
    // ゾーン依存のサービスは、どのゾーンの数なのかを添える。
    let suffix = if service.is_zoned() {
        format!(" @{}", app.zone)
    } else {
        String::new()
    };
    match app.service_counts.get(&service) {
        Some(Loadable::Ready(0)) => (format!("0 {label}{suffix}"), Style::default().fg(DIM)),
        Some(Loadable::Ready(count)) => (
            format!("{count} {label}{suffix}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        // 失敗は呼び出し側が「利用できません」として出す。
        Some(Loadable::Failed(_)) => (String::new(), Style::default()),
        _ => (format!("… {label}{suffix}"), Style::default().fg(DIM)),
    }
}

fn draw_service_picker(frame: &mut Frame, app: &App, index: usize, initial: bool) {
    let current = (!initial).then_some(app.service);
    let selected_category = Service::ALL[index].category();
    let area = frame.area();
    frame.render_widget(Clear, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " sakura-tui ",
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ),
            Span::styled(
                if initial {
                    "  サービスを選択"
                } else {
                    "  サービスを切り替え"
                },
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  │  {}", app.credential_source.label()),
                Style::default().fg(DIM),
            ),
        ])),
        rows[0],
    );
    let padded = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(rows[1]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("←→/hl", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(" カテゴリ   ", Style::default().fg(DIM)),
            Span::styled("↑↓/jk", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                " サービス   件数は右端に表示（@ は現在のゾーン）",
                Style::default().fg(DIM),
            ),
        ])),
        padded[1],
    );
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(rows[2]);
    let wide = body[1].width >= 72;
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if wide {
            [Constraint::Length(34), Constraint::Min(38)]
        } else {
            [Constraint::Length(0), Constraint::Min(1)]
        })
        .split(body[1]);
    // 左右 1 セルと上 1 行を枠内の余白として確保する。
    let category_inner_width = columns[0].width.saturating_sub(4) as usize;
    let category_spacious =
        columns[0].height.saturating_sub(3) >= (Category::ALL.len() * 2 - 1) as u16;
    let mut category_lines: Vec<Line> = Vec::new();
    for (row, category) in Category::ALL.iter().enumerate() {
        let selected = *category == selected_category;
        category_lines.push(category_picker_line(
            *category,
            selected,
            category_inner_width,
        ));
        if category_spacious && row + 1 < Category::ALL.len() {
            category_lines.push(Line::raw(""));
        }
    }
    if wide {
        frame.render_widget(
            Paragraph::new(category_lines).block(
                Block::bordered()
                    .title(" カテゴリ ")
                    .border_style(Style::default().fg(accent()))
                    .padding(ratatui::widgets::Padding::new(1, 1, 1, 0)),
            ),
            columns[0],
        );
    }
    let service_inner_width = columns[1].width.saturating_sub(4) as usize;
    let service_count = selected_category.services().count();
    let service_spacious = columns[1].height.saturating_sub(3) >= (service_count * 2 - 1) as u16;
    let (services, _) =
        service_picker_lines(app, index, current, service_spacious, service_inner_width);
    frame.render_widget(
        Paragraph::new(services).block(
            Block::bordered()
                .title(format!(" {} ", selected_category.title()))
                .border_style(Style::default().fg(accent()))
                .padding(ratatui::widgets::Padding::new(1, 1, 1, 0)),
        ),
        columns[1],
    );
    frame.render_widget(
        service_picker_hint(if initial { "開く" } else { "切り替え" }),
        rows[3],
    );
}

fn service_picker_hint(action: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("↑↓/jk", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 移動  "),
        Span::styled("PgUp/PgDn", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" ページ  "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {action}  ")),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ])
}

fn draw_zone_picker(frame: &mut Frame, app: &App, zones: &[Zone], index: usize) {
    let current = &app.zone;
    let countable = app.service.countable_label();
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{} を表示するゾーンを選んでください", app.service.title()),
            Style::default().fg(DIM),
        )),
        Line::raw(""),
    ];

    for (i, zone) in zones.iter().enumerate() {
        let selected = i == index;
        let is_current = zone.name == *current;
        let mut spans = vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(accent()),
            ),
            Span::styled(
                if is_current { "● " } else { "○ " },
                Style::default().fg(if is_current { accent() } else { DIM }),
            ),
            Span::styled(
                super::pad(&zone.label(), 22),
                if selected {
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
        ];
        // ゾーンに入らなくても、どこにリソースがあるか分かるようにする。
        if let Some(label) = countable {
            spans.push(zone_count_span(app, &zone.name, label));
        }
        if is_current {
            spans.push(Span::styled("  (現在)", Style::default().fg(DIM)));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::raw(""));
    lines.push(picker_hint("切り替え"));

    let area = centered(frame, 60, dialog_height(&lines, 60));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("ゾーンの切り替え", accent())),
        area,
    );
}

/// ゾーンごとの件数。0 件は薄く、取得中は「…」で出す。
fn zone_count_span(app: &App, zone: &str, label: &str) -> Span<'static> {
    match app.zone_counts.get(&(app.service, zone.to_string())) {
        Some(Loadable::Ready(0)) => Span::styled(format!("{label} なし"), Style::default().fg(DIM)),
        Some(Loadable::Ready(count)) => Span::styled(
            format!("{label} {count}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Some(Loadable::Failed(_)) => Span::styled("取得できず", Style::default().fg(Color::Red)),
        _ => Span::styled("…", Style::default().fg(DIM)),
    }
}

fn picker_hint(action: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("↑↓/jk", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {action}   ")),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ])
}

fn draw_profile_picker(
    frame: &mut Frame,
    app: &App,
    sources: &[(CredentialSource, Option<String>)],
    index: usize,
) {
    let current = &app.credential_source;
    let mut lines = Vec::new();
    for (i, (source, zone)) in sources.iter().enumerate() {
        let selected = i == index;
        let is_current = source == current;
        // 色は利用者が割り当てたもの。未設定なら既定色。
        let assigned = app
            .config
            .profile_color(source)
            .and_then(super::parse_color);

        let mut name_style = match assigned {
            Some(color) => Style::default().fg(color),
            None => Style::default(),
        };
        if selected {
            name_style = name_style.add_modifier(Modifier::BOLD);
            if assigned.is_none() {
                name_style = name_style.fg(accent());
            }
        }

        let mut spans = vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(accent()),
            ),
            Span::styled(
                if is_current { "● " } else { "○ " },
                Style::default().fg(if is_current { accent() } else { DIM }),
            ),
            // 名前は先頭が同じことが多いので、幅を揃えて末尾の違いを見やすくする。
            Span::styled(super::pad(&source.label(), 26), name_style),
        ];
        // ゾーンは取り違えに気づく手がかりになるので併記する。
        match zone {
            Some(zone) => spans.push(Span::styled(
                format!("ゾーン {zone}"),
                Style::default().fg(Color::Cyan),
            )),
            None => spans.push(Span::styled("ゾーン未設定", Style::default().fg(DIM))),
        }
        // 保存形式（usacloud 互換かキーチェーンか）を明示する。
        spans.push(Span::styled(
            format!("  {:10}", source.kind_label()),
            Style::default().fg(DIM),
        ));
        // 割り当てた色を色名でも示す（色覚や端末設定に依存しないように）。
        spans.push(Span::styled(
            app.config
                .profile_color(source)
                .unwrap_or("既定")
                .to_string(),
            Style::default().fg(DIM),
        ));
        if is_current {
            spans.push(Span::styled("  (現在)", Style::default().fg(DIM)));
        }
        lines.push(Line::from(spans));
    }
    // 一覧の最後に置くことで「作れる」ことに気づけるようにする。
    let on_new_row = index == sources.len();
    lines.push(Line::from(vec![
        Span::styled(
            if on_new_row { "▌ " } else { "  " },
            Style::default().fg(accent()),
        ),
        Span::styled(
            "＋ 新規作成…",
            if on_new_row {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
        Span::styled("  (n)", Style::default().fg(DIM)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(
            "c",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" 色を割り当て   ", Style::default().fg(DIM)),
        Span::styled(
            "d",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" 削除（キーチェーンのみ）", Style::default().fg(DIM)),
    ]));
    lines.push(Line::from(Span::styled(
        "切り替えはこのセッションのみで、~/.usacloud/current は書き換えません。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(picker_hint("切り替え"));

    let area = centered(frame, 72, dialog_height(&lines, 72));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("認証情報の切り替え", accent())),
        area,
    );
}

/// 画面中央に指定サイズの領域を取る。
fn centered(frame: &Frame, width: u16, height: u16) -> Rect {
    let area = frame.area();
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .split(area);
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .split(horizontal[0])[0]
}

/// ダイアログの内側の幅（枠 2 + 左右パディング 4 を引いた分）。
const DIALOG_PADDING: u16 = 6;

/// 折り返しを考慮した必要行数から、ダイアログ全体の高さを求める。
/// （`lines.len()` だけだと長い説明文がはみ出して下のキーヒントが隠れる）
fn dialog_height(lines: &[Line], width: u16) -> u16 {
    use unicode_width::UnicodeWidthStr;
    let inner = width.saturating_sub(DIALOG_PADDING).max(1) as usize;
    let rows: usize = lines
        .iter()
        .map(|line| {
            let cells: usize = line.spans.iter().map(|s| s.content.width()).sum();
            cells.div_ceil(inner).max(1)
        })
        .sum();
    // 枠 2 行 + 上下パディング 2 行。
    rows as u16 + 4
}

fn dialog(title: &str, color: Color) -> Block<'static> {
    Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(color))
        .padding(ratatui::widgets::Padding::new(2, 2, 1, 1))
}

fn draw_help(frame: &mut Frame) {
    let sections: [(&str, &[(&str, &str)]); 5] = [
        (
            "移動",
            &[
                ("↑ ↓ / k j", "リスト内を移動"),
                ("g / G", "先頭 / 末尾へ"),
                ("PgUp / PgDn", "10件ずつ移動"),
                ("← → / h l", "ペインの移動"),
                ("Enter", "右のペインへ入る"),
            ],
        ),
        (
            "タブ",
            &[
                ("Tab / Shift+Tab", "タブを切り替え"),
                ("1 / 2 / 3", "概要 / ユーザー / イメージ"),
                ("1〜4", "専有型: 概要 / アプリ / ASG / 証明書"),
                (
                    "1〜9",
                    "監視: ルール / 履歴 / 保管先 / 通知先 / 通知経路 / ログ計測 / ログ転送 / メトリクス転送 / ダッシュボード",
                ),
            ],
        ),
        (
            "モード",
            &[
                ("w", "読み取り専用 / 書き込みモードの切り替え"),
                ("", "起動時は読み取り専用。書き込み系のキーは効きません"),
            ],
        ),
        (
            "操作",
            &[
                ("r", "表示中のデータを再取得"),
                ("R", "全キャッシュを破棄して再取得"),
                ("a", "ユーザー / DNSレコードを追加"),
                ("e", "ユーザー / DNSレコードを編集"),
                ("d", "選択中の項目を削除"),
                ("n", "レジストリ / スイッチ / DNS / 監視を作成"),
                ("E", "レジストリ / スイッチ / DNS / 監視を編集"),
                ("D", "レジストリ / スイッチ / DNS / 監視を削除"),
                ("L", "レジストリにログイン"),
                ("O", "レジストリのログイン情報を破棄"),
                ("/", "表示中のリストを絞り込み"),
                ("y", "選択中の項目をコピー"),
                ("p", "認証情報（プロファイル）を切替"),
                ("s / S", "サービスを切り替え（分類ごとに一覧）"),
                ("", "  サービス選択内: PgUp/PgDn 5件移動 / g/G 先頭・末尾"),
                ("", "  ピッカー内: n 新規作成 / c 色 / d 削除"),
                ("z", "ゾーンを切り替え（サーバー / スイッチほか）"),
                ("t", "トラフィック切替 / シンプル監視の有効・停止"),
                ("t", "監視のログ／トレース保持期間を変更"),
                ("t", "AI Engineアカウントトークンを管理"),
                (
                    "a / e / d",
                    "監視ストレージのアクセスキーを作成 / 編集 / 削除",
                ),
                ("u", "シークレット / アクセスキーの秘密情報を確認表示"),
                (
                    "Enter / Esc",
                    "左の一覧と右の一覧を行き来（DNS・シークレット・監視）",
                ),
                ("← →", "請求: 表示する年を移動"),
                ("↑ ↓", "請求: 月を選ぶ（明細に入ると明細の移動）"),
                ("Enter / Esc", "請求: 明細に入る / 月一覧に戻る"),
                ("b / x", "サーバーの起動 / シャットダウン"),
                ("X / B", "サーバーの強制停止 / 強制リセット"),
            ],
        ),
        ("その他", &[("?", "このヘルプ"), ("q / Ctrl+C", "終了")]),
    ];

    let mut lines = Vec::new();
    for (section, entries) in sections {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::from(Span::styled(
            section,
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        )));
        for (key, description) in entries {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {key:<16}"),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(*description, Style::default().fg(Color::Gray)),
            ]));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "何かキーを押すと閉じます",
        Style::default().fg(DIM),
    )));

    let area = centered(frame, 60, dialog_height(&lines, 60));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(dialog("キーバインド", accent())),
        area,
    );
}

fn draw_message(frame: &mut Frame, title: &str, body: &str, kind: StatusKind, scroll: u16) {
    let color = match kind {
        StatusKind::Error => Color::Red,
        StatusKind::Success => Color::Green,
        StatusKind::Info => accent(),
    };

    // 本文は改行を保って出す。長いものは画面に収まる範囲で高さを取り、残りはスクロールで読む。
    let body_lines: Vec<Line> = body.lines().map(|l| Line::raw(l.to_string())).collect();
    let width: u16 = 76;
    let screen = frame.area();
    let needed = dialog_height(&body_lines, width) + 2;
    // 画面の 8 割までは広げる。
    let height = needed.min(screen.height.saturating_mul(4) / 5).max(7);
    let inner = height.saturating_sub(6);
    let scrollable = needed.saturating_sub(2) > inner;
    let max_scroll = dialog_height(&body_lines, width)
        .saturating_sub(2)
        .saturating_sub(inner);
    let scroll = scroll.min(max_scroll);

    let mut lines = body_lines;
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        if scrollable {
            "↑↓/PgUp/PgDn でスクロール   Esc / Enter / q で閉じる"
        } else {
            "Esc / Enter / q で閉じる"
        },
        Style::default().fg(DIM),
    )));

    let area = centered(frame, width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(dialog(title, color)),
        area,
    );
}

fn draw_confirm(frame: &mut Frame, title: &str, body: &str, verify: Option<&str>, typed: &str) {
    let mut lines: Vec<Line> = body.lines().map(|l| Line::raw(l.to_string())).collect();
    lines.push(Line::raw(""));
    match verify {
        Some(expected) => {
            lines.push(input_line("確認入力", typed, true, false));
            let ready = typed == expected;
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "Enter",
                    if ready {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(DIM)
                    },
                ),
                Span::raw(if ready {
                    " 実行    "
                } else {
                    " (名前が一致すると実行できます)    "
                }),
                Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" キャンセル"),
            ]));
        }
        None => lines.push(Line::from(vec![
            Span::styled(
                "y / Enter",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" 実行    "),
            Span::styled("n / Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" キャンセル"),
        ])),
    }
    let area = centered(frame, 70, dialog_height(&lines, 70));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, Color::Red)),
        area,
    );
}

fn draw_user_form(frame: &mut Frame, form: &UserForm) {
    let title = match form.mode {
        UserFormMode::Add => format!("ユーザーの追加 — {}", form.registry_name),
        UserFormMode::Edit => format!("ユーザーの編集 — {}", form.registry_name),
    };
    let editable_username = form.mode == UserFormMode::Add;
    let password_hint = match form.mode {
        UserFormMode::Add => "",
        UserFormMode::Edit => "  (空欄なら変更しない)",
    };

    let mut lines = vec![
        input_line("ユーザー名", &form.username, form.field == 0, false),
        input_line("パスワード", &form.password, form.field == 1, true),
    ];
    if !password_hint.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("{}{password_hint}", " ".repeat(14)),
            Style::default().fg(DIM),
        )));
    }
    lines.push(permission_line(form.permission, form.field == 2));
    lines.push(Line::raw(""));
    if !editable_username {
        lines.push(Line::from(Span::styled(
            "ユーザー名は変更できません",
            Style::default().fg(DIM),
        )));
    }
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled("← →", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 権限変更   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 66, dialog_height(&lines, 66));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_registry_form(frame: &mut Frame, form: &RegistryForm) {
    let title = match form.mode {
        RegistryFormMode::Create => "コンテナレジストリの作成".to_string(),
        RegistryFormMode::Edit => format!("レジストリの編集 — {}", form.name),
    };

    let mut lines: Vec<Line> = form
        .labels()
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    match form.mode {
        RegistryFormMode::Create => {
            let host = if form.subdomain.is_empty() {
                "<サブドメイン>.sakuracr.jp".to_string()
            } else {
                format!("{}.sakuracr.jp", form.subdomain)
            };
            lines.push(Line::from(vec![
                Span::styled("ホスト名: ", Style::default().fg(DIM)),
                Span::styled(host, Style::default().fg(Color::Cyan)),
            ]));
            lines.push(Line::from(Span::styled(
                "サブドメインは作成後に変更できません。公開設定は「非公開」で作成します。",
                Style::default().fg(DIM),
            )));
        }
        RegistryFormMode::Edit => {
            lines.push(Line::from(Span::styled(
                "サブドメインは変更できません。独自ドメインは空欄で解除されます。",
                Style::default().fg(DIM),
            )));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 70, dialog_height(&lines, 70));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_switch_form(frame: &mut Frame, form: &SwitchForm) {
    let title = match form.mode {
        SwitchFormMode::Create => "スイッチの作成".to_string(),
        SwitchFormMode::Edit => format!("スイッチの編集 — {}", form.name),
    };
    let mut lines: Vec<Line> = SwitchForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "名前は64文字、説明は512文字まで入力できます。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 70, dialog_height(&lines, 70));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_dns_record_form(frame: &mut Frame, form: &DnsRecordForm) {
    let title = match form.mode {
        DnsRecordFormMode::Add => format!("DNSレコードの追加 — {}", form.zone.name),
        DnsRecordFormMode::Edit => format!("DNSレコードの編集 — {}", form.zone.name),
    };
    let mut lines: Vec<Line> = DnsRecordForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "ゾーン頂点は名前を @ にします。種別は A / AAAA / CNAME / MX / TXT などです。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "NS・MX・CNAME等のFQDNは末尾にドットが必要です。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_dns_zone_form(frame: &mut Frame, form: &DnsZoneForm) {
    let title = match form.mode {
        DnsZoneFormMode::Create => "DNSゾーンの作成".to_string(),
        DnsZoneFormMode::Edit => format!("DNSゾーンの編集 — {}", form.name),
    };
    let mut lines: Vec<Line> = DnsZoneForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        match form.mode {
            DnsZoneFormMode::Create => {
                "ゾーン名は作成後に変更できません。国際化ドメインはPunycodeで入力してください。"
            }
            DnsZoneFormMode::Edit => "ゾーン名は作成後に変更できないため、説明だけを編集できます。",
        },
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_simple_monitor_form(frame: &mut Frame, form: &SimpleMonitorForm) {
    let title = match form.mode {
        SimpleMonitorFormMode::Create => "シンプル監視の作成".to_string(),
        SimpleMonitorFormMode::Edit => format!("シンプル監視の編集 — {}", form.target),
    };
    let protocol = form.protocol();
    let mut lines = vec![
        input_line("監視対象", &form.target, form.field == 0, false),
        input_line("説明", &form.description, form.field == 1, false),
        choice_line(
            "監視方式",
            &SimpleMonitorForm::PROTOCOLS,
            protocol,
            form.field == 2,
            |value| value.to_string(),
        ),
        input_line("ポート", &form.port, form.field == 3, false),
        input_line("パス", &form.path, form.field == 4, false),
        input_line(
            "期待ステータス",
            &form.expected_status,
            form.field == 5,
            false,
        ),
        input_line("監視間隔(秒)", &form.delay_loop, form.field == 6, false),
        input_line("タイムアウト", &form.timeout, form.field == 7, false),
        choice_line(
            "有効/停止",
            &[true, false],
            form.enabled,
            form.field == 8,
            |enabled| if enabled { "有効" } else { "停止" }.to_string(),
        ),
        choice_line(
            "メール通知",
            &[true, false],
            form.notify_email,
            form.field == 9,
            |enabled| if enabled { "有効" } else { "無効" }.to_string(),
        ),
        Line::raw(""),
    ];
    if form.mode == SimpleMonitorFormMode::Edit {
        lines.push(Line::from(Span::styled(
            "監視対象は作成後に変更できません。Webhookなど画面にない設定は維持されます。",
            Style::default().fg(DIM),
        )));
    }
    let protocol_note = match protocol {
        "ping" => "pingではポート・パス・期待ステータスを使用しません。",
        "tcp" => "TCP監視ではポートが必須です。",
        _ => "HTTP(S)ではパスと期待ステータスを指定できます。ポートは省略可能です。",
    };
    lines.push(Line::from(Span::styled(
        protocol_note,
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "監視間隔は60秒単位です。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled("← →", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 選択   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 82, dialog_height(&lines, 82));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_vault_form(frame: &mut Frame, form: &VaultForm) {
    let title = match form.mode {
        VaultFormMode::Create => "Vaultの作成".to_string(),
        VaultFormMode::Edit => format!("Vaultの編集 — {}", form.name),
    };
    let mut lines: Vec<Line> = VaultForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        if form.mode == VaultFormMode::Create {
            "KMS鍵IDは必須です。タグはカンマ区切りで入力します。"
        } else {
            "KMS鍵は編集では変更せず、現在の鍵を維持します。"
        },
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(form_footer());
    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_secret_form(frame: &mut Frame, form: &SecretForm) {
    let title = match form.mode {
        SecretFormMode::Create => format!("シークレットの登録 — {}", form.vault.name),
        SecretFormMode::Update => format!("新バージョンの登録 — {}", form.name),
    };
    let mut lines = vec![
        input_line("名前", &form.name, form.field == 0, false),
        input_line("値", &form.value, form.field == 1, true),
        Line::raw(""),
        Line::from(Span::styled(
            if form.mode == SecretFormMode::Create {
                "値は常に伏せて表示し、65,536バイトまで登録できます。"
            } else {
                "名前は変更せず、入力した値を新しいバージョンとして登録します。"
            },
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    // フォームの値はこのダイアログを閉じると画面状態から捨てられる。
    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(std::mem::take(&mut lines))
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn form_footer() -> Line<'static> {
    Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ])
}

fn draw_alert_project_form(frame: &mut Frame, form: &AlertProjectForm) {
    let title = match form.mode {
        AlertProjectFormMode::Create => "アラートプロジェクトの作成".to_string(),
        AlertProjectFormMode::Edit => {
            format!("アラートプロジェクトの編集 — {}", form.name)
        }
    };
    let mut lines: Vec<Line> = AlertProjectForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(form_footer());
    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_alert_rule_form(frame: &mut Frame, form: &AlertRuleForm) {
    let title = match form.mode {
        AlertRuleFormMode::Create => format!("アラートルールの作成 — {}", form.project.name),
        AlertRuleFormMode::Edit => format!("アラートルールの編集 — {}", form.name),
    };
    let mut lines = vec![
        input_line(
            "メトリクスID",
            &form.metrics_storage_id,
            form.field == 0,
            false,
        ),
        input_line("名前", &form.name, form.field == 1, false),
        input_line("クエリ", &form.query, form.field == 2, false),
        choice_line(
            "警告",
            &[true, false],
            form.warning_enabled,
            form.field == 3,
            |enabled| if enabled { "有効" } else { "無効" }.to_string(),
        ),
        input_line(
            "警告しきい値",
            &form.threshold_warning,
            form.field == 4,
            false,
        ),
        input_line("警告継続秒", &form.duration_warning, form.field == 5, false),
        choice_line(
            "重大",
            &[true, false],
            form.critical_enabled,
            form.field == 6,
            |enabled| if enabled { "有効" } else { "無効" }.to_string(),
        ),
        input_line(
            "重大しきい値",
            &form.threshold_critical,
            form.field == 7,
            false,
        ),
        input_line(
            "重大継続秒",
            &form.duration_critical,
            form.field == 8,
            false,
        ),
        Line::raw(""),
        Line::from(Span::styled(
            "メトリクスストレージIDは保管先タブで確認できます。編集時もformat/templateは維持されます。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 86, dialog_height(&lines, 86));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(std::mem::take(&mut lines))
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_log_measure_rule_form(frame: &mut Frame, form: &LogMeasureRuleForm) {
    let title = match form.mode {
        LogMeasureRuleFormMode::Create => "ログ計測ルールの作成",
        LogMeasureRuleFormMode::Edit => "ログ計測ルールの編集",
    };
    let lines = vec![
        input_line(
            "ログストレージID",
            &form.log_storage_id,
            form.field == 0,
            false,
        ),
        input_line(
            "メトリクスID",
            &form.metrics_storage_id,
            form.field == 1,
            false,
        ),
        input_line("名前", &form.name, form.field == 2, false),
        input_line("説明", &form.description, form.field == 3, false),
        input_line("ルールJSON", &form.rule_json, form.field == 4, false),
        Line::raw(""),
        Line::from(Span::styled(
            "JSONは version=v1 と query.matchers 配列を保持します。複雑なマッチャーもそのまま編集できます。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 92, dialog_height(&lines, 92));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

fn draw_log_routing_form(frame: &mut Frame, form: &LogRoutingForm) {
    let title = match form.mode {
        LogRoutingFormMode::Create => "ログ転送設定の作成",
        LogRoutingFormMode::Edit => "ログ転送設定の編集",
    };
    let publisher = form.publishers.get(form.publisher_index);
    let publisher_label = publisher
        .map(|p| format!("{} — {}", p.code, p.description))
        .unwrap_or_else(|| form.publisher_code.clone());
    let variant_label = publisher
        .and_then(|p| p.variants.get(form.variant_index))
        .map(|v| format!("{} — {}", v.name, v.label))
        .unwrap_or_else(|| form.variant.clone());
    let lines = vec![
        input_line("パブリッシャー", &publisher_label, form.field == 0, false),
        input_line("バリアント", &variant_label, form.field == 1, false),
        input_line("リソースID", &form.resource_id, form.field == 2, false),
        input_line(
            "ログストレージID",
            &form.log_storage_id,
            form.field == 3,
            false,
        ),
        Line::raw(""),
        Line::from(Span::styled(
            "パブリッシャーとバリアントは対象サービスが公開する値を指定します。リソースIDは任意です。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 82, dialog_height(&lines, 82));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

fn draw_metrics_routing_form(frame: &mut Frame, form: &MetricsRoutingForm) {
    let title = match form.mode {
        MetricsRoutingFormMode::Create => "メトリクス転送設定の作成",
        MetricsRoutingFormMode::Edit => "メトリクス転送設定の編集",
    };
    let publisher = form.publishers.get(form.publisher_index);
    let publisher_label = publisher
        .map(|p| format!("{} — {}", p.code, p.description))
        .unwrap_or_else(|| form.publisher_code.clone());
    let variant_label = publisher
        .and_then(|p| p.variants.get(form.variant_index))
        .map(|v| format!("{} — {}", v.name, v.label))
        .unwrap_or_else(|| form.variant.clone());
    let lines = vec![
        input_line("パブリッシャー", &publisher_label, form.field == 0, false),
        input_line("バリアント", &variant_label, form.field == 1, false),
        input_line("リソースID", &form.resource_id, form.field == 2, false),
        input_line(
            "メトリクスストレージID",
            &form.metrics_storage_id,
            form.field == 3,
            false,
        ),
        Line::raw(""),
        Line::from(Span::styled(
            "← / → でAPIが公開する候補を選択します。リソースIDは任意です。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 86, dialog_height(&lines, 86));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

fn draw_dashboard_form(frame: &mut Frame, form: &DashboardForm) {
    let title = match form.mode {
        DashboardFormMode::Create => "ダッシュボードプロジェクトの作成",
        DashboardFormMode::Edit => "ダッシュボードプロジェクトの編集",
    };
    let lines = vec![
        input_line("名前", &form.name, form.field == 0, false),
        input_line("説明", &form.description, form.field == 1, false),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 76, dialog_height(&lines, 76));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

fn draw_notification_target_form(frame: &mut Frame, form: &NotificationTargetForm) {
    let title = match form.mode {
        NotificationTargetFormMode::Create => "通知先の作成",
        NotificationTargetFormMode::Edit => "通知先の編集",
    };
    let lines = vec![
        choice_line(
            "サービス",
            &NotificationTargetForm::SERVICE_TYPES,
            form.service_type(),
            form.field == 0,
            |service| match service {
                "SAKURA_SIMPLE_NOTICE" => "シンプル通知".to_string(),
                "SAKURA_EVENT_BUS" => "EventBus".to_string(),
                other => other.to_string(),
            },
        ),
        input_line("URL", &form.url, form.field == 1, false),
        input_line("説明", &form.description, form.field == 2, false),
        Line::raw(""),
        Line::from(Span::styled(
            "URLはAPI上省略可能です。指定する場合は http(s) URLを入力してください。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

fn draw_notification_routing_form(frame: &mut Frame, form: &NotificationRoutingForm) {
    let title = match form.mode {
        NotificationRoutingFormMode::Create => "通知経路の作成",
        NotificationRoutingFormMode::Edit => "通知経路の編集",
    };
    let selected = form
        .selected_target()
        .map(|target| {
            let label = match target.service_type.as_str() {
                "SAKURA_SIMPLE_NOTICE" => "シンプル通知",
                "SAKURA_EVENT_BUS" => "EventBus",
                other => other,
            };
            if target.description.is_empty() {
                format!("{label} ({})", target.uid)
            } else {
                format!("{label} ({})", target.description)
            }
        })
        .unwrap_or_else(|| "通知先なし".to_string());
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{:<14}", "通知先"),
                if form.field == 0 {
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DIM)
                },
            ),
            Span::raw(if form.field == 0 {
                format!("< {selected} >")
            } else {
                selected
            }),
        ]),
        input_line(
            "再送間隔（分）",
            &form.resend_interval,
            form.field == 1,
            false,
        ),
        input_line("ラベル条件", &form.match_labels, form.field == 2, false),
        Line::raw(""),
        Line::from(Span::styled(
            "条件は name=value をカンマ区切りで入力します。空欄なら全アラートが対象です。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 84, dialog_height(&lines, 84));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

fn draw_storage_form(frame: &mut Frame, form: &StorageForm) {
    let title = match form.mode {
        StorageFormMode::Create => "ストレージの作成".to_string(),
        StorageFormMode::Edit => format!("{}ストレージの編集 — {}", form.kind.label(), form.name),
    };
    let mut lines = Vec::new();
    if form.mode == StorageFormMode::Create {
        lines.push(choice_line(
            "種別",
            &StorageForm::KINDS,
            form.kind,
            form.field == 0,
            |kind| kind.label().to_string(),
        ));
        lines.push(choice_line(
            "保存領域",
            &[true, false],
            form.is_system,
            form.field == 1,
            |system| if system { "システム" } else { "ユーザ" }.to_string(),
        ));
        lines.push(choice_line(
            "プラン",
            &StorageForm::CLASSIFICATIONS,
            form.classification(),
            form.field == 2,
            |value| value.to_string(),
        ));
    } else {
        let current_classification = form
            .target
            .as_ref()
            .map(|storage| storage.classification.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("-");
        lines.push(Line::from(Span::styled(
            format!("種別          {}", form.kind.label()),
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "保存領域      {}",
                if form.is_system {
                    "システム"
                } else {
                    "ユーザ"
                }
            ),
            Style::default().fg(DIM),
        )));
        lines.push(Line::from(Span::styled(
            format!("プラン        {current_classification}"),
            Style::default().fg(DIM),
        )));
    }
    lines.push(input_line("名前", &form.name, form.field == 3, false));
    lines.push(input_line(
        "説明",
        &form.description,
        form.field == 4,
        false,
    ));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        if form.mode == StorageFormMode::Create {
            "作成後は課金対象です。ログ／メトリクスの最初の保管先はシステム領域を選んでください。\nトレースはユーザ領域固定、メトリクスはプラン指定を使いません。"
        } else {
            "種別・区分・保持期間はこのフォームでは変更しません。"
        },
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(form_footer());
    let area = centered(frame, 82, dialog_height(&lines, 82));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

fn draw_storage_retention_form(frame: &mut Frame, form: &StorageRetentionForm) {
    let lines = vec![
        Line::from(format!(
            "{}ストレージ    {}",
            form.storage.kind.label(),
            form.storage.name
        )),
        input_line("保持期間（日）", &form.days, true, false),
        Line::raw(""),
        Line::from(Span::styled(
            "ログ／トレースだけ変更できます。41日以上は追加料金が発生します。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        form_footer(),
    ];
    let area = centered(frame, 72, dialog_height(&lines, 72));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("ストレージ保持期間の変更", accent())),
        area,
    );
}

fn draw_storage_access_key_form(frame: &mut Frame, form: &StorageAccessKeyForm) {
    let title = match form.mode {
        StorageAccessKeyFormMode::Create => "アクセスキーの作成",
        StorageAccessKeyFormMode::Edit => "アクセスキーの説明編集",
    };
    let mut lines = vec![
        Line::from(format!(
            "{}ストレージ    {}",
            form.storage.kind.label(),
            form.storage.name
        )),
        input_line("説明", &form.description, true, false),
        Line::raw(""),
    ];
    lines.push(Line::from(Span::styled(
        if form.mode == StorageAccessKeyFormMode::Create {
            "作成後にトークンとシークレットを表示します。安全な場所へ保存してください。"
        } else {
            "キー本体は変更せず、説明だけを更新します。"
        },
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(form_footer());
    let area = centered(frame, 76, dialog_height(&lines, 76));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

fn draw_login_form(frame: &mut Frame, form: &LoginForm) {
    let lines = vec![
        Line::from(vec![
            Span::styled("ホスト  ", Style::default().fg(DIM)),
            Span::styled(&form.host, Style::default().fg(Color::Cyan)),
        ]),
        Line::raw(""),
        input_line("ユーザー名", &form.username, form.field == 0, false),
        input_line("パスワード", &form.password, form.field == 1, true),
        Line::from(vec![
            Span::styled(
                super::pad("設定に保存", 14),
                if form.field == 2 {
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DIM)
                },
            ),
            Span::raw(if form.save {
                "[x] する"
            } else {
                "[ ] しない"
            }),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "パスワードはOSのキーチェーンに保存します（設定ファイルには書きません）",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "レジストリユーザーの認証情報です（クラウドAPIトークンではありません）",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 項目移動   "),
            Span::styled("Space", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 保存切替   "),
            Span::styled(
                "Enter",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ログイン   "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 中止"),
        ]),
    ];

    let area = centered(frame, 72, dialog_height(&lines, 72));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("レジストリへログイン", accent())),
        area,
    );
}

/// 入力欄 1 行。フォーカス中はカーソルを表示する。
fn input_line(label: &str, value: &str, focused: bool, masked: bool) -> Line<'static> {
    let count = value.chars().count();
    // 伏せ字だと何文字入ったか数えづらい。貼り付けが途中で切れていないか
    // 確かめられるよう、長いものは点の代わりに文字数を出す。
    let shown = if masked {
        if count > 12 {
            format!("{} ({count}文字)", "•".repeat(12))
        } else {
            "•".repeat(count)
        }
    } else {
        value.to_string()
    };
    let label_style = if focused {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    let value_style = if focused {
        Style::default().add_modifier(Modifier::UNDERLINED)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(super::pad(label, 14), label_style),
        Span::styled(shown, value_style),
        Span::styled(
            if focused { "▏" } else { "" },
            Style::default().fg(accent()),
        ),
    ])
}

/// 権限の選択欄。
fn permission_line(selected: usize, focused: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        super::pad("権限", 14),
        if focused {
            Style::default().fg(accent()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        },
    )];
    for (i, permission) in Permission::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let style = if i == selected {
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(DIM)
        };
        spans.push(Span::styled(format!(" {} ", permission.as_str()), style));
    }
    Line::from(spans)
}

#[cfg(test)]
mod layout_tests {
    use super::{aligned_padding, category_picker_line};
    use crate::app::Category;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn count_values_end_at_the_same_column() {
        let content = 52;
        let trailing = 8;
        for (name_width, value_width) in [(12, 4), (24, 9), (18, 13)] {
            let padding = aligned_padding(content, name_width, value_width, trailing);
            assert_eq!(name_width + padding + value_width + trailing, content);
        }
    }

    #[test]
    fn narrow_rows_keep_a_minimum_gap() {
        assert_eq!(aligned_padding(20, 18, 8, 0), 2);
    }

    #[test]
    fn every_category_count_ends_at_the_content_edge() {
        let content_width = 30;
        for category in Category::ALL {
            for selected in [false, true] {
                let line = category_picker_line(category, selected, content_width);
                let width: usize = line
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref().width())
                    .sum();
                assert_eq!(width, content_width, "{}", category.title());
            }
        }
    }
}
