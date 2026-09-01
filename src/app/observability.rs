//! DNS・シンプル監視・シークレットマネージャ・モニタリングスイートの状態と操作。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    App, ConfirmAction, DnsRecordForm, DnsRecordFormMode, DnsZoneForm, DnsZoneFormMode, Loadable,
    Message, Overlay, Pane, SecretForm, SecretFormMode, SimpleMonitorForm, SimpleMonitorFormMode,
    StatusKind, VaultForm, VaultFormMode, fmt_error, matches,
};
use crate::commonservice::{DnsRecord, DnsZone, SimpleMonitor, SimpleMonitorInput};
use crate::monitoring::{
    AlertHistory, AlertProject, AlertRule, DashboardProject, LogMeasureRule, LogRouting,
    MetricsRouting, NotificationRouting, NotificationTarget, Publisher, Storage, StorageAccessKey,
    StorageKind,
};
use crate::secretmanager::{Secret, Vault};

/// 左右に並んだ一覧の、どちらを操作しているか。
///
/// 「右に選択があるかどうか」で判定すると、いちど右を選んだあと
/// 左に戻れなくなる（選択は自動で入るため）。明示的に持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListFocus {
    #[default]
    Left,
    Right,
}

/// DNS 画面（ゾーン → レコード）。
#[derive(Debug, Default)]
pub struct DnsView {
    pub zones: Loadable<Vec<DnsZone>>,
    pub zone_state: TableState,
    pub record_state: TableState,
    pub focus: ListFocus,
    /// 更新後の再取得で同じゾーンを選び直すための名前。
    pub reselect_zone: Option<String>,
}

/// シンプル監視画面。
#[derive(Debug, Default)]
pub struct SimpleMonitorView {
    pub monitors: Loadable<Vec<SimpleMonitor>>,
    pub monitor_state: TableState,
}

/// シークレットマネージャ画面（Vault → シークレット）。
#[derive(Debug, Default)]
pub struct SecretsView {
    /// Vault はグローバルリソース。
    pub vaults: Loadable<Vec<Vault>>,
    pub vault_state: TableState,
    pub focus: ListFocus,
    /// Vault ID をキーにしたシークレット一覧。値は含まない。
    pub secrets: HashMap<String, Loadable<Vec<Secret>>>,
    pub secret_state: TableState,
    /// 更新後に同じ Vault を選び直す。
    pub reselect_vault: Option<String>,
}

/// モニタリングスイート画面のタブ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MonitoringTab {
    #[default]
    Rules,
    Histories,
    Storages,
    NotificationTargets,
    NotificationRoutings,
    LogMeasureRules,
    LogRoutings,
    MetricsRoutings,
    Dashboards,
}

impl MonitoringTab {
    pub const ALL: [MonitoringTab; 9] = [
        MonitoringTab::Rules,
        MonitoringTab::Histories,
        MonitoringTab::Storages,
        MonitoringTab::NotificationTargets,
        MonitoringTab::NotificationRoutings,
        MonitoringTab::LogMeasureRules,
        MonitoringTab::LogRoutings,
        MonitoringTab::MetricsRoutings,
        MonitoringTab::Dashboards,
    ];

    pub fn title(self) -> &'static str {
        match self {
            MonitoringTab::Rules => "ルール",
            MonitoringTab::Histories => "履歴",
            MonitoringTab::Storages => "保管先",
            MonitoringTab::NotificationTargets => "通知先",
            MonitoringTab::NotificationRoutings => "通知経路",
            MonitoringTab::LogMeasureRules => "ログ計測",
            MonitoringTab::LogRoutings => "ログ転送",
            MonitoringTab::MetricsRoutings => "メトリクス転送",
            MonitoringTab::Dashboards => "ダッシュボード",
        }
    }
}

/// モニタリングスイート画面。
#[derive(Debug, Default)]
pub struct MonitoringView {
    pub projects: HashMap<String, Loadable<Vec<AlertProject>>>,
    pub project_state: TableState,
    pub focus: ListFocus,
    pub tab: MonitoringTab,
    /// `(ゾーン, プロジェクトID)` をキーにしたルール・履歴。
    pub rules: HashMap<(String, i64), Loadable<Vec<AlertRule>>>,
    pub rule_state: TableState,
    pub log_measure_rules: HashMap<(String, i64), Loadable<Vec<LogMeasureRule>>>,
    pub log_measure_rule_state: TableState,
    pub log_routings: HashMap<String, Loadable<Vec<LogRouting>>>,
    pub log_routing_state: TableState,
    pub metrics_routings: HashMap<String, Loadable<Vec<MetricsRouting>>>,
    pub metrics_routing_state: TableState,
    pub publishers: HashMap<String, Loadable<Vec<Publisher>>>,
    pub dashboard_projects: HashMap<String, Loadable<Vec<DashboardProject>>>,
    pub dashboard_state: TableState,
    pub histories: HashMap<(String, i64), Loadable<Vec<AlertHistory>>>,
    pub history_state: TableState,
    pub notification_targets: HashMap<(String, i64), Loadable<Vec<NotificationTarget>>>,
    pub notification_target_state: TableState,
    pub notification_routings: HashMap<(String, i64), Loadable<Vec<NotificationRouting>>>,
    pub notification_routing_state: TableState,
    pub storages: HashMap<String, Loadable<Vec<Storage>>>,
    pub storage_state: TableState,
    /// `(ゾーン, 種別, ストレージresource_id)` ごとのアクセスキー一覧。
    pub storage_keys: HashMap<(String, StorageKind, i64), Loadable<Vec<StorageAccessKey>>>,
    pub storage_key_state: TableState,
    pub reselect_project: Option<i64>,
}

