//! DNS・シンプル監視・シークレットマネージャ・モニタリングスイートの描画。

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, Tabs, Wrap};

use super::{DIM, accent, border_style, error_paragraph, field, format_datetime, placeholder};
use crate::app::{App, ListFocus, Loadable, MonitoringTab};
use crate::monitoring::StorageKind;

/// 一覧テーブルの体裁。中身が無いときの文言と列だけ差し替える。
struct TableSpec {
    title: &'static str,
    /// 読み込み済みで 0 件のときの文言。
    empty: &'static str,
    /// まだ要求していないときの文言。親の選択待ちや、親の取得が失敗した場合に出る。
    /// （ここを「読み込み中」にすると、親が失敗したとき永久に読み込み中に見える）
    idle: &'static str,
    header: Vec<&'static str>,
    widths: Vec<Constraint>,
    focused: bool,
}

/// 一覧をテーブルで描く共通処理。
fn table_pane<T>(
    frame: &mut Frame,
    area: Rect,
    spec: TableSpec,
    data: &Loadable<Vec<T>>,
    rows: Vec<Row<'static>>,
    state: &mut ratatui::widgets::TableState,
) {
    let TableSpec {
        title,
        empty,
        idle,
        header,
        widths,
        focused,
    } = spec;
    let block = Block::bordered()
        .title(Span::styled(
            if rows.is_empty() {
                format!(" {title} ")
            } else {
                format!(" {title} ({}) ", rows.len())
            },
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ))
        .border_style(border_style(focused));

    match data {
        Loadable::Idle => frame.render_widget(placeholder(idle).block(block), area),
        Loadable::Loading => frame.render_widget(placeholder("読み込み中…").block(block), area),
        Loadable::Failed(err) => frame.render_widget(error_paragraph(err).block(block), area),
        Loadable::Ready(_) if rows.is_empty() => {
            frame.render_widget(placeholder(empty).block(block), area)
        }
        Loadable::Ready(_) => {
            let table = Table::new(rows, widths)
                .header(
                    Row::new(header).style(Style::default().fg(DIM).add_modifier(Modifier::BOLD)),
                )
                .row_highlight_style(if focused {
                    Style::default()
                        .fg(accent())
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else {
                    Style::default().add_modifier(Modifier::BOLD)
                })
                .block(block);
            frame.render_stateful_widget(table, area, state);
        }
    }
}

fn dim(text: String) -> Cell<'static> {
    Cell::from(Span::styled(text, Style::default().fg(DIM)))
}

// --- DNS ---

pub fn draw_dns(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    let zones: Vec<(String, usize)> = app
        .visible_dns_zones()
        .into_iter()
        .map(|z| (z.name.clone(), z.records.len()))
        .collect();
    let zone_rows: Vec<Row> = zones
        .iter()
        .map(|(name, count)| Row::new(vec![Cell::from(name.clone()), dim(format!("{count} 件"))]))
        .collect();
    let focus_records = app.dns.focus == ListFocus::Right;
    let data = app.dns.zones.clone();
    table_pane(
        frame,
        chunks[0],
        TableSpec {
            title: "DNSゾーン",
            idle: "読み込み中…",
            empty: "DNSゾーンがありません",
            header: vec!["DNSゾーン", "レコード"],
            widths: vec![Constraint::Min(12), Constraint::Length(9)],
            focused: !focus_records,
        },
        &data,
        zone_rows,
        &mut app.dns.zone_state,
    );

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(6),
        ])
        .split(chunks[2]);

    let records = app.visible_dns_records();
    let record_rows: Vec<Row> = records
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(if r.name.is_empty() {
                    "@".to_string()
                } else {
                    r.name.clone()
                }),
                Cell::from(Span::styled(
                    r.record_type.clone(),
                    Style::default().fg(Color::Cyan),
                )),
                Cell::from(r.data.clone()),
                dim(format!("{}", r.ttl)),
            ])
        })
        .collect();
    // ゾーンが選ばれていればレコードは常に「読み込み済み」扱い。
    let record_data = if app.selected_dns_zone().is_some() {
        Loadable::Ready(records.clone())
    } else {
        Loadable::Idle
    };
    table_pane(
        frame,
        right[0],
        TableSpec {
            title: "レコード",
            idle: "DNSゾーンを選択してください",
            empty: "レコードがありません",
            header: vec!["名前", "種別", "値", "TTL"],
            widths: vec![
                Constraint::Length(16),
                Constraint::Length(7),
                Constraint::Min(16),
                Constraint::Length(6),
            ],
            focused: focus_records,
        },
        &record_data,
        record_rows,
        &mut app.dns.record_state,
    );

    // 委任先のネームサーバーは設定時に必ず要るので、常に見える位置に出す。
    let block = Block::bordered()
        .title(" DNSゾーン情報 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let lines = match app.selected_dns_zone() {
        Some(zone) => {
            let mut lines = vec![field("ID", &zone.id.to_string())];
            if !zone.tags.is_empty() {
                lines.push(field("タグ", &zone.tags.join(", ")));
            }
            if let Some(created) = &zone.created_at {
                lines.push(field("作成日時", &format_datetime(created)));
            }
            if zone.name_servers.is_empty() {
                lines.push(field("ネームサーバー", "（未割り当て）"));
            } else {
                for (i, ns) in zone.name_servers.iter().enumerate() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            super::pad(&format!("NS {}", i + 1), 14),
                            Style::default().fg(DIM),
                        ),
                        Span::styled(ns.clone(), Style::default().fg(Color::Cyan)),
                    ]));
                }
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            "DNSゾーンを選択してください",
            Style::default().fg(DIM),
        ))],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        right[2],
    );
}

