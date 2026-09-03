//! 入力フォームの描画。
//!
//! 定義と編集操作は `src/app/forms.rs` にあり、ここは見た目だけを持つ。

use super::*;
use crate::ui::clip;

pub(super) fn draw_user_form(frame: &mut Frame, form: &UserForm) {
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

pub(super) fn draw_registry_form(frame: &mut Frame, form: &RegistryForm) {
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

pub(super) fn draw_iam_resource_form(frame: &mut Frame, form: &IamResourceForm) {
    let action = match form.mode {
        IamResourceFormMode::Create => "作成",
        IamResourceFormMode::Edit => "編集",
    };
    let title = format!("IAM {}の{action}", form.resource_type);
    let mut lines: Vec<Line> = form
        .labels()
        .iter()
        .enumerate()
        .map(|(index, label)| {
            input_line(
                label,
                form.value(index),
                form.field == index,
                *label == "パスワード",
            )
        })
        .collect();
    if form.mode == IamResourceFormMode::Edit && form.resource_type == "ユーザー" {
        lines.push(Line::from(Span::styled(
            "パスワードは空欄なら変更しません。メール変更は現在未対応です。",
            Style::default().fg(DIM),
        )));
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
    let area = centered(frame, 72, dialog_height(&lines, 72));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

pub(super) fn draw_iam_role_form(frame: &mut Frame, form: &IamRoleForm) {
    let action = if form.grant { "付与" } else { "解除" };
    let mut lines: Vec<Line> = IamRoleForm::LABELS
        .iter()
        .enumerate()
        .map(|(index, label)| input_line(label, form.value(index), form.field == index, false))
        .collect();
    lines.push(Line::from(Span::styled(
        "プリンシパル種別: user / group / service-principal",
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
        Span::raw(format!(" {action}確認へ   ")),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));
    let area = centered(frame, 76, dialog_height(&lines, 76));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&format!("IAMロールの{action}"), accent())),
        area,
    );
}

pub(super) fn draw_switch_form(frame: &mut Frame, form: &SwitchForm) {
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

/// サーバー作成フォーム。選択式の欄は今選ばれているものと、一覧の中の位置を出す。
pub(super) fn draw_server_create_form(
    frame: &mut Frame,
    form: &ServerCreateForm,
    app: &crate::app::App,
) {
    let choices = app.server_choices();
    let loading = app.server.plans.ready().is_none();

    let cpus = crate::iaas::cpu_choices(&choices.plans);
    let memories = crate::iaas::memory_choices(&choices.plans, form.cpu);

    // 選べる値と、その中で今どこにいるか。一覧が長いので位置が分かるようにする。
    let choice_row =
        |label: &str, text: String, pos: Option<usize>, total: usize, focused: bool| {
            // スタートアップスクリプト名などは長い。折り返して行がずれないよう切る。
            let text = clip(&text, CHOICE_TEXT_WIDTH);
            let shown = if focused {
                format!("‹ {text} ›")
            } else {
                format!("  {text}  ")
            };
            let style = if focused {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let mut spans = vec![
                Span::styled(
                    crate::ui::pad(label, 16),
                    if focused {
                        style
                    } else {
                        Style::default().fg(DIM)
                    },
                ),
                Span::styled(crate::ui::pad(&shown, 28), style),
            ];
            if let Some(pos) = pos
                && total > 1
            {
                spans.push(Span::styled(
                    format!("{}/{total}", pos + 1),
                    Style::default().fg(DIM),
                ));
            }
            Line::from(spans)
        };

    let unknown = || {
        if loading {
            "読み込み中…".to_string()
        } else {
            "選べる値がありません".to_string()
        }
    };
    let at = |values: &[u32], current: u32| values.iter().position(|v| *v == current);

    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in form.fields(&choices).iter().enumerate() {
        let focused = form.field == i;
        let label = field.label();
        let row = match field {
            ServerField::Cpu => choice_row(
                label,
                if cpus.is_empty() {
                    unknown()
                } else {
                    format!("{} コア", form.cpu)
                },
                at(&cpus, form.cpu),
                cpus.len(),
                focused,
            ),
            ServerField::Memory => choice_row(
                label,
                if memories.is_empty() {
                    unknown()
                } else {
                    format!("{} GB", form.memory_mb / 1024)
                },
                at(&memories, form.memory_mb),
                memories.len(),
                focused,
            ),
            ServerField::Os => choice_row(
                label,
                crate::iaas::OS_CHOICES[form.os.min(crate::iaas::OS_CHOICES.len() - 1)]
                    .label
                    .to_string(),
                Some(form.os),
                crate::iaas::OS_CHOICES.len(),
                focused,
            ),
            ServerField::DiskSize => choice_row(
                label,
                if choices.disk_sizes.is_empty() {
                    unknown()
                } else {
                    format!("{} GB", form.disk_size_mb / 1024)
                },
                at(&choices.disk_sizes, form.disk_size_mb),
                choices.disk_sizes.len(),
                focused,
            ),
            ServerField::Nic => choice_row(
                label,
                choices.nic(form.nic).label(),
                Some(form.nic),
                choices.nics.len(),
                focused,
            ),
            ServerField::PacketFilter => choice_row(
                label,
                choices
                    .packet_filter(form.packet_filter)
                    .map_or_else(|| "なし".to_string(), |(_, name)| name),
                Some(form.packet_filter),
                choices.packet_filters.len(),
                focused,
            ),
            ServerField::StartupScript => choice_row(
                label,
                choices
                    .startup_script(form.startup_script)
                    .map_or_else(|| "なし".to_string(), |s| s.name),
                Some(form.startup_script),
                choices.startup_scripts.len(),
                focused,
            ),
            ServerField::Boot => choice_row(
                label,
                if form.boot_after_create {
                    "作成後に起動する".to_string()
                } else {
                    "作成後は停止のまま".to_string()
                },
                None,
                0,
                focused,
            ),
            // 公開鍵はそのまま出すと数百文字あって画面に収まらない。要約で出す。
            ServerField::SshKey => {
                let shown = if form.ssh_public_key.trim().is_empty() {
                    String::new()
                } else {
                    crate::pubkey::PublicKey {
                        label: String::new(),
                        key: form.ssh_public_key.clone(),
                    }
                    .summary()
                };
                input_line(label, &shown, focused, false)
            }
            ServerField::Password => input_line(label, form.value(*field), focused, true),
            _ => input_line(label, form.value(*field), focused, false),
        };
        lines.push(row);
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "ホスト名を省くとサーバー名を使います。SSH公開鍵を入れるとパスワード認証を切ります。",
        Style::default().fg(DIM),
    )));
    if matches!(choices.nic(form.nic), NicChoice::Switch(..)) {
        lines.push(Line::from(Span::styled(
            "スイッチにはDHCPが無いので、IPアドレスとマスク長は必須です。",
            Style::default().fg(DIM),
        )));
    }
    lines.push(Line::from(Span::styled(
        "ディスクは作成した時点から、サーバーは起動した時点から課金されます。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    let mut hint = vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled("←→", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 選択   "),
    ];
    // 公開鍵の欄にいるときだけ、鍵を選べることを出す。
    if form.current(&choices) == ServerField::SshKey {
        hint.push(Span::styled(
            "Ctrl+K",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ));
        hint.push(Span::raw(" 公開鍵を選ぶ   "));
    }
    // 候補が多い欄では、絞り込んで選べることを出す。
    if ServerChoices::is_list_field(form.current(&choices)) {
        hint.push(Span::styled(
            "/",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ));
        hint.push(Span::raw(" 一覧から探す   "));
    }
    hint.extend([
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 確認へ   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]);
    lines.push(Line::from(hint));

    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("サーバーの作成", accent())),
        area,
    );
}

/// 選択式の欄に出せる値の幅。ラベルと位置表示を除いた残り。
const CHOICE_TEXT_WIDTH: usize = 44;

/// NIC の接続先やパケットフィルタを選ぶ画面。
///
/// 候補の出どころが作成フォームと同じなので、行の作り方もそろえる。
pub(super) fn draw_nic_picker(frame: &mut Frame, picker: &NicPicker, app: &crate::app::App) {
    let choices = app.server_choices();
    let rows = picker.visible(&choices);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(crate::ui::pad("今の接続先", 14), Style::default().fg(DIM)),
            Span::raw(picker.nic.connection.label()),
        ]),
        Line::from(vec![
            Span::styled(crate::ui::pad("今のフィルタ", 14), Style::default().fg(DIM)),
            Span::raw(
                picker
                    .nic
                    .packet_filter
                    .as_ref()
                    .map_or_else(|| "なし".to_string(), Clone::clone),
            ),
        ]),
        Line::raw(""),
        input_line("絞り込み", &picker.filter, true, false),
        Line::raw(""),
    ];

    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "一致するものがありません",
            Style::default().fg(DIM),
        )));
    } else {
        let start = picker
            .index
            .saturating_sub(CHOICE_PICKER_ROWS - 1)
            .min(rows.len().saturating_sub(CHOICE_PICKER_ROWS));
        for (i, row) in rows.iter().enumerate().skip(start).take(CHOICE_PICKER_ROWS) {
            lines.push(selectable_line(
                &row.label,
                &row.detail,
                i == picker.index,
                CHOICE_PICKER_LABEL_WIDTH,
                CHOICE_PICKER_ROW_WIDTH,
            ));
        }
        if rows.len() > CHOICE_PICKER_ROWS {
            lines.push(Line::from(Span::styled(
                format!("{} / {} 件", picker.index + 1, rows.len()),
                Style::default().fg(DIM),
            )));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("文字入力", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 絞り込み   "),
        Span::styled("↑↓", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 決定   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let width = CHOICE_PICKER_WIDTH;
    let area = centered(frame, width, dialog_height(&lines, width));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&picker.title(), accent())),
        area,
    );
}

