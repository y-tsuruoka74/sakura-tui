//! 前面に重ねるダイアログ（ヘルプ・確認・入力フォーム）。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use super::{DIM, SAKURA};
use crate::app::{App, LoginForm, Overlay, StatusKind, UserForm, UserFormMode};
use crate::sacloud::Permission;

pub fn draw(frame: &mut Frame, app: &App) {
    let Some(overlay) = &app.overlay else {
        return;
    };
    match overlay {
        Overlay::Help => draw_help(frame),
        Overlay::Message { title, body, kind } => draw_message(frame, title, body, *kind),
        Overlay::Confirm { title, body, .. } => draw_confirm(frame, title, body),
        Overlay::UserForm(form) => draw_user_form(frame, form),
        Overlay::Login(form) => draw_login_form(frame, form),
    }
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
    let sections: [(&str, &[(&str, &str)]); 4] = [
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
            ],
        ),
        (
            "操作",
            &[
                ("r", "表示中のデータを再取得"),
                ("R", "全キャッシュを破棄して再取得"),
                ("a", "ユーザーを追加"),
                ("e", "ユーザーを編集"),
                ("d", "ユーザーを削除"),
                ("L", "レジストリにログイン"),
                ("O", "レジストリのログイン情報を破棄"),
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

    let height = (lines.len() as u16) + 4;
    let area = centered(frame, 60, height);
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
    let area = centered(frame, 70, 12);
    frame.render_widget(Clear, area);
    let text = vec![
        Line::raw(body.to_string()),
        Line::raw(""),
        Line::from(Span::styled(
            "何かキーを押すと閉じます",
            Style::default().fg(DIM),
        )),
    ];
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(dialog(title, color)),
        area,
    );
}

fn draw_confirm(frame: &mut Frame, title: &str, body: &str) {
    let area = centered(frame, 64, 11);
    frame.render_widget(Clear, area);
    let mut lines: Vec<Line> = body.lines().map(|l| Line::raw(l.to_string())).collect();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(
            "y / Enter",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" 実行    "),
        Span::styled("n / Esc", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" キャンセル"),
    ]));
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

    let height = lines.len() as u16 + 4;
    let area = centered(frame, 66, height);
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
            Span::raw(if form.save { "[x] する" } else { "[ ] しない" }),
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

    let height = lines.len() as u16 + 4;
    let area = centered(frame, 72, height);
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
        Span::styled(
            if focused { "▏" } else { "" },
            Style::default().fg(SAKURA),
        ),
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
