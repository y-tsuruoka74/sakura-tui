//! 右ペイン: 選択中レジストリの概要・ユーザー・イメージ。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph, Wrap};

use super::{DIM, SAKURA, border_style, field, format_datetime, placeholder};
use crate::app::{App, Focus, ImagePane, Loadable, Tab};
use crate::sacloud::ContainerRegistry;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(registry) = app.selected_registry().cloned() else {
        frame.render_widget(
            Paragraph::new("レジストリを選択してください")
                .style(Style::default().fg(DIM))
                .block(Block::bordered().border_style(border_style(false))),
            area,
        );
        return;
    };

    match app.registry.tab {
        Tab::Overview => draw_overview(frame, area, app, &registry),
        Tab::Users => draw_users(frame, area, app, &registry),
        Tab::Images => draw_images(frame, area, app, &registry),
    }
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &App, registry: &ContainerRegistry) {
    let host = registry.host();
    let access_level = match registry.access_level.as_str() {
        "readonly" => "readonly (pull は認証不要 / 廃止予定)",
        "none" | "" => "none (非公開)",
        other => other,
    };

    let mut lines = vec![
        field("名前", &registry.name),
        field("ID", &registry.id.to_string()),
        field("ホスト", host),
        field("サブドメイン", &registry.subdomain_label),
    ];
    if !registry.virtual_domain.is_empty() {
        lines.push(field("独自ドメイン", &registry.virtual_domain));
        lines.push(field("既定FQDN", &registry.fqdn));
    }
    lines.push(field("公開設定", access_level));
    lines.push(field("状態", &registry.availability));
    if !registry.description.is_empty() {
        lines.push(field("説明", &registry.description));
    }
    if !registry.tags.is_empty() {
        lines.push(field("タグ", &registry.tags.join(", ")));
    }
    if let Some(created) = &registry.created_at {
        lines.push(field("作成日時", &format_datetime(created)));
    }
    if let Some(modified) = &registry.modified_at {
        lines.push(field("更新日時", &format_datetime(modified)));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "docker コマンド",
        Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
    )));
    if host.is_empty() {
        lines.push(Line::from(Span::styled(
            "  ホスト名が未割り当てです",
            Style::default().fg(DIM),
        )));
    } else {
        for command in [
            format!("docker login {host}"),
            format!("docker tag <image> {host}/<repo>:<tag>"),
            format!("docker push {host}/<repo>:<tag>"),
        ] {
            lines.push(Line::from(Span::styled(
                format!("  {command}"),
                Style::default().fg(Color::Cyan),
            )));
        }
    }

    let focused = app.registry.focus == Focus::Detail;
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::bordered()
                .title(" 概要 ")
                .border_style(border_style(focused))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        area,
    );
}

fn draw_users(frame: &mut Frame, area: Rect, app: &mut App, _registry: &ContainerRegistry) {
    let focused = app.registry.focus == Focus::Detail;
    let users = app.visible_users();
    let block = Block::bordered()
        .title(" ユーザー ")
        .border_style(border_style(focused))
        .padding(ratatui::widgets::Padding::horizontal(1));

    match &users {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area)
        }
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(users) if users.is_empty() => frame.render_widget(
            placeholder("ユーザーが登録されていません（a キーで追加）").block(block),
            area,
        ),
        Loadable::Ready(users) => {
            let items: Vec<ListItem> = users
                .iter()
                .map(|user| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:<24}", user.username),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            user.permission.description().to_string(),
                            Style::default().fg(DIM),
                        ),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(block)
                .highlight_style(highlight_style(focused))
                .highlight_symbol("▌");
            frame.render_stateful_widget(list, area, &mut app.registry.user_state);
        }
    }
}

fn draw_images(frame: &mut Frame, area: Rect, app: &mut App, registry: &ContainerRegistry) {
    let host = registry.host().to_string();

    if !app.is_logged_in() {
        let lines = vec![
            Line::from(Span::styled(
                "レジストリにログインしていません",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::raw("イメージ一覧は "),
                Span::styled(&host, Style::default().fg(Color::Cyan)),
                Span::raw(" の Docker Registry API から取得します。"),
            ]),
            Line::raw("クラウドAPIのトークンとは別に、レジストリユーザーの認証が必要です。"),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    "L",
                    Style::default().fg(SAKURA).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" キーでログインしてください。"),
            ]),
        ];
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::bordered()
                    .title(" イメージ ")
                    .border_style(border_style(app.registry.focus == Focus::Detail))
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            ),
            area,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_repositories(frame, chunks[0], app);

    // タグ一覧と、選択中タグの詳細を縦に並べる。
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(9)])
        .split(chunks[1]);
    draw_tags(frame, right[0], app);
    draw_tag_detail(frame, right[1], app);
}