/// 候補が多い欄を絞り込みながら選ぶ画面。
const CHOICE_PICKER_WIDTH: u16 = 88;
const CHOICE_PICKER_ROW_WIDTH: usize =
    CHOICE_PICKER_WIDTH as usize - super::DIALOG_PADDING as usize - MARKER_WIDTH;
const CHOICE_PICKER_LABEL_WIDTH: usize = 46;
/// 一度に出す行数。これを超える分はスクロールさせる。
const CHOICE_PICKER_ROWS: usize = 12;

pub(super) fn draw_server_choice_picker(
    frame: &mut Frame,
    picker: &ServerChoicePicker,
    app: &crate::app::App,
) {
    let choices = app.server_choices();
    let rows = picker.visible(&choices);

    let mut lines = vec![
        input_line("絞り込み", &picker.filter, true, false),
        Line::raw(""),
    ];

    if rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "一致するものがありません",
            Style::default().fg(DIM),
        )));
    } else {
        // 選んでいる行が常に見えるよう、その行を含む範囲だけ出す。
        let start = picker
            .index
            .saturating_sub(CHOICE_PICKER_ROWS - 1)
            .min(rows.len().saturating_sub(CHOICE_PICKER_ROWS));
        for (i, row) in rows.iter().enumerate().skip(start).take(CHOICE_PICKER_ROWS) {
            lines.push(selectable_line(
                &row.label,
                &row.detail,
                i == picker.index,
                CHOICE_PICKER_LABEL_WIDTH,
                CHOICE_PICKER_ROW_WIDTH,
            ));
        }
        if rows.len() > CHOICE_PICKER_ROWS {
            lines.push(Line::from(Span::styled(
                format!(
                    "{} / {} 件（絞り込むと減ります）",
                    picker.index + 1,
                    rows.len()
                ),
                Style::default().fg(DIM),
            )));
        }
        // 選んでいるものの説明は、行に収まらないので下に出す。
        if let Some(note) = rows.get(picker.index).map(|r| r.note.as_str())
            && !note.is_empty()
        {
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                note.to_string(),
                Style::default().fg(DIM),
            )));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("文字入力", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 絞り込み   "),
        Span::styled("↑↓", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 移動   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 決定   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 戻る"),
    ]));

    let width = CHOICE_PICKER_WIDTH;
    let area = centered(frame, width, dialog_height(&lines, width));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&picker.title(), accent())),
        area,
    );
}

