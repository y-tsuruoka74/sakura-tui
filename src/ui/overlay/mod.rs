//! 前面に重ねるダイアログ（ヘルプ・確認・入力フォーム）。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

mod forms;
mod pickers;

use forms::*;
use pickers::*;

use super::{DIM, accent};
use crate::app::{
    AiEngineTokenForm, AlertProjectForm, AlertProjectFormMode, AlertRuleForm, AlertRuleFormMode,
    App, ArchiveForm, Availability, Category, DashboardForm, DashboardFormMode, DiskCreateForm,
    DiskField, DiskServerPicker, DiskSourceKind, DnsRecordForm, DnsRecordFormMode, DnsZoneForm,
    DnsZoneFormMode, IamCredentialForm, IamResourceForm, IamResourceFormMode, IamRoleForm,
    LogMeasureRuleForm, LogMeasureRuleFormMode, LogRoutingForm, LogRoutingFormMode, LoginForm,
    MetricsRoutingForm, MetricsRoutingFormMode, NicChoice, NicPicker, NotificationRoutingForm,
    NotificationRoutingFormMode, NotificationTargetForm, NotificationTargetFormMode, Overlay,
    PacketFilterForm, PacketFilterFormMode, ProfileForm, ProfileStorage, RagEditForm,
    RagUploadForm, RegistryForm, RegistryFormMode, RuleField, RuleForm, RuleFormMode, SecretForm,
    SecretFormMode, ServerChoicePicker, ServerChoices, ServerCreateForm, ServerField,
    ServerPlanForm, SimpleMonitorForm, SimpleMonitorFormMode, SshKeyForm, SshKeyFormMode,
    SshKeyReturn, SshKeyStage, StatusKind, StorageAccessKeyForm, StorageAccessKeyFormMode,
    StorageForm, StorageFormMode, StorageRetentionForm, SwitchForm, SwitchFormMode, UserForm,
    UserFormMode, VaultForm, VaultFormMode,
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
        Overlay::IamResourceForm(form) => draw_iam_resource_form(frame, form),
        Overlay::IamRoleForm(form) => draw_iam_role_form(frame, form),
        Overlay::SwitchForm(form) => draw_switch_form(frame, form),
        Overlay::RagUploadForm(form) => draw_rag_upload_form(frame, form),
        Overlay::ServerCreateForm(form) => draw_server_create_form(frame, form, app),
        Overlay::ServerChoicePicker(picker) => draw_server_choice_picker(frame, picker, app),
        Overlay::NicPicker(picker) => draw_nic_picker(frame, picker, app),
        Overlay::PacketFilterForm(form) => draw_packet_filter_form(frame, form),
        Overlay::RuleForm(form) => draw_rule_form(frame, form),
        Overlay::ServerPlanForm(form) => draw_server_plan_form(frame, form, app),
        Overlay::SshKeyForm(form) => draw_ssh_key_form(frame, form),
        Overlay::DiskCreateForm(form) => draw_disk_create_form(frame, form, app),
        Overlay::ArchiveForm(form) => draw_archive_form(frame, form, app),
        Overlay::DiskServerPicker(picker) => draw_disk_server_picker(frame, picker),
        Overlay::SshKeyPicker { back, stage } => draw_ssh_key_picker(frame, back, stage),
        Overlay::RagEditForm(form) => draw_rag_edit_form(frame, form),
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
        Overlay::LoginPicker {
            host,
            accounts,
            index,
        } => draw_login_picker(frame, host, accounts, *index),
        Overlay::ProfilePicker { sources, index } => {
            draw_profile_picker(frame, app, sources, *index)
        }
        Overlay::ZonePicker { zones, index } => draw_zone_picker(frame, app, zones, *index),
        Overlay::ServicePicker { index, initial } => {
            draw_service_picker(frame, app, *index, *initial)
        }
        Overlay::ProfileForm(form) => draw_profile_form(frame, app, form),
        Overlay::AiEngineTokenForm(form) => draw_ai_engine_token_form(frame, form),
        Overlay::IamCredentialForm(form) => draw_iam_credential_form(frame, form),
    }
}

/// 選択肢を横に並べた行を作る。
pub(super) fn choice_line<T: Copy + PartialEq>(
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

pub(super) fn draw_profile_form(frame: &mut Frame, app: &App, form: &ProfileForm) {
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

pub(super) fn draw_ai_engine_token_form(frame: &mut Frame, form: &AiEngineTokenForm) {
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

pub(super) fn draw_iam_credential_form(frame: &mut Frame, form: &IamCredentialForm) {
    let key_status = if form.private_key.is_empty() {
        "未入力".to_string()
    } else {
        format!("貼り付け済み（{}文字）", form.private_key.chars().count())
    };
    let mut lines = vec![
        Line::from(Span::styled(
            "IAM API専用のサービスプリンシパルを設定します。",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "秘密鍵はOSのキーチェーンに保存され、設定ファイルには書きません。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        input_line(
            "リソースID",
            &form.service_principal_id,
            form.field == 0,
            false,
        ),
        input_line("キーID", &form.key_id, form.field == 1, false),
        input_line("RSA秘密鍵", &key_status, form.field == 2, false),
        Line::from(Span::styled(
            format!("{}PEM全文をこの欄へ貼り付けてください", " ".repeat(14)),
            Style::default().fg(DIM),
        )),
        Line::raw(""),
    ];
    if form.verifying {
        lines.push(Line::from(Span::styled(
            "Bearerトークンの発行とユーザー参照権限を検証しています…",
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
            Span::raw(" 閉じる"),
        ]));
    }

    let area = centered(frame, 76, dialog_height(&lines, 76));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("IAMサービスプリンシパル認証", accent())),
        area,
    );
}

/// サービス一覧を分類ごとの見出し付きで組む。
///
/// `spacious` が偽なら分類の間の空行を省く。分類の数だけ行が増えるので、
/// 低い端末では空行を落とさないと下のキーヒントが枠外に出てしまう。
pub(super) fn centered(frame: &Frame, width: u16, height: u16) -> Rect {
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
pub(super) fn dialog_height(lines: &[Line], width: u16) -> u16 {
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

pub(super) fn dialog(title: &str, color: Color) -> Block<'static> {
    Block::bordered()
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::default().fg(color))
        .padding(ratatui::widgets::Padding::new(2, 2, 1, 1))
}

pub(super) fn draw_help(frame: &mut Frame) {
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

pub(super) fn draw_message(
    frame: &mut Frame,
    title: &str,
    body: &str,
    kind: StatusKind,
    scroll: u16,
) {
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

pub(super) fn draw_confirm(
    frame: &mut Frame,
    title: &str,
    body: &str,
    verify: Option<&str>,
    typed: &str,
) {
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

pub(super) fn input_line(label: &str, value: &str, focused: bool, masked: bool) -> Line<'static> {
    input_line_at(label, value, focused, masked, 14)
}

/// ラベル幅を指定できる入力行。
///
/// 同じダイアログの中に長いラベルの選択欄が混ざるときは、そちらに合わせる。
/// 揃っていないと値の位置がばらつく。
pub(super) fn input_line_at(
    label: &str,
    value: &str,
    focused: bool,
    masked: bool,
    label_width: usize,
) -> Line<'static> {
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
        Span::styled(super::pad(label, label_width), label_style),
        Span::styled(shown, value_style),
        Span::styled(
            if focused { "▏" } else { "" },
            Style::default().fg(accent()),
        ),
    ])
}

/// 権限の選択欄。
pub(super) fn permission_line(selected: usize, focused: bool) -> Line<'static> {
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