/// 選択中タグのサイズ・レイヤ数・プラットフォーム・ビルド日時。
fn draw_tag_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::bordered()
        .title(" タグの詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));

    let detail = app.selected_tag_detail();
    let lines = match &detail {
        Loadable::Idle => {
            frame.render_widget(placeholder("タグを選択してください").block(block), area);
            return;
        }
        Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area);
            return;
        }
        Loadable::Failed(err) => {
            frame.render_widget(error_paragraph(err).block(block), area);
            return;
        }
        Loadable::Ready(detail) => {
            let mut lines = Vec::new();
            if let Some(size) = detail.size {
                lines.push(field("サイズ", &format_bytes(size)));
            }
            if let Some(layers) = detail.layers {
                lines.push(field("レイヤ数", &layers.to_string()));
            }
            if !detail.platforms.is_empty() {
                let platforms: Vec<String> =
                    detail.platforms.iter().map(ToString::to_string).collect();
                lines.push(field("プラットフォーム", &platforms.join(", ")));
            }
            if let Some(created) = &detail.created {
                lines.push(field("ビルド日時", &format_datetime(created)));
            }
            if !detail.media_type.is_empty() {
                lines.push(field("形式", short_media_type(&detail.media_type)));
            }
            if let Some(digest) = &detail.digest {
                lines.push(field("ダイジェスト", digest));
            }
            lines
        }
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

/// バイト数を人が読める単位にする。
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {} ({bytes} B)", UNITS[unit])
    }
}

/// `application/vnd.oci.image.index.v1+json` のような長い名前を短く言い換える。
fn short_media_type(media_type: &str) -> &str {
    match media_type {
        t if t.contains("index") || t.contains("manifest.list") => "マルチアーキテクチャ (index)",
        t if t.contains("oci.image.manifest") => "OCI イメージマニフェスト",
        t if t.contains("docker.distribution.manifest.v2") => "Docker マニフェスト v2",
        other => other,
    }
}

fn draw_repositories(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.registry.focus == Focus::Detail && app.registry.image_pane == ImagePane::Repositories;
    let repositories = app.visible_repositories();
    let count = repositories.ready().map_or(0, Vec::len);
    let block = Block::bordered()
        .title(if count > 0 {
            format!(" リポジトリ ({count}) ")
        } else {
            " リポジトリ ".to_string()
        })
        .border_style(border_style(focused));

    match &repositories {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area)
        }
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(repos) if repos.is_empty() => frame.render_widget(
            placeholder("イメージがまだ push されていません").block(block),
            area,
        ),
        Loadable::Ready(repos) => {
            let items: Vec<ListItem> = repos.iter().map(|r| ListItem::new(r.as_str())).collect();
            let list = List::new(items)
                .block(block)
                .highlight_style(highlight_style(focused))
                .highlight_symbol("▌");
            frame.render_stateful_widget(list, area, &mut app.registry.repository_state);
        }
    }
}

fn draw_tags(frame: &mut Frame, area: Rect, app: &mut App) {
    let focused = app.registry.focus == Focus::Detail && app.registry.image_pane == ImagePane::Tags;
    let repository = app.selected_repository();
    let tags = app.visible_tags();
    let block = Block::bordered()
        .title(match &repository {
            Some(repository) => format!(" タグ: {repository} "),
            None => " タグ ".to_string(),
        })
        .border_style(border_style(focused))
        .padding(ratatui::widgets::Padding::horizontal(1));

    if repository.is_none() {
        frame.render_widget(placeholder("リポジトリを選択してください").block(block), area);
        return;
    }

    match &tags {
        Loadable::Idle | Loadable::Loading => {
            frame.render_widget(placeholder("読み込み中…").block(block), area)
        }
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(tags) if tags.is_empty() => {
            frame.render_widget(placeholder("タグがありません").block(block), area)
        }
        Loadable::Ready(tags) => {
            let items: Vec<ListItem> = tags
                .iter()
                .map(|tag| {
                    let digest = tag
                        .digest
                        .as_deref()
                        // sha256:xxxx… は長いので先頭 12 桁だけ見せる。
                        .map(|d| d.split_once(':').map_or(d, |(_, hex)| hex))
                        .map(|hex| hex.chars().take(12).collect::<String>())
                        .unwrap_or_default();
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:<28}", tag.name),
                            Style::default().add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(digest, Style::default().fg(DIM)),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(block)
                .highlight_style(highlight_style(focused))
                .highlight_symbol("▌");
            frame.render_stateful_widget(list, area, &mut app.registry.tag_state);
        }
    }
}

fn error_paragraph(err: &str) -> Paragraph<'static> {
    Paragraph::new(err.to_string())
        .style(Style::default().fg(Color::Red))
        .wrap(Wrap { trim: false })
}

fn highlight_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(SAKURA).add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}