/// SSH 公開鍵の取得元と、取れた鍵の一覧。
pub(super) fn draw_ssh_key_picker(frame: &mut Frame, back: &SshKeyReturn, stage: &SshKeyStage) {
    let mut lines: Vec<Line> = Vec::new();
    let title;

    match stage {
        SshKeyStage::Source { index } => {
            title = "公開鍵の取得元".to_string();
            for (i, source) in back.sources().iter().enumerate() {
                lines.push(selectable_line(
                    source.label(),
                    source.detail(),
                    i == *index,
                    SSH_SOURCE_LABEL_WIDTH,
                    SSH_KEY_ROW_WIDTH,
                ));
            }
            lines.push(Line::raw(""));
            lines.push(picker_hint("選ぶ"));
        }
        SshKeyStage::GithubUser { user } => {
            title = "GitHub のユーザー名".to_string();
            lines.push(input_line("ユーザー名", user, true, false));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "github.com/<名前>.keys で公開されている鍵を読みます。",
                Style::default().fg(DIM),
            )));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled(
                    "Enter",
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" 取得   "),
                Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" 戻る"),
            ]));
        }
        SshKeyStage::Loading { from } => {
            title = "公開鍵".to_string();
            lines.push(Line::from(Span::styled(
                format!("{from} から取得しています…"),
                Style::default().fg(DIM),
            )));
        }
        SshKeyStage::Keys { from, keys, index } => {
            title = format!("公開鍵 — {from}");
            for (i, key) in keys.iter().enumerate() {
                lines.push(selectable_line(
                    &key.label,
                    &key.summary(),
                    i == *index,
                    SSH_KEY_LABEL_WIDTH,
                    SSH_KEY_ROW_WIDTH,
                ));
            }
            lines.push(Line::raw(""));
            lines.push(picker_hint("入れる"));
        }
    }

    let width = SSH_KEY_PICKER_WIDTH;
    let area = centered(frame, width, dialog_height(&lines, width));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