// --- シンプル監視 ---

pub fn draw_simple_monitor(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(62),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    let monitors: Vec<_> = app
        .visible_monitors()
        .into_iter()
        .map(|m| (m.summary(), m.enabled, m.delay_loop, m.protocol.clone()))
        .collect();
    let rows: Vec<Row> = monitors
        .iter()
        .map(|(summary, enabled, delay, protocol)| {
            Row::new(vec![
                Cell::from(Span::styled(
                    if *enabled { "有効" } else { "停止" },
                    if *enabled {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(DIM)
                    },
                )),
                Cell::from(summary.clone()),
                dim(protocol.clone()),
                dim(format!("{delay}s")),
            ])
        })
        .collect();
    let data = app.simple_monitor.monitors.clone();
    table_pane(
        frame,
        chunks[0],
        TableSpec {
            title: "シンプル監視",
            idle: "読み込み中…",
            empty: "監視が登録されていません",
            header: vec!["状態", "監視対象", "種別", "間隔"],
            widths: vec![
                Constraint::Length(6),
                Constraint::Min(20),
                Constraint::Length(8),
                Constraint::Length(7),
            ],
            focused: true,
        },
        &data,
        rows,
        &mut app.simple_monitor.monitor_state,
    );

    let block = Block::bordered()
        .title(" 監視の詳細 ")
        .border_style(border_style(false))
        .padding(ratatui::widgets::Padding::horizontal(1));
    let Some(monitor) = app.selected_monitor() else {
        frame.render_widget(
            placeholder("監視を選択してください").block(block),
            chunks[2],
        );
        return;
    };
    let mut lines = vec![
        field("監視対象", &monitor.target),
        field("ID", &monitor.id.to_string()),
        field("プロトコル", &monitor.protocol),
    ];
    if !monitor.port.is_empty() {
        lines.push(field("ポート", &monitor.port));
    }
    if !monitor.path.is_empty() {
        lines.push(field("パス", &monitor.path));
    }
    if !monitor.expected_status.is_empty() {
        lines.push(field("期待ステータス", &monitor.expected_status));
    }
    lines.push(field("監視間隔", &format!("{} 秒", monitor.delay_loop)));
    if monitor.timeout > 0 {
        lines.push(field("タイムアウト", &format!("{} 秒", monitor.timeout)));
    }
    lines.push(field(
        "通知",
        &match (monitor.notify_email, monitor.notify_slack) {
            (true, true) => "メール・Slack".to_string(),
            (true, false) => "メール".to_string(),
            (false, true) => "Slack".to_string(),
            (false, false) => "なし".to_string(),
        },
    ));
    if !monitor.description.is_empty() {
        lines.push(field("説明", &monitor.description));
    }
    if !monitor.tags.is_empty() {
        lines.push(field("タグ", &monitor.tags.join(", ")));
    }
    if let Some(created) = &monitor.created_at {
        lines.push(field("作成日時", &format_datetime(created)));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        chunks[2],
    );
}

// --- シークレットマネージャ ---