impl App {
    // --- DNS ---

    pub fn visible_dns_zones(&self) -> Vec<&DnsZone> {
        let Some(items) = self.dns.zones.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::DnsZones);
        items
            .iter()
            .filter(|z| matches(filter, &[&z.name, &z.description]))
            .collect()
    }

    pub fn selected_dns_zone(&self) -> Option<&DnsZone> {
        let index = self.dns.zone_state.selected()?;
        self.visible_dns_zones().into_iter().nth(index)
    }

    pub fn visible_dns_records(&self) -> Vec<DnsRecord> {
        let Some(zone) = self.selected_dns_zone() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::DnsRecords);
        zone.records
            .iter()
            .filter(|r| matches(filter, &[&r.name, &r.record_type, &r.data]))
            .cloned()
            .collect()
    }

    pub fn selected_dns_record(&self) -> Option<DnsRecord> {
        let index = self.dns.record_state.selected()?;
        self.visible_dns_records().get(index).cloned()
    }

    // --- シンプル監視 ---

    pub fn visible_monitors(&self) -> Vec<&SimpleMonitor> {
        let Some(items) = self.simple_monitor.monitors.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Monitors);
        items
            .iter()
            .filter(|m| {
                let summary = m.summary();
                matches(filter, &[&m.target, &summary, &m.description])
            })
            .collect()
    }

    pub fn selected_monitor(&self) -> Option<&SimpleMonitor> {
        let index = self.simple_monitor.monitor_state.selected()?;
        self.visible_monitors().into_iter().nth(index)
    }

    pub(super) fn on_key_simple_monitor(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => self.open_create_simple_monitor(),
            KeyCode::Char('E') => self.open_edit_simple_monitor(),
            KeyCode::Char('D') => self.confirm_delete_simple_monitor(),
            KeyCode::Char('t') => self.toggle_simple_monitor(),
            _ => {}
        }
    }

    fn open_create_simple_monitor(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::SimpleMonitorForm(SimpleMonitorForm {
            mode: SimpleMonitorFormMode::Create,
            target_monitor: None,
            target: String::new(),
            description: String::new(),
            protocol: 0,
            port: String::new(),
            path: "/".to_string(),
            expected_status: "200".to_string(),
            delay_loop: "60".to_string(),
            timeout: "10".to_string(),
            enabled: true,
            notify_email: true,
            field: 0,
        }));
    }

    fn open_edit_simple_monitor(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(monitor) = self.selected_monitor().cloned() else {
            self.set_status("編集する監視を選択してください", StatusKind::Info);
            return;
        };
        let Some(protocol) = SimpleMonitorForm::PROTOCOLS
            .iter()
            .position(|protocol| *protocol == monitor.protocol)
        else {
            self.set_status(
                format!(
                    "監視方式 {} の編集にはまだ対応していません",
                    monitor.protocol
                ),
                StatusKind::Info,
            );
            return;
        };
        self.overlay = Some(Overlay::SimpleMonitorForm(SimpleMonitorForm {
            mode: SimpleMonitorFormMode::Edit,
            target_monitor: Some(monitor.clone()),
            target: monitor.target,
            description: monitor.description,
            protocol,
            port: monitor.port,
            path: monitor.path,
            expected_status: monitor.expected_status,
            delay_loop: monitor.delay_loop.to_string(),
            timeout: monitor.timeout.to_string(),
            enabled: monitor.enabled,
            notify_email: monitor.notify_email,
            field: 1,
        }));
    }

    pub(super) fn submit_simple_monitor_form(&mut self, form: SimpleMonitorForm) {
        let input = match simple_monitor_input(&form) {
            Ok(input) => input,
            Err(message) => {
                self.set_status(message, StatusKind::Error);
                self.overlay = Some(Overlay::SimpleMonitorForm(form));
                return;
            }
        };
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let (label, target) = match form.mode {
            SimpleMonitorFormMode::Create => {
                let label = format!("シンプル監視「{}」を作成", input.target);
                (label, None)
            }
            SimpleMonitorFormMode::Edit => {
                let Some(target) = form.target_monitor else {
                    return;
                };
                let label = format!("シンプル監視「{}」を更新", target.target);
                (label, Some(target))
            }
        };
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = match target {
                Some(monitor) => client.update_simple_monitor(&monitor, &input).await,
                None => client.create_simple_monitor(&input).await,
            }
            .map_err(fmt_error);
            let _ = tx.send(Message::SimpleMonitorAction { label, result });
        });
    }

    fn toggle_simple_monitor(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(monitor) = self.selected_monitor().cloned() else {
            return;
        };
        let enabled = !monitor.enabled;
        let state = if enabled { "有効化" } else { "停止" };
        let label = format!("シンプル監視「{}」を{state}", monitor.target);
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .set_simple_monitor_enabled(&monitor, enabled)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::SimpleMonitorAction { label, result });
        });
    }

    fn confirm_delete_simple_monitor(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(monitor) = self.selected_monitor() else {
            self.set_status("削除する監視を選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "シンプル監視の削除".to_string(),
            body: format!(
                "監視対象「{}」のシンプル監視を削除します。\nこの操作は取り消せません。実行しますか？",
                monitor.target
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteSimpleMonitor {
                id: monitor.id,
                target: monitor.target.clone(),
            },
        });
    }

    pub(super) fn run_delete_simple_monitor(
        &mut self,
        id: crate::sacloud::ResourceId,
        target: String,
    ) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!("シンプル監視「{target}」を削除");
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client.delete_simple_monitor(id).await.map_err(fmt_error);
            let _ = tx.send(Message::SimpleMonitorAction { label, result });
        });
    }

    // --- シークレットマネージャ ---

    pub fn visible_vaults(&self) -> Vec<Vault> {
        let Some(items) = self.secrets.vaults.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Vaults);
        items
            .iter()
            .filter(|v| matches(filter, &[&v.name, &v.description]))
            .cloned()
            .collect()
    }

    pub fn selected_vault(&self) -> Option<Vault> {
        let index = self.secrets.vault_state.selected()?;
        self.visible_vaults().into_iter().nth(index)
    }

    pub fn visible_secrets(&self) -> Loadable<Vec<Secret>> {
        let Some(vault) = self.selected_vault() else {
            return Loadable::Idle;
        };
        let loadable = self
            .secrets
            .secrets
            .get(&vault.id)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|s| matches(self.filters.get(Pane::Secrets), &[&s.name]))
                .collect(),
        )
    }

    pub fn selected_secret(&self) -> Option<Secret> {
        let index = self.secrets.secret_state.selected()?;
        self.visible_secrets().ready()?.get(index).cloned()
    }

    // --- モニタリングスイート ---

    // --- 読み込み ---

    pub(super) fn dns_ensure_loaded(&mut self) {
        if self.dns.zones.is_idle() {
            self.dns.zones = Loadable::Loading;
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.list_dns_zones().await.map_err(fmt_error);
                let _ = tx.send(Message::DnsZones(result));
            });
            return;
        }
        self.fill_selection(Pane::DnsZones);
        self.fill_selection(Pane::DnsRecords);
    }

    pub(super) fn monitor_ensure_loaded(&mut self) {
        if self.simple_monitor.monitors.is_idle() {
            self.simple_monitor.monitors = Loadable::Loading;
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.list_simple_monitors().await.map_err(fmt_error);
                let _ = tx.send(Message::SimpleMonitors(result));
            });
            return;
        }
        self.fill_selection(Pane::Monitors);
    }

    pub(super) fn secrets_ensure_loaded(&mut self) {
        if self.secrets.vaults.is_idle() {
            self.secrets.vaults = Loadable::Loading;
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.list_vaults().await.map_err(fmt_error);
                let _ = tx.send(Message::Vaults(result));
            });
            return;
        }
        self.fill_selection(Pane::Vaults);
        let Some(vault) = self.selected_vault() else {
            return;
        };
        let key = vault.id.clone();
        if self.secrets.secrets.get(&key).is_none_or(Loadable::is_idle) {
            self.secrets.secrets.insert(key, Loadable::Loading);
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            let vault_id = vault.id;
            tokio::spawn(async move {
                let result = client.list_secrets(&vault_id).await.map_err(fmt_error);
                let _ = tx.send(Message::Secrets {
                    vault: vault_id,
                    result,
                });
            });
        } else {
            self.fill_selection(Pane::Secrets);
        }
    }

    // --- キー入力 ---

    pub(super) fn on_key_dns(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.dns.focus = ListFocus::Right;
                self.fill_selection(Pane::DnsRecords);
            }
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => self.dns.focus = ListFocus::Left,
            KeyCode::Char('a') => self.open_add_dns_record(),
            KeyCode::Char('e') => self.open_edit_dns_record(),
            KeyCode::Char('d') => self.confirm_delete_dns_record(),
            KeyCode::Char('n') => self.open_create_dns_zone(),
            KeyCode::Char('E') => self.open_edit_dns_zone(),
            KeyCode::Char('D') => self.confirm_delete_dns_zone(),
            _ => {}
        }
    }

    fn open_create_dns_zone(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::DnsZoneForm(DnsZoneForm {
            mode: DnsZoneFormMode::Create,
            target: None,
            name: String::new(),
            description: String::new(),
            field: 0,
        }));
    }

    fn open_edit_dns_zone(&mut self) {
        if !self.require_write() {
            return;
        }
        if self.dns.focus != ListFocus::Left {
            self.set_status(
                "DNSゾーン一覧へ戻って対象を選択してください",
                StatusKind::Info,
            );
            return;
        }
        let Some(zone) = self.selected_dns_zone().cloned() else {
            self.set_status("編集するDNSゾーンを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::DnsZoneForm(DnsZoneForm {
            mode: DnsZoneFormMode::Edit,
            name: zone.name.clone(),
            description: zone.description.clone(),
            target: Some(zone),
            field: 1,
        }));
    }

    pub(super) fn submit_dns_zone_form(&mut self, form: DnsZoneForm) {
        let name = match validate_dns_zone_form(&form) {
            Ok(name) => name,
            Err(message) => {
                self.set_status(message, StatusKind::Error);
                self.overlay = Some(Overlay::DnsZoneForm(form));
                return;
            }
        };
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            DnsZoneFormMode::Create => {
                self.dns.reselect_zone = Some(name.clone());
                let description = form.description.trim().to_string();
                let label = format!("DNSゾーン「{name}」を作成");
                tokio::spawn(async move {
                    let result = client
                        .create_dns_zone(&name, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::DnsAction { label, result });
                });
            }
            DnsZoneFormMode::Edit => {
                let Some(zone) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    return;
                };
                self.dns.reselect_zone = Some(zone.name.clone());
                let description = form.description.trim().to_string();
                let label = format!("DNSゾーン「{}」を更新", zone.name);
                tokio::spawn(async move {
                    let result = client
                        .update_dns_zone(&zone, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::DnsAction { label, result });
                });
            }
        }
    }

    fn confirm_delete_dns_zone(&mut self) {
        if !self.require_write() {
            return;
        }
        if self.dns.focus != ListFocus::Left {
            self.set_status(
                "DNSゾーン一覧へ戻って対象を選択してください",
                StatusKind::Info,
            );
            return;
        }
        let Some(zone) = self.selected_dns_zone() else {
            self.set_status("削除するDNSゾーンを選択してください", StatusKind::Info);
            return;
        };
        let (id, name, record_count) = (zone.id, zone.name.clone(), zone.records.len());
        self.overlay = Some(Overlay::Confirm {
            title: "DNSゾーンの削除".to_string(),
            body: format!(
                "DNSゾーン「{name}」と登録されているレコード {record_count} 件を削除します。\n削除後は復元できません。\n\n実行するにはゾーン名を入力してください。"
            ),
            verify: Some(name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteDnsZone { id, name },
        });
    }

    pub(super) fn run_delete_dns_zone(&mut self, id: crate::sacloud::ResourceId, name: String) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!("DNSゾーン「{name}」を削除");
        self.dns.reselect_zone = None;
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client.delete_dns_zone(id).await.map_err(fmt_error);
            let _ = tx.send(Message::DnsAction { label, result });
        });
    }

    fn open_add_dns_record(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(zone) = self.selected_dns_zone().cloned() else {
            self.set_status("DNSゾーンを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::DnsRecordForm(DnsRecordForm {
            mode: DnsRecordFormMode::Add,
            zone,
            original: None,
            name: "@".to_string(),
            record_type: "A".to_string(),
            data: String::new(),
            ttl: "3600".to_string(),
            field: 0,
        }));
    }

    fn open_edit_dns_record(&mut self) {
        if !self.require_write() {
            return;
        }
        if self.dns.focus != ListFocus::Right {
            self.set_status(
                "レコード一覧へ移動して対象を選択してください",
                StatusKind::Info,
            );
            return;
        }
        let Some(zone) = self.selected_dns_zone().cloned() else {
            return;
        };
        let Some(record) = self.selected_dns_record() else {
            self.set_status("編集するレコードを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::DnsRecordForm(DnsRecordForm {
            mode: DnsRecordFormMode::Edit,
            zone,
            original: Some(record.clone()),
            name: record.name,
            record_type: record.record_type,
            data: record.data,
            ttl: record.ttl.to_string(),
            field: 0,
        }));
    }

    pub(super) fn submit_dns_record_form(&mut self, form: DnsRecordForm) {
        let record = match dns_record_from_form(&form) {
            Ok(record) => record,
            Err(message) => {
                self.set_status(message, StatusKind::Error);
                self.overlay = Some(Overlay::DnsRecordForm(form));
                return;
            }
        };
        let mut records = form.zone.records.clone();
        let label = match form.mode {
            DnsRecordFormMode::Add => {
                if records.contains(&record) {
                    self.set_status("同じDNSレコードが既にあります", StatusKind::Error);
                    self.overlay = Some(Overlay::DnsRecordForm(form));
                    return;
                }
                records.push(record.clone());
                format!(
                    "DNSレコード「{} {}」を追加",
                    record.name, record.record_type
                )
            }
            DnsRecordFormMode::Edit => {
                let Some(original) = &form.original else {
                    return;
                };
                let Some(index) = records.iter().position(|item| item == original) else {
                    self.set_status("編集元のDNSレコードが見つかりません", StatusKind::Error);
                    return;
                };
                if records
                    .iter()
                    .enumerate()
                    .any(|(i, item)| i != index && item == &record)
                {
                    self.set_status("同じDNSレコードが既にあります", StatusKind::Error);
                    self.overlay = Some(Overlay::DnsRecordForm(form));
                    return;
                }
                records[index] = record.clone();
                format!(
                    "DNSレコード「{} {}」を更新",
                    record.name, record.record_type
                )
            }
        };
        self.run_dns_update(form.zone, records, label);
    }

    fn confirm_delete_dns_record(&mut self) {
        if !self.require_write() {
            return;
        }
        if self.dns.focus != ListFocus::Right {
            self.set_status(
                "レコード一覧へ移動して対象を選択してください",
                StatusKind::Info,
            );
            return;
        }
        let Some(zone) = self.selected_dns_zone().cloned() else {
            return;
        };
        let Some(record) = self.selected_dns_record() else {
            self.set_status("削除するレコードを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "DNSレコードの削除".to_string(),
            body: format!(
                "ゾーン「{}」から次のレコードを削除します。\n\n{}  {}  {}  TTL {}\n\nこの操作は取り消せません。実行しますか？",
                zone.name, record.name, record.record_type, record.data, record.ttl
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteDnsRecord { zone, record },
        });
    }

    pub(super) fn run_delete_dns_record(&mut self, zone: DnsZone, record: DnsRecord) {
        let Some(index) = zone.records.iter().position(|item| item == &record) else {
            self.set_status("削除するDNSレコードが見つかりません", StatusKind::Error);
            return;
        };
        let mut records = zone.records.clone();
        records.remove(index);
        let label = format!(
            "DNSレコード「{} {}」を削除",
            record.name, record.record_type
        );
        self.run_dns_update(zone, records, label);
    }

    fn run_dns_update(&mut self, zone: DnsZone, records: Vec<DnsRecord>, label: String) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.dns.reselect_zone = Some(zone.name.clone());
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .update_dns_records(zone.id, &records, &zone.settings_hash)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::DnsAction { label, result });
        });
    }

    pub(super) fn on_key_secrets(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.secrets.focus = ListFocus::Right;
                self.fill_selection(Pane::Secrets);
            }
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                self.secrets.focus = ListFocus::Left
            }
            // 値の取得は明示操作のときだけ。
            KeyCode::Char('u') => self.confirm_unveil(),
            KeyCode::Char('n') if self.secrets.focus == ListFocus::Left => self.open_create_vault(),
            KeyCode::Char('E') if self.secrets.focus == ListFocus::Left => self.open_edit_vault(),
            KeyCode::Char('D') if self.secrets.focus == ListFocus::Left => {
                self.confirm_delete_vault()
            }
            KeyCode::Char('a') if self.secrets.focus == ListFocus::Right => {
                self.open_create_secret()
            }
            KeyCode::Char('e') if self.secrets.focus == ListFocus::Right => {
                self.open_update_secret()
            }
            KeyCode::Char('d') if self.secrets.focus == ListFocus::Right => {
                self.confirm_delete_secret()
            }
            _ => {}
        }
    }

    fn open_create_vault(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::VaultForm(VaultForm {
            mode: VaultFormMode::Create,
            target: None,
            name: String::new(),
            description: String::new(),
            kms_key_id: String::new(),
            tags: String::new(),
            field: 0,
        }));
    }

    fn open_edit_vault(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(vault) = self.selected_vault() else {
            self.set_status("Vaultを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::VaultForm(VaultForm {
            mode: VaultFormMode::Edit,
            target: Some(vault.clone()),
            name: vault.name,
            description: vault.description,
            kms_key_id: vault.kms_key_id,
            tags: vault.tags.join(", "),
            field: 0,
        }));
    }

    fn confirm_delete_vault(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(vault) = self.selected_vault() else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "Vaultの削除".to_string(),
            body: format!(
                "Vault「{}」を削除します。\n課金はVaultの削除時に停止します。\n\nこの操作は取り消せません。",
                vault.name
            ),
            verify: Some(vault.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteVault {
                id: vault.id,
                name: vault.name,
            },
        });
    }

    fn open_create_secret(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(vault) = self.selected_vault() else {
            self.set_status("Vaultを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::SecretForm(SecretForm::new(
            SecretFormMode::Create,
            vault,
            String::new(),
        )));
    }

    fn open_update_secret(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(vault), Some(secret)) = (self.selected_vault(), self.selected_secret()) else {
            self.set_status("シークレットを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::SecretForm(SecretForm::new(
            SecretFormMode::Update,
            vault,
            secret.name,
        )));
    }

    fn confirm_delete_secret(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(vault), Some(secret)) = (self.selected_vault(), self.selected_secret()) else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "シークレットの削除".to_string(),
            body: format!(
                "Vault「{}」のシークレット「{}」と全バージョンを削除します。\n\nこの操作は取り消せません。",
                vault.name, secret.name
            ),
            verify: Some(secret.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteSecret {
                vault: vault.id,
                name: secret.name,
            },
        });
    }

    pub(super) fn submit_vault_form(&mut self, mut form: VaultForm) {
        form.name = form.name.trim().to_string();
        form.kms_key_id = form.kms_key_id.trim().to_string();
        if form.name.is_empty() {
            self.set_status("Vaultの名前を入力してください", StatusKind::Error);
            self.overlay = Some(Overlay::VaultForm(form));
            return;
        }
        if form.mode == VaultFormMode::Create && form.kms_key_id.is_empty() {
            self.set_status("KMS鍵IDを入力してください", StatusKind::Error);
            self.overlay = Some(Overlay::VaultForm(form));
            return;
        }

        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let tags = form.tags();
        let name = form.name;
        let description = form.description;
        let kms_key_id = form.kms_key_id;
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            VaultFormMode::Create => tokio::spawn(async move {
                let label = format!("Vault「{name}」を作成");
                let result = client
                    .create_vault(&name, &description, &kms_key_id, &tags)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::SecretManagerAction {
                    label,
                    reselect_vault: None,
                    result,
                });
            }),
            VaultFormMode::Edit => {
                let Some(vault) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象のVaultがありません", StatusKind::Error);
                    return;
                };
                let vault_id = vault.id.clone();
                tokio::spawn(async move {
                    let label = format!("Vault「{name}」を更新");
                    let result = client
                        .update_vault(&vault, &name, &description, &tags)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::SecretManagerAction {
                        label,
                        reselect_vault: Some(vault_id),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn submit_secret_form(&mut self, mut form: SecretForm) {
        form.name = form.name.trim().to_string();
        if form.name.is_empty() || form.value.is_empty() {
            self.set_status(
                "シークレットの名前と値を入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::SecretForm(form));
            return;
        }
        if form.value.len() > 65_536 {
            self.set_status(
                "シークレット値は65,536バイト以下にしてください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::SecretForm(form));
            return;
        }

        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let vault_id = form.vault.id;
        let name = form.name;
        let value = form.value;
        let verb = if form.mode == SecretFormMode::Create {
            "登録"
        } else {
            "新バージョンを登録"
        };
        let label = format!("シークレット「{name}」を{verb}");
        let reselect_vault = vault_id.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .put_secret(&vault_id, &name, value)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::SecretManagerAction {
                label,
                reselect_vault: Some(reselect_vault),
                result,
            });
        });
    }

    pub(super) fn run_delete_vault(&mut self, id: String, name: String) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!("Vault「{name}」を削除");
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client.delete_vault(&id).await.map_err(fmt_error);
            let _ = tx.send(Message::SecretManagerAction {
                label,
                reselect_vault: None,
                result,
            });
        });
    }

    pub(super) fn run_delete_secret(&mut self, vault: String, name: String) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!("シークレット「{name}」を削除");
        let reselect_vault = vault.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client.delete_secret(&vault, &name).await.map_err(fmt_error);
            let _ = tx.send(Message::SecretManagerAction {
                label,
                reselect_vault: Some(reselect_vault),
                result,
            });
        });
    }

    /// シークレットの値を表示する前に確認を挟む。
    fn confirm_unveil(&mut self) {
        let Some(vault) = self.selected_vault() else {
            return;
        };
        let Some(secret) = self.selected_secret() else {
            self.set_status("シークレットを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "シークレットの値を表示".to_string(),
            body: format!(
                "Vault「{}」のシークレット「{}」の値を取得して画面に表示します。\n\
                 肩越しに覗かれていないか確認してください。",
                vault.name, secret.name
            ),
            verify: None,
            typed: String::new(),
            action: super::ConfirmAction::UnveilSecret {
                vault: vault.id,
                name: secret.name,
            },
        });
    }

    pub(super) fn run_unveil(&mut self, vault: String, name: String) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        let secret_name = name.clone();
        tokio::spawn(async move {
            let result = client
                .unveil_secret(&vault, &name, None)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::UnveiledSecret {
                name: secret_name,
                result,
            });
        });
        self.set_status("取得中…", StatusKind::Info);
    }

    pub(super) fn observability_invalidate(&mut self) {
        self.dns = DnsView::default();
        self.simple_monitor = SimpleMonitorView::default();
        self.secrets = SecretsView::default();
        self.monitoring = MonitoringView::default();
    }
}