/// SSH公開鍵の登録・編集フォーム。
pub(super) fn draw_ssh_key_form(frame: &mut Frame, form: &SshKeyForm) {
    let title = match form.mode {
        SshKeyFormMode::Add => "公開鍵の登録".to_string(),
        SshKeyFormMode::Edit => format!("公開鍵の編集 — {}", form.name),
    };

    let mut lines: Vec<Line> = form
        .labels()
        .iter()
        .enumerate()
        .map(|(i, label)| {
            // 鍵はそのまま出すと数百文字あるので要約で出す。
            if i == SshKeyForm::PUBLIC_KEY_FIELD {
                let shown = if form.public_key.trim().is_empty() {
                    String::new()
                } else {
                    crate::pubkey::PublicKey {
                        label: String::new(),
                        key: form.public_key.clone(),
                    }
                    .summary()
                };
                input_line(label, &shown, form.field == i, false)
            } else {
                input_line(label, form.value(i), form.field == i, false)
            }
        })
        .collect();

    lines.push(Line::raw(""));
    match form.mode {
        SshKeyFormMode::Add => lines.push(Line::from(Span::styled(
            "登録した鍵はサーバー作成のときに選べます。",
            Style::default().fg(DIM),
        ))),
        SshKeyFormMode::Edit => lines.push(Line::from(Span::styled(
            "鍵そのものは変更できません。変えるときは登録し直してください。",
            Style::default().fg(DIM),
        ))),
    }
    lines.push(Line::raw(""));

    let mut hint = vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
    ];
    if form.mode == SshKeyFormMode::Add {
        hint.push(Span::styled(
            "Ctrl+K",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ));
        hint.push(Span::raw(" 公開鍵を選ぶ   "));
    }
    hint.extend([
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]);
    lines.push(Line::from(hint));

    let area = centered(frame, 74, dialog_height(&lines, 74));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(&title, accent())),
        area,
    );
}