pub fn draw_secrets(frame: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(45),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    let vaults = app.visible_vaults();
    let vault_rows: Vec<Row> = vaults
        .iter()
        .map(|v| {
            let note = if !v.description.is_empty() {
                v.description.clone()
            } else if !v.tags.is_empty() {
                v.tags.join(", ")
            } else {
                v.created_at
                    .as_deref()
                    .map(format_datetime)
                    .unwrap_or_default()
            };
            Row::new(vec![
                Cell::from(v.name.clone()),
                dim(note),
                // KMS 鍵を使っているかは Vault ごとに違うので出す。
                dim(if v.kms_key_id.is_empty() {
                    String::new()
                } else {
                    "KMS".to_string()
                }),
            ])
        })
        .collect();
    let focus_secrets = app.secrets.focus == ListFocus::Right;
    let vault_data = app.secrets.vaults.clone();
    table_pane(
        frame,
        chunks[0],
        TableSpec {
            title: "Vault",
            idle: "読み込み中…",
            empty: "Vault がありません",
            header: vec!["名前", "説明 / 作成日時", "暗号鍵"],
            widths: vec![
                Constraint::Min(12),
                Constraint::Min(10),
                Constraint::Length(7),
            ],
            focused: !focus_secrets,
        },
        &vault_data,
        vault_rows,
        &mut app.secrets.vault_state,
    );

    let secrets = app.visible_secrets();
    let secret_rows: Vec<Row> = secrets
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|s| {
                    Row::new(vec![
                        Cell::from(s.name.clone()),
                        dim(s
                            .latest_version
                            .map(|v| format!("v{v}"))
                            .unwrap_or_default()),
                        // 値は一覧に出さない。u キーで明示的に取得する。
                        Cell::from(Span::styled("••••••", Style::default().fg(DIM))),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        chunks[2],
        TableSpec {
            title: "シークレット",
            idle: "Vault を選択してください",
            empty: "シークレットがありません",
            header: vec!["名前", "最新版", "値"],
            widths: vec![
                Constraint::Min(14),
                Constraint::Length(8),
                Constraint::Length(8),
            ],
            focused: focus_secrets,
        },
        &secrets,
        secret_rows,
        &mut app.secrets.secret_state,
    );
}

// --- モニタリングスイート ---

pub fn draw_monitoring(frame: &mut Frame, area: Rect, app: &mut App) {
    // 保管先はプロジェクトに紐づかない。選択した保管先のアクセスキーを右に出す。
    if app.monitoring.tab == MonitoringTab::Storages {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(area);
        draw_monitoring_tabs(frame, rows[0], app);
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(55),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(rows[1]);
        draw_storages(frame, panes[0], app);
        draw_storage_access_keys(frame, panes[2], app);
        return;
    }
    if matches!(
        app.monitoring.tab,
        MonitoringTab::LogRoutings | MonitoringTab::MetricsRoutings | MonitoringTab::Dashboards
    ) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(1)])
            .split(area);
        draw_monitoring_tabs(frame, rows[0], app);
        match app.monitoring.tab {
            MonitoringTab::LogRoutings => draw_log_routings(frame, rows[1], app),
            MonitoringTab::MetricsRoutings => draw_metrics_routings(frame, rows[1], app),
            MonitoringTab::Dashboards => draw_dashboards(frame, rows[1], app),
            _ => unreachable!(),
        }
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    let projects = app.visible_projects();
    let project_rows: Vec<Row> = projects
        .iter()
        .map(|p| {
            let note = if p.description.is_empty() {
                p.tags.join(", ")
            } else {
                p.description.clone()
            };
            Row::new(vec![
                Cell::from(p.name.clone()),
                dim(note),
                dim(p
                    .created_at
                    .as_deref()
                    .map(format_datetime)
                    .unwrap_or_default()),
            ])
        })
        .collect();
    let project_data = app
        .monitoring
        .projects
        .get(&app.zone)
        .cloned()
        .unwrap_or(Loadable::Idle);
    table_pane(
        frame,
        chunks[0],
        TableSpec {
            title: "プロジェクト",
            idle: "読み込み中…",
            empty: "プロジェクトがありません（z でゾーンを切り替え）",
            header: vec!["名前", "説明 / タグ", "作成日時"],
            widths: vec![
                Constraint::Min(10),
                Constraint::Min(8),
                Constraint::Length(17),
            ],
            focused: app.monitoring.focus == ListFocus::Left,
        },
        &project_data,
        project_rows,
        &mut app.monitoring.project_state,
    );

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(chunks[2]);
    draw_monitoring_tabs(frame, right[0], app);

    match app.monitoring.tab {
        MonitoringTab::Rules => draw_rules(frame, right[1], app),
        MonitoringTab::Histories => draw_histories(frame, right[1], app),
        MonitoringTab::NotificationTargets => draw_notification_targets(frame, right[1], app),
        MonitoringTab::NotificationRoutings => draw_notification_routings(frame, right[1], app),
        MonitoringTab::LogMeasureRules => draw_log_measure_rules(frame, right[1], app),
        MonitoringTab::LogRoutings => draw_log_routings(frame, right[1], app),
        MonitoringTab::MetricsRoutings => draw_metrics_routings(frame, right[1], app),
        MonitoringTab::Dashboards => draw_dashboards(frame, right[1], app),
        MonitoringTab::Storages => {}
    }
}

