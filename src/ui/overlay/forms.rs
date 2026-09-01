//! 入力フォームの描画。
//!
//! 定義と編集操作は `src/app/forms.rs` にあり、ここは見た目だけを持つ。

use super::*;

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
