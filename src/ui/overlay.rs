//! 前面に重ねるダイアログ（ヘルプ・確認・入力フォーム）。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::{DIM, SAKURA};
use crate::app::Service;
use crate::app::{
    App, LoginForm, Overlay, RegistryForm, RegistryFormMode, StatusKind, UserForm, UserFormMode,
};
use crate::config::CredentialSource;
use crate::iaas::Zone;
use crate::sacloud::Permission;

pub fn draw(frame: &mut Frame, app: &App) {
    let Some(overlay) = &app.overlay else {
        return;
    };
    match overlay {
        Overlay::Help => draw_help(frame),
        Overlay::Message { title, body, kind } => draw_message(frame, title, body, *kind),
        Overlay::Confirm {
            title,
            body,
            verify,
            typed,
            ..
        } => draw_confirm(frame, title, body, verify.as_deref(), typed),
        Overlay::UserForm(form) => draw_user_form(frame, form),
        Overlay::RegistryForm(form) => draw_registry_form(frame, form),
        Overlay::Login(form) => draw_login_form(frame, form),
        Overlay::ProfilePicker { sources, index } => {
            draw_profile_picker(frame, sources, *index, &app.credential_source)
        }
        Overlay::ZonePicker { zones, index } => draw_zone_picker(frame, zones, *index, &app.zone),
        Overlay::ServicePicker { index } => draw_service_picker(frame, *index, app.service),
    }
}

fn draw_service_picker(frame: &mut Frame, index: usize, current: Service) {
    let mut lines = vec![Line::raw("")];
    for (i, service) in Service::ALL.iter().enumerate() {
        lines.push(picker_row(i == index, *service == current, service.title()));
    }
    lines.push(Line::raw(""));
    lines.push(picker_hint("切り替え"));

    let area = centered(frame, 52, dialog_height(&lines, 52));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("サービスの切り替え", SAKURA)),
        area,
    );
}

fn draw_zone_picker(frame: &mut Frame, zones: &[Zone], index: usize, current: &str) {
    let rows = zones
        .iter()
        .enumerate()
        .map(|(i, zone)| picker_row(i == index, zone.name == current, &zone.label()));
    let mut lines = vec![
        Line::from(Span::styled(
            "サーバーを表示するゾーンを選んでください",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
    ];
    lines.extend(rows);
    lines.push(Line::raw(""));
    lines.push(picker_hint("切り替え"));

    let area = centered(frame, 60, dialog_height(&lines, 60));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("ゾーンの切り替え", SAKURA)),
        area,
    );
}

/// ピッカーの 1 行（選択中は ▌、現在値は ●）。
fn picker_row(selected: bool, is_current: bool, label: &str) -> Line<'static> {
    let style = if selected {
        Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut spans = vec![
        Span::styled(
            if selected { "▌ " } else { "  " },
            Style::default().fg(SAKURA),
        ),
        Span::styled(
            format!("{} {label}", if is_current { "●" } else { "○" }),
            style,
        ),
    ];
    if is_current {
        spans.push(Span::styled(" (現在)", Style::default().fg(DIM)));
    }
    Line::from(spans)
}

fn picker_hint(action: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("↑↓/jk", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {action}   ")),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ])
}

fn draw_profile_picker(
    frame: &mut Frame,
    sources: &[CredentialSource],
    index: usize,
    current: &CredentialSource,
) {
    let mut lines = vec![Line::from(Span::styled(
        "切り替え先を選んでください",
        Style::default().fg(DIM),
    ))];
    lines.push(Line::raw(""));
    for (i, source) in sources.iter().enumerate() {
        lines.push(picker_row(i == index, source == current, &source.label()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "~/.usacloud/current は書き換えません（このセッションのみ）",
        Style::default().fg(DIM),
    )));
    lines.push(picker_hint("切り替え"));

    let area = centered(frame, 64, dialog_height(&lines, 64));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("認証情報の切り替え", SAKURA)),
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
                ("a", "ユーザーを追加"),
                ("e", "ユーザーを編集"),
                ("d", "選択中の項目を削除"),
                ("n", "レジストリを作成"),
                ("E", "レジストリを編集"),
                ("D", "レジストリを削除"),
                ("L", "レジストリにログイン"),
                ("O", "レジストリのログイン情報を破棄"),
                ("/", "表示中のリストを絞り込み"),
                ("y", "選択中の項目をコピー"),
                ("p", "認証情報（プロファイル）を切替"),
                ("s / S", "サービスを切り替え（4種）"),
                ("z", "ゾーンを切り替え（サーバー）"),
                ("t", "トラフィックを切替（AppRun共用型）"),
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
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
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
        Paragraph::new(lines).block(dialog("キーバインド", SAKURA)),
        area,
    );
}

fn draw_message(frame: &mut Frame, title: &str, body: &str, kind: StatusKind) {
    let color = match kind {
        StatusKind::Error => Color::Red,
        StatusKind::Success => Color::Green,
        StatusKind::Info => SAKURA,
    };
    let text = vec![
        Line::raw(body.to_string()),
        Line::raw(""),
        Line::from(Span::styled(
            "何かキーを押すと閉じます",
            Style::default().fg(DIM),
        )),
    ];
    let area = centered(frame, 70, dialog_height(&text, 70));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
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
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
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
            .block(dialog(&title, SAKURA)),
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
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
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
            .block(dialog(&title, SAKURA)),
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
                    Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
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
            "保存先: ~/.config/sakura-tui/config.toml (パスワードは平文・0600)",
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
                Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
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
            .block(dialog("レジストリへログイン", SAKURA)),
        area,
    );
}

/// 入力欄 1 行。フォーカス中はカーソルを表示する。
fn input_line(label: &str, value: &str, focused: bool, masked: bool) -> Line<'static> {
    let shown = if masked {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    let label_style = if focused {
        Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
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
        Span::styled(if focused { "▏" } else { "" }, Style::default().fg(SAKURA)),
    ])
}

/// 権限の選択欄。
fn permission_line(selected: usize, focused: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        super::pad("権限", 14),
        if focused {
            Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
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
                .fg(SAKURA)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default().fg(DIM)
        };
        spans.push(Span::styled(format!(" {} ", permission.as_str()), style));
    }
    Line::from(spans)
}