fn draw_monitoring_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = MonitoringTab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| Line::from(format!("{} {}", i + 1, tab.title())))
        .collect();
    let selected = MonitoringTab::ALL
        .iter()
        .position(|t| *t == app.monitoring.tab)
        .unwrap_or(0);
    frame.render_widget(
        Tabs::new(titles)
            .select(selected)
            .highlight_style(Style::default().fg(accent()).add_modifier(Modifier::BOLD))
            .divider(Span::styled("│", Style::default().fg(DIM)))
            .block(Block::default().padding(ratatui::widgets::Padding::horizontal(1))),
        area,
    );
}

fn draw_rules(frame: &mut Frame, area: Rect, app: &mut App) {
    let rules = app.visible_rules();
    let rows: Vec<Row> = rules
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|r| {
                    // 発報中のルールが一目で分かるようにする。
                    let state = if r.open { "発報中" } else { "正常" };
                    let style = if r.open {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Green)
                    };
                    let thresholds = [
                        r.warning_enabled
                            .then(|| format!("W:{}", r.threshold_warning)),
                        r.critical_enabled
                            .then(|| format!("C:{}", r.threshold_critical)),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                    Row::new(vec![
                        Cell::from(Span::styled(state, style)),
                        Cell::from(r.name.clone()),
                        dim(thresholds),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "アラートルール",
            idle: "プロジェクトを選択してください",
            empty: "ルールがありません",
            header: vec!["状態", "名前", "しきい値"],
            widths: vec![
                Constraint::Length(8),
                Constraint::Min(16),
                Constraint::Length(20),
            ],
            focused: app.monitoring.focus == ListFocus::Left,
        },
        &rules,
        rows,
        &mut app.monitoring.rule_state,
    );
}

fn draw_histories(frame: &mut Frame, area: Rect, app: &mut App) {
    let histories = app.visible_histories();
    let rows: Vec<Row> = histories
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|h| {
                    let severity_style = match h.severity.as_str() {
                        "critical" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        "warning" => Style::default().fg(Color::Yellow),
                        _ => Style::default().fg(DIM),
                    };
                    Row::new(vec![
                        Cell::from(Span::styled(h.severity.clone(), severity_style)),
                        Cell::from(Span::styled(
                            if h.open { "継続中" } else { "復旧" },
                            if h.open {
                                Style::default().fg(Color::Red)
                            } else {
                                Style::default().fg(Color::Green)
                            },
                        )),
                        dim(format_datetime(&h.starts_at)),
                        Cell::from(h.labels.clone()),
                        dim(h
                            .value
                            .map(|v| format!("{v} / {}", h.threshold))
                            .unwrap_or_default()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "発報履歴",
            idle: "プロジェクトを選択してください",
            empty: "履歴がありません",
            header: vec!["深刻度", "状態", "発生", "ラベル", "値/しきい値"],
            widths: vec![
                Constraint::Length(9),
                Constraint::Length(7),
                Constraint::Length(17),
                Constraint::Min(12),
                Constraint::Length(16),
            ],
            focused: app.monitoring.focus == ListFocus::Right,
        },
        &histories,
        rows,
        &mut app.monitoring.history_state,
    );
}

fn draw_notification_targets(frame: &mut Frame, area: Rect, app: &mut App) {
    let targets = app.visible_notification_targets();
    let rows: Vec<Row> = targets
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|target| {
                    let service = match target.service_type.as_str() {
                        "SAKURA_SIMPLE_NOTICE" => "シンプル通知",
                        "SAKURA_EVENT_BUS" => "EventBus",
                        other => other,
                    };
                    Row::new(vec![
                        Cell::from(service.to_string()),
                        Cell::from(target.description.clone()),
                        dim(if target.url.is_empty() {
                            target.uid.clone()
                        } else {
                            target.url.clone()
                        }),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "通知先",
            idle: "プロジェクトを選択してください",
            empty: "通知先がありません",
            header: vec!["サービス", "説明", "URL / UID"],
            widths: vec![
                Constraint::Length(16),
                Constraint::Min(14),
                Constraint::Min(20),
            ],
            focused: app.monitoring.focus == ListFocus::Right,
        },
        &targets,
        rows,
        &mut app.monitoring.notification_target_state,
    );
}

fn draw_log_measure_rules(frame: &mut Frame, area: Rect, app: &mut App) {
    let rules = app.visible_log_measure_rules();
    let rows: Vec<Row> = rules
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|rule| {
                    let matchers = rule
                        .rule
                        .pointer("/query/matchers")
                        .and_then(serde_json::Value::as_array)
                        .map_or(0, Vec::len);
                    Row::new(vec![
                        Cell::from(rule.name.clone()),
                        dim(rule.description.clone()),
                        dim(format!(
                            "{} → {}",
                            rule.log_storage_id, rule.metrics_storage_id
                        )),
                        dim(format!("{matchers} 条件")),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "ログ計測ルール",
            idle: "プロジェクトを選択してください",
            empty: "ログ計測ルールがありません",
            header: vec!["名前", "説明", "ログ → メトリクス", "条件"],
            widths: vec![
                Constraint::Min(14),
                Constraint::Min(12),
                Constraint::Length(29),
                Constraint::Length(8),
            ],
            focused: app.monitoring.focus == ListFocus::Right,
        },
        &rules,
        rows,
        &mut app.monitoring.log_measure_rule_state,
    );
}

fn draw_notification_routings(frame: &mut Frame, area: Rect, app: &mut App) {
    let routings = app.visible_notification_routings();
    let rows: Vec<Row> = routings
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|routing| {
                    let service = match routing.target_service_type.as_str() {
                        "SAKURA_SIMPLE_NOTICE" => "シンプル通知",
                        "SAKURA_EVENT_BUS" => "EventBus",
                        other => other,
                    };
                    let labels = routing
                        .match_labels
                        .iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    Row::new(vec![
                        dim(routing.order.to_string()),
                        Cell::from(if routing.target_description.is_empty() {
                            service.to_string()
                        } else {
                            routing.target_description.clone()
                        }),
                        dim(routing
                            .resend_interval_minutes
                            .map(|minutes| format!("{minutes} 分"))
                            .unwrap_or_else(|| "-".to_string())),
                        dim(if labels.is_empty() {
                            "全アラート".to_string()
                        } else {
                            labels
                        }),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "通知経路",
            idle: "プロジェクトを選択してください",
            empty: "通知経路がありません",
            header: vec!["順序", "通知先", "再送", "条件"],
            widths: vec![
                Constraint::Length(5),
                Constraint::Min(14),
                Constraint::Length(9),
                Constraint::Min(18),
            ],
            focused: app.monitoring.focus == ListFocus::Right,
        },
        &routings,
        rows,
        &mut app.monitoring.notification_routing_state,
    );
}

fn draw_log_routings(frame: &mut Frame, area: Rect, app: &mut App) {
    let routings = app.visible_log_routings();
    let rows: Vec<Row> = routings
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|routing| {
                    Row::new(vec![
                        Cell::from(routing.publisher_code.clone()),
                        dim(routing.publisher_description.clone()),
                        Cell::from(routing.variant.clone()),
                        dim(routing
                            .resource_id
                            .map_or_else(|| "-".to_string(), |id| id.to_string())),
                        dim(routing.log_storage_id.to_string()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "ログ転送",
            idle: "読み込み中…",
            empty: "ログ転送設定がありません",
            header: vec![
                "パブリッシャー",
                "説明",
                "バリアント",
                "リソースID",
                "保管先ID",
            ],
            widths: vec![
                Constraint::Min(14),
                Constraint::Min(14),
                Constraint::Min(10),
                Constraint::Length(14),
                Constraint::Length(14),
            ],
            focused: app.monitoring.focus == ListFocus::Right,
        },
        &routings,
        rows,
        &mut app.monitoring.log_routing_state,
    );
}

fn draw_metrics_routings(frame: &mut Frame, area: Rect, app: &mut App) {
    let items = app.visible_metrics_routings();
    let rows = items
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|r| {
                    Row::new(vec![
                        Cell::from(r.publisher_code.clone()),
                        dim(r.publisher_description.clone()),
                        Cell::from(r.variant.clone()),
                        dim(r
                            .resource_id
                            .map_or_else(|| "-".to_string(), |id| id.to_string())),
                        dim(r.metrics_storage_id.to_string()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "メトリクス転送",
            idle: "読み込み中…",
            empty: "メトリクス転送設定がありません",
            header: vec![
                "パブリッシャー",
                "説明",
                "バリアント",
                "リソースID",
                "保管先ID",
            ],
            widths: vec![
                Constraint::Min(14),
                Constraint::Min(14),
                Constraint::Min(10),
                Constraint::Length(14),
                Constraint::Length(14),
            ],
            focused: true,
        },
        &items,
        rows,
        &mut app.monitoring.metrics_routing_state,
    );
}

fn draw_dashboards(frame: &mut Frame, area: Rect, app: &mut App) {
    let items = app.visible_dashboard_projects();
    let rows = items
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|p| {
                    Row::new(vec![
                        Cell::from(p.name.clone()),
                        dim(p.description.clone()),
                        dim(p.tags.join(", ")),
                        dim(p.resource_id.to_string()),
                        dim(format_datetime(&p.created_at)),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "ダッシュボードプロジェクト",
            idle: "読み込み中…",
            empty: "ダッシュボードプロジェクトがありません",
            header: vec!["名前", "説明", "タグ", "リソースID", "作成日時"],
            widths: vec![
                Constraint::Min(16),
                Constraint::Min(16),
                Constraint::Min(10),
                Constraint::Length(14),
                Constraint::Length(17),
            ],
            focused: true,
        },
        &items,
        rows,
        &mut app.monitoring.dashboard_state,
    );
}

fn draw_storages(frame: &mut Frame, area: Rect, app: &mut App) {
    let storages = app.visible_storages();
    let rows: Vec<Row> = storages
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|s| {
                    let kind_style = match s.kind {
                        StorageKind::Logs => Style::default().fg(Color::Cyan),
                        StorageKind::Metrics => Style::default().fg(Color::Magenta),
                        StorageKind::Traces => Style::default().fg(Color::Blue),
                    };
                    Row::new(vec![
                        Cell::from(Span::styled(s.kind.label(), kind_style)),
                        Cell::from(s.name.clone()),
                        dim(format!(
                            "{}/{}",
                            if s.is_system { "system" } else { "user" },
                            if s.classification.is_empty() {
                                "-"
                            } else {
                                &s.classification
                            }
                        )),
                        dim(s
                            .retention_days
                            .map(|d| format!("{d} 日"))
                            .unwrap_or_default()),
                        dim(if s.description.is_empty() {
                            format!("#{}", s.id)
                        } else {
                            s.description.clone()
                        }),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "保管先",
            idle: "読み込み中…",
            empty: "保管先がありません",
            header: vec!["種別", "名前", "領域/プラン", "保持期間", "説明"],
            widths: vec![
                Constraint::Length(18),
                Constraint::Min(14),
                Constraint::Length(11),
                Constraint::Length(9),
                Constraint::Min(10),
            ],
            focused: app.monitoring.focus == ListFocus::Right,
        },
        &storages,
        rows,
        &mut app.monitoring.storage_state,
    );
}

fn draw_storage_access_keys(frame: &mut Frame, area: Rect, app: &mut App) {
    let system_storage = app
        .selected_storage()
        .is_some_and(|storage| storage.is_system);
    let keys = app.visible_storage_access_keys();
    let rows: Vec<Row> = keys
        .ready()
        .map(|items| {
            items
                .iter()
                .map(|key| {
                    Row::new(vec![
                        Cell::from(key.token.clone()),
                        dim(if key.description.is_empty() {
                            key.uid.clone()
                        } else {
                            key.description.clone()
                        }),
                        Cell::from(Span::styled("••••••", Style::default().fg(DIM))),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();
    table_pane(
        frame,
        area,
        TableSpec {
            title: "アクセスキー",
            idle: "ストレージを選択してください",
            empty: if system_storage {
                "システム領域ではアクセスキーを利用できません"
            } else {
                "アクセスキーがありません"
            },
            header: vec!["トークン", "説明 / UID", "シークレット"],
            widths: vec![
                Constraint::Min(12),
                Constraint::Min(12),
                Constraint::Length(10),
            ],
            focused: app.monitoring.focus == ListFocus::Right,
        },
        &keys,
        rows,
        &mut app.monitoring.storage_key_state,
    );
}