fn dns_record_from_form(form: &DnsRecordForm) -> Result<DnsRecord, &'static str> {
    let name = form.name.trim();
    if name.chars().any(char::is_whitespace) {
        return Err("名前に空白は使用できません");
    }
    let record_type = form.record_type.trim().to_ascii_uppercase();
    if record_type.is_empty() {
        return Err("レコード種別を入力してください");
    }
    if !record_type
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("レコード種別には英数字とハイフンだけを使用できます");
    }
    let data = form.data.trim();
    if data.is_empty() {
        return Err("値を入力してください");
    }
    let ttl = form
        .ttl
        .trim()
        .parse::<u32>()
        .map_err(|_| "TTLは0以上の整数で入力してください")?;
    Ok(DnsRecord {
        name: if name.is_empty() { "@" } else { name }.to_string(),
        record_type,
        data: data.to_string(),
        ttl,
    })
}

pub(super) fn notification_service_label(service_type: &str) -> &str {
    match service_type {
        "SAKURA_SIMPLE_NOTICE" => "シンプル通知",
        "SAKURA_EVENT_BUS" => "EventBus",
        other => other,
    }
}

pub(super) fn format_match_labels(labels: &[(String, String)]) -> String {
    labels
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn parse_match_labels(input: &str) -> Result<Vec<(String, String)>, &'static str> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    input
        .split(',')
        .map(|item| {
            let Some((name, value)) = item.trim().split_once('=') else {
                return Err("ラベル条件は name=value をカンマ区切りで入力してください");
            };
            let (name, value) = (name.trim(), value.trim());
            if name.is_empty() || value.is_empty() {
                return Err("ラベル条件の名前と値は空にできません");
            }
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

fn validate_dns_zone_form(form: &DnsZoneForm) -> Result<String, &'static str> {
    let name = form.name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err("ゾーン名を入力してください");
    }
    if name.len() > 253 {
        return Err("ゾーン名は253文字以内で入力してください");
    }
    if name.starts_with(['.', '-']) || name.ends_with(['.', '-']) {
        return Err("ゾーン名の先頭と末尾にピリオドやハイフンは使用できません");
    }
    if name.contains("..") || name.contains(".-") || name.contains("-.") {
        return Err("ゾーン名に連続したピリオドや不正な区切りは使用できません");
    }
    if name.split('.').any(|label| {
        label.len() > 63
            || label.is_empty()
            || !label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }) {
        return Err("ゾーン名は英小文字・数字・ハイフンで構成されたラベルを指定してください");
    }
    if form.description.chars().count() > 512 {
        return Err("説明は512文字以内で入力してください");
    }
    Ok(name)
}

fn simple_monitor_input(form: &SimpleMonitorForm) -> Result<SimpleMonitorInput, &'static str> {
    let target = form.target.trim();
    if target.is_empty() {
        return Err("監視対象を入力してください");
    }
    if target.chars().any(char::is_whitespace) {
        return Err("監視対象に空白は使用できません");
    }
    if form.description.chars().count() > 512 {
        return Err("説明は512文字以内で入力してください");
    }
    let delay_loop = form
        .delay_loop
        .trim()
        .parse::<u32>()
        .map_err(|_| "監視間隔は60秒単位の整数で入力してください")?;
    if delay_loop == 0 || delay_loop % 60 != 0 {
        return Err("監視間隔は60秒単位の整数で入力してください");
    }
    let timeout = form
        .timeout
        .trim()
        .parse::<u32>()
        .map_err(|_| "タイムアウトは1以上の整数で入力してください")?;
    if timeout == 0 {
        return Err("タイムアウトは1以上の整数で入力してください");
    }
    let protocol = form.protocol();
    let port = if form.port.trim().is_empty() {
        None
    } else {
        let port = form
            .port
            .trim()
            .parse::<u16>()
            .map_err(|_| "ポートは1〜65535の整数で入力してください")?;
        if port == 0 {
            return Err("ポートは1〜65535の整数で入力してください");
        }
        Some(port)
    };
    if protocol == "tcp" && port.is_none() {
        return Err("TCP監視ではポートを入力してください");
    }
    let expected_status = if form.expected_status.trim().is_empty() {
        None
    } else {
        let status = form
            .expected_status
            .trim()
            .parse::<u16>()
            .map_err(|_| "期待ステータスは100〜599で入力してください")?;
        if !(100..=599).contains(&status) {
            return Err("期待ステータスは100〜599で入力してください");
        }
        Some(status)
    };
    Ok(SimpleMonitorInput {
        target: target.to_string(),
        description: form.description.trim().to_string(),
        protocol: protocol.to_string(),
        port: if protocol == "ping" { None } else { port },
        path: if form.path.trim().is_empty() {
            "/".to_string()
        } else {
            form.path.trim().to_string()
        },
        expected_status: if matches!(protocol, "http" | "https") {
            expected_status
        } else {
            None
        },
        delay_loop,
        timeout,
        enabled: form.enabled,
        notify_email: form.notify_email,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // モニタリングスイート側へ移したフォームだが、検証はここに残してある。
    use crate::app::{LogMeasureRuleForm, LogMeasureRuleFormMode};

    /// 右の一覧に入っても、左の一覧に戻れること。
    ///
    /// 「右に選択があるか」で判定していたころは、いちど右に入ると
    /// 左の選択を動かせず、最初の 1 件しか中身を見られなかった。
    #[test]
    fn focus_starts_on_the_left_list() {
        assert_eq!(DnsView::default().focus, ListFocus::Left);
        assert_eq!(SecretsView::default().focus, ListFocus::Left);
        assert_eq!(MonitoringView::default().focus, ListFocus::Left);
    }

    fn dns_form(name: &str, record_type: &str, data: &str, ttl: &str) -> DnsRecordForm {
        DnsRecordForm {
            mode: DnsRecordFormMode::Add,
            zone: DnsZone {
                id: crate::sacloud::ResourceId(1),
                name: "example.jp".to_string(),
                description: String::new(),
                tags: Vec::new(),
                name_servers: Vec::new(),
                records: Vec::new(),
                settings_hash: "hash".to_string(),
                created_at: None,
            },
            original: None,
            name: name.to_string(),
            record_type: record_type.to_string(),
            data: data.to_string(),
            ttl: ttl.to_string(),
            field: 0,
        }
    }

    #[test]
    fn normalizes_dns_record_form() {
        let record =
            dns_record_from_form(&dns_form("", " a ", " 192.0.2.1 ", "300")).expect("valid record");
        assert_eq!(record.name, "@");
        assert_eq!(record.record_type, "A");
        assert_eq!(record.data, "192.0.2.1");
        assert_eq!(record.ttl, 300);
    }

    #[test]
    fn rejects_invalid_dns_record_form() {
        assert_eq!(
            dns_record_from_form(&dns_form("bad name", "A", "192.0.2.1", "300")),
            Err("名前に空白は使用できません")
        );
        assert_eq!(
            dns_record_from_form(&dns_form("www", "A", "", "300")),
            Err("値を入力してください")
        );
        assert_eq!(
            dns_record_from_form(&dns_form("www", "A", "192.0.2.1", "x")),
            Err("TTLは0以上の整数で入力してください")
        );
    }

    #[test]
    fn validates_and_normalizes_dns_zone_form() {
        let form = DnsZoneForm {
            mode: DnsZoneFormMode::Create,
            target: None,
            name: " Example.JP ".to_string(),
            description: String::new(),
            field: 0,
        };
        assert_eq!(validate_dns_zone_form(&form), Ok("example.jp".to_string()));
    }

    #[test]
    fn rejects_invalid_dns_zone_names() {
        for name in ["", "-example.jp", "example..jp", "exa_mple.jp"] {
            let form = DnsZoneForm {
                mode: DnsZoneFormMode::Create,
                target: None,
                name: name.to_string(),
                description: String::new(),
                field: 0,
            };
            assert!(validate_dns_zone_form(&form).is_err(), "{name}");
        }
    }

    fn monitor_form(protocol: usize) -> SimpleMonitorForm {
        SimpleMonitorForm {
            mode: SimpleMonitorFormMode::Create,
            target_monitor: None,
            target: "example.jp".to_string(),
            description: String::new(),
            protocol,
            port: String::new(),
            path: "/".to_string(),
            expected_status: "200".to_string(),
            delay_loop: "60".to_string(),
            timeout: "10".to_string(),
            enabled: true,
            notify_email: true,
            field: 0,
        }
    }

    #[test]
    fn validates_simple_monitor_protocol_fields() {
        let ping = simple_monitor_input(&monitor_form(0)).expect("valid ping");
        assert_eq!(ping.protocol, "ping");
        assert_eq!(ping.port, None);

        let mut tcp = monitor_form(1);
        assert_eq!(
            simple_monitor_input(&tcp),
            Err("TCP監視ではポートを入力してください")
        );
        tcp.port = "443".to_string();
        assert_eq!(simple_monitor_input(&tcp).unwrap().port, Some(443));
    }

    #[test]
    fn requires_sixty_second_monitor_interval() {
        let mut form = monitor_form(2);
        form.delay_loop = "90".to_string();
        assert_eq!(
            simple_monitor_input(&form),
            Err("監視間隔は60秒単位の整数で入力してください")
        );
    }

    #[test]
    fn parses_notification_match_labels() {
        assert_eq!(
            parse_match_labels("severity=critical, instance=web-01"),
            Ok(vec![
                ("severity".to_string(), "critical".to_string()),
                ("instance".to_string(), "web-01".to_string()),
            ])
        );
        assert_eq!(parse_match_labels(""), Ok(Vec::new()));
        assert!(parse_match_labels("severity").is_err());
        assert!(parse_match_labels("=critical").is_err());
    }

    #[test]
    fn validates_log_measure_rule_json_without_narrowing_matcher_types() {
        let form = LogMeasureRuleForm {
            mode: LogMeasureRuleFormMode::Create,
            project: AlertProject {
                id: 1,
                resource_id: 1,
                name: "alerts".to_string(),
                description: String::new(),
                tags: Vec::new(),
                created_at: None,
            },
            target: None,
            log_storage_id: "101".to_string(),
            metrics_storage_id: "202".to_string(),
            name: "errors".to_string(),
            description: String::new(),
            rule_json: r#"{"version":"v1","query":{"matchers":[{"type":"map-key-exists","field":"attributes","key":"error"}]}}"#.to_string(),
            field: 0,
        };
        let input = form.input().expect("valid v1 rule");
        assert_eq!(input.rule["query"]["matchers"][0]["type"], "map-key-exists");

        let mut invalid = form;
        invalid.rule_json = r#"{"version":"v2","query":{"matchers":[]}}"#.to_string();
        assert!(invalid.input().is_err());
    }
}
