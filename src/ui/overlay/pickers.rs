//! サービス・ゾーン・認証情報・ログインの選択画面。
//!
//! どれも一覧から1つ選ぶだけの画面で、共通の枠は `super` のヘルパを使う。

use super::*;

pub(super) fn service_picker_lines(
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
        let used = crate::ui::width(&spans);
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

pub(super) fn aligned_padding(
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
pub(super) fn category_picker_line(
    category: Category,
    selected: bool,
    content_width: usize,
) -> Line<'static> {
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
pub(super) fn service_count_text(app: &App, service: Service, label: &str) -> (String, Style) {
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

pub(super) fn draw_service_picker(frame: &mut Frame, app: &App, index: usize, initial: bool) {
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

pub(super) fn service_picker_hint(action: &str) -> Line<'static> {
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

pub(super) fn draw_zone_picker(frame: &mut Frame, app: &App, zones: &[Zone], index: usize) {
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
                crate::ui::pad(&zone.label(), 22),
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
pub(super) fn zone_count_span(app: &App, zone: &str, label: &str) -> Span<'static> {
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

pub(super) fn picker_hint(action: &str) -> Line<'static> {
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

pub(super) fn draw_profile_picker(
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
            .and_then(crate::ui::parse_color);

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
            Span::styled(crate::ui::pad(&source.label(), 26), name_style),
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

/// 保存済みのユーザー名から選ぶログイン画面。末尾に「新しく入力する」を1件追加する。
pub(super) fn draw_login_picker(frame: &mut Frame, host: &str, accounts: &[String], index: usize) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("ホスト  ", Style::default().fg(DIM)),
            Span::styled(host.to_string(), Style::default().fg(Color::Cyan)),
        ]),
        Line::raw(""),
    ];

    for (i, username) in accounts.iter().enumerate() {
        let selected = i == index;
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▌ " } else { "  " },
                Style::default().fg(accent()),
            ),
            Span::styled(
                username.clone(),
                if selected {
                    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
        ]));
    }
    let entering_new = index == accounts.len();
    lines.push(Line::from(vec![
        Span::styled(
            if entering_new { "▌ " } else { "  " },
            Style::default().fg(accent()),
        ),
        Span::styled(
            "新しいログイン情報を入力…",
            if entering_new {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            },
        ),
    ]));

    lines.push(Line::raw(""));
    lines.push(picker_hint("ログイン"));

    let area = centered(frame, 60, dialog_height(&lines, 60));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(dialog("ログイン情報の選択", accent())),
        area,
    );
}