/// パケットフィルタ本体のフォーム。
pub(super) fn draw_packet_filter_form(frame: &mut Frame, form: &PacketFilterForm) {
    let title = match form.mode {
        PacketFilterFormMode::Create => "パケットフィルタの作成".to_string(),
        PacketFilterFormMode::Edit => format!("パケットフィルタの編集 — {}", form.name),
    };
    let mut lines: Vec<Line> = PacketFilterForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "ルールは作成後にこの画面で足せます。",
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

/// パケットフィルタのルールのフォーム。
pub(super) fn draw_rule_form(frame: &mut Frame, form: &RuleForm) {
    let title = match form.mode {
        RuleFormMode::Add => "ルールの追加",
        RuleFormMode::Edit => "ルールの編集",
    };

    let choice_row = |label: &str, text: &str, pos: usize, total: usize, focused: bool| {
        let shown = if focused {
            format!("‹ {text} ›")
        } else {
            format!("  {text}  ")
        };
        let style = if focused {
            Style::default().fg(accent()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Line::from(vec![
            Span::styled(
                crate::ui::pad(label, 20),
                if focused {
                    style
                } else {
                    Style::default().fg(DIM)
                },
            ),
            Span::styled(crate::ui::pad(&shown, 16), style),
            Span::styled(format!("{}/{total}", pos + 1), Style::default().fg(DIM)),
        ])
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in form.fields().iter().enumerate() {
        let focused = form.field == i;
        lines.push(match field {
            RuleField::Protocol => choice_row(
                field.label(),
                form.protocol(),
                form.protocol,
                crate::packet_filter::PROTOCOLS.len(),
                focused,
            ),
            RuleField::Action => choice_row(
                field.label(),
                form.action(),
                form.action,
                crate::packet_filter::ACTIONS.len(),
                focused,
            ),
            _ => input_line_at(field.label(), form.value(*field), focused, false, 20),
        });
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "空欄は「すべて」の意味です。ポートは 80 か 80-89 の形で入れます。",
        Style::default().fg(DIM),
    )));
    if !crate::packet_filter::PacketFilterRule::takes_port(form.protocol()) {
        lines.push(Line::from(Span::styled(
            format!("{} はポートを指定できません。", form.protocol()),
            Style::default().fg(DIM),
        )));
    }
    lines.push(Line::from(Span::styled(
        "ルールは上から順に評価され、どれにも当たらない通信は拒否されます。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled("←→", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 選択   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 74, dialog_height(&lines, 74));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(title, accent())),
        area,
    );
}

/// ディスクの作成フォーム。選択式の欄は一覧の中の位置も出す。
pub(super) fn draw_disk_create_form(
    frame: &mut Frame,
    form: &DiskCreateForm,
    app: &crate::app::App,
) {
    let plans = app.disk_plan_choices();
    let archives = app.disk_archive_choices();
    let loading = app.disk.plans.ready().is_none();
    let sizes = crate::app::sizes_of(&plans, form.plan_id);

    let choice_row =
        |label: &str, text: String, pos: Option<usize>, total: usize, focused: bool| {
            // 長い名前で折り返して行がずれないよう切る。
            let text = clip(&text, CHOICE_TEXT_WIDTH);
            let shown = if focused {
                format!("‹ {text} ›")
            } else {
                format!("  {text}  ")
            };
            let style = if focused {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let mut spans = vec![
                Span::styled(
                    crate::ui::pad(label, 14),
                    if focused {
                        style
                    } else {
                        Style::default().fg(DIM)
                    },
                ),
                Span::styled(crate::ui::pad(&shown, 30), style),
            ];
            if let Some(pos) = pos
                && total > 1
            {
                spans.push(Span::styled(
                    format!("{}/{total}", pos + 1),
                    Style::default().fg(DIM),
                ));
            }
            Line::from(spans)
        };
    let unknown = || {
        if loading {
            "読み込み中…".to_string()
        } else {
            "選べる値がありません".to_string()
        }
    };

    let plan_pos = plans.iter().position(|p| p.id == form.plan_id);
    let plan_text = match plans.iter().find(|p| p.id == form.plan_id) {
        Some(plan) => plan.name.clone(),
        None => unknown(),
    };
    let sources = form.source_rows(&archives);

    let mut lines: Vec<Line> = Vec::new();
    for (i, field) in form.fields().iter().enumerate() {
        let focused = form.field == i;
        let label = field.label();
        lines.push(match field {
            DiskField::Plan => choice_row(label, plan_text.clone(), plan_pos, plans.len(), focused),
            DiskField::Size => choice_row(
                label,
                if sizes.is_empty() {
                    unknown()
                } else {
                    format!("{} GB", form.size_mb / 1024)
                },
                sizes.iter().position(|mb| *mb == form.size_mb),
                sizes.len(),
                focused,
            ),
            DiskField::SourceKind => choice_row(
                label,
                form.kind().label().to_string(),
                Some(form.source_kind),
                DiskSourceKind::ALL.len(),
                focused,
            ),
            DiskField::Source => choice_row(
                label,
                form.source_label(&archives),
                (!sources.is_empty()).then_some(form.source),
                sources.len(),
                focused,
            ),
            _ => input_line(label, form.value(*field), focused, false),
        });
    }

    lines.push(Line::raw(""));
    if form.kind().needs_source() {
        lines.push(Line::from(Span::styled(
            "元にするものがあるとコピーが走り、使えるまで数分かかります。",
            Style::default().fg(DIM),
        )));
    }
    lines.push(Line::from(Span::styled(
        "ディスクは作成した時点から課金されます。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled("←→", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 選択   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 確認へ   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 78, dialog_height(&lines, 78));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("ディスクの作成", accent())),
        area,
    );
}

/// ディスクからアーカイブを取るフォーム。
pub(super) fn draw_archive_form(frame: &mut Frame, form: &ArchiveForm, app: &crate::app::App) {
    let sources = app.archive_source_choices();
    let loading = app.disk.sources.ready().is_none();

    let mut lines: Vec<Line> = Vec::new();
    for (i, label) in ArchiveForm::LABELS.iter().enumerate() {
        let focused = form.field == i;
        if i == ArchiveForm::SOURCE_FIELD {
            let text = match sources.get(form.source) {
                Some((_, name)) => clip(name, CHOICE_TEXT_WIDTH),
                None if loading => "読み込み中…".to_string(),
                None => "このゾーンにディスクがありません".to_string(),
            };
            let shown = if focused {
                format!("‹ {text} ›")
            } else {
                format!("  {text}  ")
            };
            let style = if focused {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let mut spans = vec![
                Span::styled(
                    crate::ui::pad(label, 14),
                    if focused {
                        style
                    } else {
                        Style::default().fg(DIM)
                    },
                ),
                Span::styled(crate::ui::pad(&shown, 30), style),
            ];
            if sources.len() > 1 {
                spans.push(Span::styled(
                    format!("{}/{}", form.source + 1, sources.len()),
                    Style::default().fg(DIM),
                ));
            }
            lines.push(Line::from(spans));
        } else {
            lines.push(input_line(label, form.value(i), focused, false));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "起動中のサーバーに繋がったディスクから取ると、中身が壊れていることがあります。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 項目移動   "),
        Span::styled("←→", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" ディスク   "),
        Span::styled(
            "Enter",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 確認へ   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 74, dialog_height(&lines, 74));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("アーカイブの作成", accent())),
        area,
    );
}

/// ディスクの接続先を出すダイアログの幅。
const DISK_SERVER_PICKER_WIDTH: u16 = 74;
const DISK_SERVER_ROW_WIDTH: usize =
    DISK_SERVER_PICKER_WIDTH as usize - super::DIALOG_PADDING as usize - MARKER_WIDTH;
const DISK_SERVER_LABEL_WIDTH: usize = 44;

/// ディスクの接続先サーバーを選ぶ画面。
pub(super) fn draw_disk_server_picker(frame: &mut Frame, picker: &DiskServerPicker) {
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(crate::ui::pad("ディスク", 12), Style::default().fg(DIM)),
            Span::raw(picker.disk_name.clone()),
        ]),
        Line::raw(""),
    ];
    match &picker.servers {
        Loadable::Ready(servers) if servers.is_empty() => {
            lines.push(Line::from(Span::styled(
                "接続できるサーバーがありません。接続できるのは停止中のサーバーだけです。",
                Style::default().fg(DIM),
            )));
        }
        Loadable::Ready(servers) => {
            for (i, (id, name)) in servers.iter().enumerate() {
                lines.push(selectable_line(
                    name,
                    &id.to_string(),
                    i == picker.index,
                    // 接続先はサーバー名だけなので、幅を名前に多めに回す。
                    DISK_SERVER_LABEL_WIDTH,
                    DISK_SERVER_ROW_WIDTH,
                ));
            }
        }
        Loadable::Failed(err) => {
            lines.push(Line::from(Span::styled(
                err.clone(),
                Style::default().fg(DIM),
            )));
        }
        _ => lines.push(Line::from(Span::styled(
            "停止中のサーバーを探しています…",
            Style::default().fg(DIM),
        ))),
    }
    lines.push(Line::raw(""));
    lines.push(picker_hint("接続する"));

    let width = DISK_SERVER_PICKER_WIDTH;
    let area = centered(frame, width, dialog_height(&lines, width));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("ディスクの接続先", accent())),
        area,
    );
}

/// サーバーのプラン変更フォーム。変更前の構成を並べて出す。
pub(super) fn draw_server_plan_form(
    frame: &mut Frame,
    form: &ServerPlanForm,
    app: &crate::app::App,
) {
    let plans = app.server_plan_choices();
    let loading = app.server.plans.ready().is_none();
    let cpus = crate::iaas::cpu_choices(&plans);
    let memories = crate::iaas::memory_choices(&plans, form.cpu);

    let row = |label: &str, text: String, choices: &[u32], current: u32, focused: bool| {
        let shown = if focused {
            format!("‹ {text} ›")
        } else {
            format!("  {text}  ")
        };
        let style = if focused {
            Style::default().fg(accent()).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let mut spans = vec![
            Span::styled(
                crate::ui::pad(label, 12),
                if focused {
                    style
                } else {
                    Style::default().fg(DIM)
                },
            ),
            Span::styled(crate::ui::pad(&shown, 20), style),
        ];
        if let Some(pos) = choices.iter().position(|v| *v == current)
            && choices.len() > 1
        {
            spans.push(Span::styled(
                format!("{}/{}", pos + 1, choices.len()),
                Style::default().fg(DIM),
            ));
        }
        Line::from(spans)
    };
    let unknown = || {
        if loading {
            "読み込み中…".to_string()
        } else {
            "選べる値がありません".to_string()
        }
    };

    let lines = vec![
        Line::from(Span::styled(
            form.server_name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled(crate::ui::pad("変更前", 12), Style::default().fg(DIM)),
            Span::raw(format!(
                "  {} コア / {} GB",
                form.original_cpu,
                form.original_memory_mb / 1024
            )),
        ]),
        Line::raw(""),
        row(
            ServerPlanForm::LABELS[0],
            if cpus.is_empty() {
                unknown()
            } else {
                format!("{} コア", form.cpu)
            },
            &cpus,
            form.cpu,
            form.field == 0,
        ),
        row(
            ServerPlanForm::LABELS[1],
            if memories.is_empty() {
                unknown()
            } else {
                format!("{} GB", form.memory_mb / 1024)
            },
            &memories,
            form.memory_mb,
            form.field == 1,
        ),
        Line::raw(""),
        Line::from(Span::styled(
            "ディスクと NIC はそのまま引き継がれますが、サーバーの ID が変わります。",
            Style::default().fg(DIM),
        )),
        Line::raw(""),
        Line::from(vec![
            Span::styled("Tab", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 項目移動   "),
            Span::styled("←→", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 選択   "),
            Span::styled(
                "Enter",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" 確認へ   "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" 中止"),
        ]),
    ];

    let area = centered(frame, 66, dialog_height(&lines, 66));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("プランの変更", accent())),
        area,
    );
}

/// 公開鍵の一覧を出すダイアログの幅。
///
/// 鍵の名前とコメントを1行に並べるので、他のフォームより広く取る。
const SSH_KEY_PICKER_WIDTH: u16 = 84;
/// 行頭の ▸ に使う幅。
const MARKER_WIDTH: usize = 3;
/// 枠と余白を除いた、公開鍵の一覧の1行に書ける桁数。
const SSH_KEY_ROW_WIDTH: usize =
    SSH_KEY_PICKER_WIDTH as usize - super::DIALOG_PADDING as usize - MARKER_WIDTH;
/// 取得元の一覧。名前が長く、右の説明は短い。
const SSH_SOURCE_LABEL_WIDTH: usize = 36;
/// 鍵の一覧。名前は短めで、右のコメントに幅を回す。
const SSH_KEY_LABEL_WIDTH: usize = 26;

/// 一覧の1行。選ばれているものに ▸ を付ける。
///
/// 名前も右の説明も長さが読めないので、折り返して行がずれないよう幅で切る。
fn selectable_line(
    label: &str,
    detail: &str,
    selected: bool,
    label_width: usize,
    row_width: usize,
) -> Line<'static> {
    let style = if selected {
        Style::default().fg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(if selected { " ▸ " } else { "   " }, style),
        // 切り詰めたときも右の説明とくっつかないよう、2桁ぶん空けて切る。
        Span::styled(
            crate::ui::pad(&clip(label, label_width - 2), label_width),
            style,
        ),
        Span::styled(
            clip(detail, row_width.saturating_sub(label_width)),
            Style::default().fg(DIM),
        ),
    ])
}

pub(super) fn draw_rag_edit_form(frame: &mut Frame, form: &RagEditForm) {
    let mut lines: Vec<Line> = RagEditForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "モデルと分割サイズは取り込み時に決まるため変更できません。",
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
        Span::raw(" 更新   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 72, dialog_height(&lines, 72));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog(
                &format!("ドキュメントの編集 — {}", form.original_name),
                accent(),
            )),
        area,
    );
}

pub(super) fn draw_rag_upload_form(frame: &mut Frame, form: &RagUploadForm) {
    let mut lines: Vec<Line> = RagUploadForm::LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| input_line(label, form.value(i), form.field == i, false))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "名前を省くとファイル名が使われます。モデルと分割サイズを空にすると既定値になります。",
        Style::default().fg(DIM),
    )));
    lines.push(Line::from(Span::styled(
        "取り込みには時間がかかります。状態は一覧で確認してください。",
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
        Span::raw(" アップロード   "),
        Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" 中止"),
    ]));

    let area = centered(frame, 76, dialog_height(&lines, 76));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("ドキュメントのアップロード", accent())),
        area,
    );
}

pub(super) fn draw_dns_record_form(frame: &mut Frame, form: &DnsRecordForm) {
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

pub(super) fn draw_dns_zone_form(frame: &mut Frame, form: &DnsZoneForm) {
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

pub(super) fn draw_simple_monitor_form(frame: &mut Frame, form: &SimpleMonitorForm) {
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

pub(super) fn draw_vault_form(frame: &mut Frame, form: &VaultForm) {
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

pub(super) fn draw_secret_form(frame: &mut Frame, form: &SecretForm) {
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

pub(super) fn form_footer() -> Line<'static> {
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

pub(super) fn draw_alert_project_form(frame: &mut Frame, form: &AlertProjectForm) {
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

pub(super) fn draw_alert_rule_form(frame: &mut Frame, form: &AlertRuleForm) {
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

pub(super) fn draw_log_measure_rule_form(frame: &mut Frame, form: &LogMeasureRuleForm) {
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

pub(super) fn draw_log_routing_form(frame: &mut Frame, form: &LogRoutingForm) {
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

pub(super) fn draw_metrics_routing_form(frame: &mut Frame, form: &MetricsRoutingForm) {
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

pub(super) fn draw_dashboard_form(frame: &mut Frame, form: &DashboardForm) {
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

pub(super) fn draw_notification_target_form(frame: &mut Frame, form: &NotificationTargetForm) {
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

pub(super) fn draw_notification_routing_form(frame: &mut Frame, form: &NotificationRoutingForm) {
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

pub(super) fn draw_storage_form(frame: &mut Frame, form: &StorageForm) {
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

pub(super) fn draw_storage_retention_form(frame: &mut Frame, form: &StorageRetentionForm) {
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

pub(super) fn draw_storage_access_key_form(frame: &mut Frame, form: &StorageAccessKeyForm) {
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

pub(super) fn draw_login_form(frame: &mut Frame, form: &LoginForm) {
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
                crate::ui::pad("設定に保存", 14),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SshKeySource;

    /// 長い名前で行が折り返さないこと。折り返すと一覧の行がずれる。
    #[test]
    fn clipping_keeps_a_row_within_its_column() {
        use unicode_width::UnicodeWidthStr;
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("0123456789abc", 10), "012345678…");
        // 全角でも桁数ではなく表示幅で数える。
        let clipped = clip("社用パソコンの公開鍵です", 10);
        assert!(clipped.width() <= 10, "{clipped} が 10 桁を超えた");
        assert!(clipped.ends_with('…'));
    }

    /// 一覧の1行が枠に収まること。溢れると折り返して行がずれる。
    #[test]
    fn a_row_fits_inside_its_dialog() {
        use unicode_width::UnicodeWidthStr;
        let width = |line: ratatui::text::Line| -> usize {
            line.spans.iter().map(|s| s.content.width()).sum()
        };
        let long = "とても長い名前がついた公開鍵のファイル.pub";

        let key_row = selectable_line(
            long,
            "ssh-ed25519 …162vD11s7JNX  y-tsuruoka74@github.com",
            true,
            SSH_KEY_LABEL_WIDTH,
            SSH_KEY_ROW_WIDTH,
        );
        // 取得元の名前は鍵の名前より長い。こちらも収まること。
        let source_row = selectable_line(
            SshKeySource::Sacloud.label(),
            SshKeySource::Sacloud.detail(),
            true,
            SSH_SOURCE_LABEL_WIDTH,
            SSH_KEY_ROW_WIDTH,
        );
        let inner = SSH_KEY_PICKER_WIDTH as usize - super::super::DIALOG_PADDING as usize;
        for (name, line) in [("鍵", key_row), ("取得元", source_row)] {
            let cells = width(line);
            assert!(
                cells <= inner,
                "{name}の行 {cells} 桁が {inner} 桁に収まらない"
            );
        }
        // 取得元の名前は切り詰めずに出せること。
        assert_eq!(
            clip(SshKeySource::Sacloud.label(), SSH_SOURCE_LABEL_WIDTH),
            SshKeySource::Sacloud.label()
        );

        let disk_row = selectable_line(
            long,
            "113802075714",
            true,
            DISK_SERVER_LABEL_WIDTH,
            DISK_SERVER_ROW_WIDTH,
        );
        let inner = DISK_SERVER_PICKER_WIDTH as usize - super::super::DIALOG_PADDING as usize;
        let cells = width(disk_row);
        assert!(
            cells <= inner,
            "接続先の行 {cells} 桁が {inner} 桁に収まらない"
        );
    }
}
