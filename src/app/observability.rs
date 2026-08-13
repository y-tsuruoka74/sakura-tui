//! DNS・シンプル監視・シークレットマネージャ・モニタリングスイートの状態と操作。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    AlertProjectForm, AlertProjectFormMode, AlertRuleForm, AlertRuleFormMode, App, ConfirmAction,
    DnsRecordForm, DnsRecordFormMode, DnsZoneForm, DnsZoneFormMode, Loadable, Message, Overlay,
    Pane, SecretForm, SecretFormMode, SimpleMonitorForm, SimpleMonitorFormMode, StatusKind,
    StorageForm, StorageFormMode, VaultForm, VaultFormMode, fmt_error, matches,
};
use crate::commonservice::{DnsRecord, DnsZone, SimpleMonitor, SimpleMonitorInput};
use crate::monitoring::{AlertHistory, AlertProject, AlertRule, Storage, StorageKind};
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
}

impl MonitoringTab {
    pub const ALL: [MonitoringTab; 3] = [
        MonitoringTab::Rules,
        MonitoringTab::Histories,
        MonitoringTab::Storages,
    ];

    pub fn title(self) -> &'static str {
        match self {
            MonitoringTab::Rules => "ルール",
            MonitoringTab::Histories => "履歴",
            MonitoringTab::Storages => "保管先",
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
    pub histories: HashMap<(String, i64), Loadable<Vec<AlertHistory>>>,
    pub history_state: TableState,
    pub storages: HashMap<String, Loadable<Vec<Storage>>>,
    pub storage_state: TableState,
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

    pub fn visible_projects(&self) -> Vec<AlertProject> {
        let Some(items) = self
            .monitoring
            .projects
            .get(&self.zone)
            .and_then(Loadable::ready)
        else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Projects);
        items
            .iter()
            .filter(|p| matches(filter, &[&p.name, &p.description]))
            .cloned()
            .collect()
    }

    pub fn selected_project(&self) -> Option<AlertProject> {
        let index = self.monitoring.project_state.selected()?;
        self.visible_projects().into_iter().nth(index)
    }

    pub fn visible_rules(&self) -> Loadable<Vec<AlertRule>> {
        let Some(project) = self.selected_project() else {
            return Loadable::Idle;
        };
        let loadable = self
            .monitoring
            .rules
            .get(&(self.zone.clone(), project.resource_id))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|r| matches(self.filters.get(Pane::Rules), &[&r.name, &r.query]))
                .collect(),
        )
    }

    pub fn visible_histories(&self) -> Loadable<Vec<AlertHistory>> {
        let Some(project) = self.selected_project() else {
            return Loadable::Idle;
        };
        let loadable = self
            .monitoring
            .histories
            .get(&(self.zone.clone(), project.resource_id))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|h| {
                    matches(
                        self.filters.get(Pane::Histories),
                        &[&h.severity, &h.labels, &h.rule_uid],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_rule(&self) -> Option<AlertRule> {
        let index = self.monitoring.rule_state.selected()?;
        self.visible_rules().ready()?.get(index).cloned()
    }

    pub fn visible_storages(&self) -> Loadable<Vec<Storage>> {
        let loadable = self
            .monitoring
            .storages
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|s| {
                    let area = if s.is_system { "system" } else { "user" };
                    matches(
                        self.filters.get(Pane::Storages),
                        &[&s.name, s.kind.label(), area, &s.classification],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_storage(&self) -> Option<Storage> {
        let index = self.monitoring.storage_state.selected()?;
        self.visible_storages().ready()?.get(index).cloned()
    }

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

    pub(super) fn monitoring_ensure_loaded(&mut self) {
        let zone = self.zone.clone();
        if self.monitoring.tab == MonitoringTab::Storages {
            if self
                .monitoring
                .storages
                .get(&zone)
                .is_none_or(Loadable::is_idle)
            {
                self.monitoring
                    .storages
                    .insert(zone.clone(), Loadable::Loading);
                self.inflight += 1;
                let client = self.monitoring_client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = client.list_storages(&zone).await.map_err(fmt_error);
                    let _ = tx.send(Message::Storages { zone, result });
                });
            } else {
                self.fill_selection(Pane::Storages);
            }
            return;
        }

        if self
            .monitoring
            .projects
            .get(&zone)
            .is_none_or(Loadable::is_idle)
        {
            self.monitoring
                .projects
                .insert(zone.clone(), Loadable::Loading);
            self.inflight += 1;
            let client = self.monitoring_client.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.list_projects(&zone).await.map_err(fmt_error);
                let _ = tx.send(Message::Projects { zone, result });
            });
            return;
        }
        self.fill_selection(Pane::Projects);
        let Some(project) = self.selected_project() else {
            return;
        };
        let key = (zone.clone(), project.resource_id);

        match self.monitoring.tab {
            MonitoringTab::Rules => {
                if self
                    .monitoring
                    .rules
                    .get(&key)
                    .is_none_or(Loadable::is_idle)
                {
                    self.monitoring.rules.insert(key, Loadable::Loading);
                    self.inflight += 1;
                    let client = self.monitoring_client.clone();
                    let tx = self.tx.clone();
                    let id = project.resource_id;
                    tokio::spawn(async move {
                        let result = client.list_rules(&zone, id).await.map_err(fmt_error);
                        let _ = tx.send(Message::AlertRules {
                            zone,
                            project: id,
                            result,
                        });
                    });
                } else {
                    self.fill_selection(Pane::Rules);
                }
            }
            MonitoringTab::Histories => {
                if self
                    .monitoring
                    .histories
                    .get(&key)
                    .is_none_or(Loadable::is_idle)
                {
                    self.monitoring.histories.insert(key, Loadable::Loading);
                    self.inflight += 1;
                    let client = self.monitoring_client.clone();
                    let tx = self.tx.clone();
                    let id = project.resource_id;
                    tokio::spawn(async move {
                        let result = client.list_histories(&zone, id).await.map_err(fmt_error);
                        let _ = tx.send(Message::AlertHistories {
                            zone,
                            project: id,
                            result,
                        });
                    });
                } else {
                    self.fill_selection(Pane::Histories);
                }
            }
            MonitoringTab::Storages => {}
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

    pub(super) fn on_key_monitoring(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.monitoring.focus = ListFocus::Right
            }
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                self.monitoring.focus = ListFocus::Left
            }
            KeyCode::Tab => self.cycle_monitoring_tab(1),
            KeyCode::BackTab => self.cycle_monitoring_tab(-1),
            KeyCode::Char('1') => self.set_monitoring_tab(MonitoringTab::Rules),
            KeyCode::Char('2') => self.set_monitoring_tab(MonitoringTab::Histories),
            KeyCode::Char('3') => self.set_monitoring_tab(MonitoringTab::Storages),
            KeyCode::Char('n')
                if self.monitoring.focus == ListFocus::Left
                    && self.monitoring.tab != MonitoringTab::Storages =>
            {
                self.open_create_alert_project()
            }
            KeyCode::Char('E')
                if self.monitoring.focus == ListFocus::Left
                    && self.monitoring.tab != MonitoringTab::Storages =>
            {
                self.open_edit_alert_project()
            }
            KeyCode::Char('D')
                if self.monitoring.focus == ListFocus::Left
                    && self.monitoring.tab != MonitoringTab::Storages =>
            {
                self.confirm_delete_alert_project()
            }
            KeyCode::Char('a')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::Rules =>
            {
                self.open_create_alert_rule()
            }
            KeyCode::Char('e')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::Rules =>
            {
                self.open_edit_alert_rule()
            }
            KeyCode::Char('d')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::Rules =>
            {
                self.confirm_delete_alert_rule()
            }
            KeyCode::Char('n') if self.monitoring.tab == MonitoringTab::Storages => {
                self.open_create_storage()
            }
            KeyCode::Char('E') if self.monitoring.tab == MonitoringTab::Storages => {
                self.open_edit_storage()
            }
            KeyCode::Char('D') if self.monitoring.tab == MonitoringTab::Storages => {
                self.confirm_delete_storage()
            }
            _ => {}
        }
    }

    fn open_create_alert_rule(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_project() else {
            self.set_status("プロジェクトを選択してください", StatusKind::Info);
            return;
        };
        let metrics_storage_id = self
            .monitoring
            .storages
            .get(&self.zone)
            .and_then(Loadable::ready)
            .and_then(|items| items.iter().find(|s| s.kind == StorageKind::Metrics))
            .map(|s| s.resource_id.to_string())
            .unwrap_or_default();
        self.overlay = Some(Overlay::AlertRuleForm(AlertRuleForm {
            mode: AlertRuleFormMode::Create,
            project,
            target: None,
            metrics_storage_id,
            name: String::new(),
            query: String::new(),
            warning_enabled: true,
            threshold_warning: String::new(),
            duration_warning: "60".to_string(),
            critical_enabled: false,
            threshold_critical: String::new(),
            duration_critical: "60".to_string(),
            field: 0,
        }));
    }

    fn open_edit_alert_rule(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(rule)) = (self.selected_project(), self.selected_rule()) else {
            self.set_status("アラートルールを選択してください", StatusKind::Info);
            return;
        };
        if rule.uid.is_empty() {
            self.set_status(
                "ルールUIDを取得できないため編集できません",
                StatusKind::Error,
            );
            return;
        }
        self.overlay = Some(Overlay::AlertRuleForm(AlertRuleForm {
            mode: AlertRuleFormMode::Edit,
            project,
            target: Some(rule.clone()),
            metrics_storage_id: rule.metrics_storage_id.to_string(),
            name: rule.name,
            query: rule.query,
            warning_enabled: rule.warning_enabled,
            threshold_warning: rule.threshold_warning,
            duration_warning: rule.duration_warning.to_string(),
            critical_enabled: rule.critical_enabled,
            threshold_critical: rule.threshold_critical,
            duration_critical: rule.duration_critical.to_string(),
            field: 0,
        }));
    }

    fn confirm_delete_alert_rule(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(rule)) = (self.selected_project(), self.selected_rule()) else {
            return;
        };
        if rule.uid.is_empty() {
            self.set_status(
                "ルールUIDを取得できないため削除できません",
                StatusKind::Error,
            );
            return;
        }
        self.overlay = Some(Overlay::Confirm {
            title: "アラートルールの削除".to_string(),
            body: format!(
                "アラートルール「{}」を削除します。\n\nこの操作は取り消せません。",
                rule.name
            ),
            verify: Some(rule.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteAlertRule {
                zone: self.zone.clone(),
                project: project.resource_id,
                uid: rule.uid,
                name: rule.name,
            },
        });
    }

    fn open_create_storage(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::StorageForm(StorageForm {
            mode: StorageFormMode::Create,
            target: None,
            kind: StorageKind::Logs,
            is_system: false,
            classification: 0,
            name: String::new(),
            description: String::new(),
            field: 0,
        }));
    }

    fn open_edit_storage(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(storage) = self.selected_storage() else {
            self.set_status("ストレージを選択してください", StatusKind::Info);
            return;
        };
        let classification = StorageForm::CLASSIFICATIONS
            .iter()
            .position(|value| *value == storage.classification)
            .unwrap_or(0);
        self.overlay = Some(Overlay::StorageForm(StorageForm {
            mode: StorageFormMode::Edit,
            target: Some(storage.clone()),
            kind: storage.kind,
            is_system: storage.is_system,
            classification,
            name: storage.name,
            description: storage.description,
            field: 3,
        }));
    }

    fn confirm_delete_storage(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(storage) = self.selected_storage() else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: format!("{}ストレージの削除", storage.kind.label()),
            body: format!(
                "{}ストレージ「{}」を削除します。保存済みデータへアクセスできなくなり、課金が停止します。\n\nこの操作は取り消せません。",
                storage.kind.label(),
                storage.name
            ),
            verify: Some(storage.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteStorage {
                zone: self.zone.clone(),
                storage,
            },
        });
    }

    fn open_create_alert_project(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::AlertProjectForm(AlertProjectForm {
            mode: AlertProjectFormMode::Create,
            target: None,
            name: String::new(),
            description: String::new(),
            field: 0,
        }));
    }

    fn open_edit_alert_project(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_project() else {
            self.set_status("プロジェクトを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::AlertProjectForm(AlertProjectForm {
            mode: AlertProjectFormMode::Edit,
            target: Some(project.clone()),
            name: project.name,
            description: project.description,
            field: 0,
        }));
    }

    fn confirm_delete_alert_project(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_project() else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "アラートプロジェクトの削除".to_string(),
            body: format!(
                "アラートプロジェクト「{}」を削除します。\n関連するルールと履歴へアクセスできなくなります。\n\nこの操作は取り消せません。",
                project.name
            ),
            verify: Some(project.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteAlertProject {
                zone: self.zone.clone(),
                resource_id: project.resource_id,
                name: project.name,
            },
        });
    }

    pub(super) fn submit_alert_project_form(&mut self, mut form: AlertProjectForm) {
        form.name = form.name.trim().to_string();
        if form.name.is_empty() {
            self.set_status("プロジェクト名を入力してください", StatusKind::Error);
            self.overlay = Some(Overlay::AlertProjectForm(form));
            return;
        }
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let name = form.name;
        let description = form.description;
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            AlertProjectFormMode::Create => tokio::spawn(async move {
                let label = format!("アラートプロジェクト「{name}」を作成");
                let result = client
                    .create_project(&zone, &name, &description)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::MonitoringAction {
                    zone,
                    label,
                    reselect_project: None,
                    result,
                });
            }),
            AlertProjectFormMode::Edit => {
                let Some(project) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let label = format!("アラートプロジェクト「{name}」を更新");
                    let result = client
                        .update_project(&zone, project.resource_id, &name, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::MonitoringAction {
                        zone,
                        label,
                        reselect_project: Some(project.resource_id),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_alert_project(
        &mut self,
        zone: String,
        resource_id: i64,
        name: String,
    ) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let label = format!("アラートプロジェクト「{name}」を削除");
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_project(&zone, resource_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::MonitoringAction {
                zone,
                label,
                reselect_project: None,
                result,
            });
        });
    }

    pub(super) fn submit_alert_rule_form(&mut self, form: AlertRuleForm) {
        let input = match form.input() {
            Ok(input) => input,
            Err(err) => {
                self.set_status(err, StatusKind::Error);
                self.overlay = Some(Overlay::AlertRuleForm(form));
                return;
            }
        };
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let project = form.project.resource_id;
        let name = input.name.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            AlertRuleFormMode::Create => tokio::spawn(async move {
                let label = format!("アラートルール「{name}」を作成");
                let result = client
                    .create_rule(&zone, project, &input)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::AlertRuleAction {
                    zone,
                    project,
                    label,
                    result,
                });
            }),
            AlertRuleFormMode::Edit => {
                let Some(rule) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let label = format!("アラートルール「{name}」を更新");
                    let result = client
                        .update_rule(&zone, project, &rule.uid, &input)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::AlertRuleAction {
                        zone,
                        project,
                        label,
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_alert_rule(
        &mut self,
        zone: String,
        project: i64,
        uid: String,
        name: String,
    ) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let label = format!("アラートルール「{name}」を削除");
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_rule(&zone, project, &uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::AlertRuleAction {
                zone,
                project,
                label,
                result,
            });
        });
    }

    pub(super) fn submit_storage_form(&mut self, mut form: StorageForm) {
        form.name = form.name.trim().to_string();
        if form.name.is_empty() {
            self.set_status("ストレージ名を入力してください", StatusKind::Error);
            self.overlay = Some(Overlay::StorageForm(form));
            return;
        }
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let kind = form.kind;
        let is_system = form.is_system && kind != StorageKind::Traces;
        let classification = form.classification().to_string();
        let name = form.name;
        let description = form.description;
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            StorageFormMode::Create => tokio::spawn(async move {
                let label = format!("{}ストレージ「{name}」を作成", kind.label());
                let result = client
                    .create_storage(&zone, kind, &name, &description, &classification, is_system)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::StorageAction {
                    zone,
                    label,
                    result,
                });
            }),
            StorageFormMode::Edit => {
                let Some(storage) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let label = format!("{}ストレージ「{name}」を更新", kind.label());
                    let result = client
                        .update_storage(&zone, &storage, &name, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::StorageAction {
                        zone,
                        label,
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_storage(&mut self, zone: String, storage: Storage) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let label = format!(
            "{}ストレージ「{}」を削除",
            storage.kind.label(),
            storage.name
        );
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_storage(&zone, &storage)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::StorageAction {
                zone,
                label,
                result,
            });
        });
    }

    /// タブを選んだらその中身を操作したいはずなので、右に移る。
    fn set_monitoring_tab(&mut self, tab: MonitoringTab) {
        self.monitoring.tab = tab;
        self.monitoring.focus = ListFocus::Right;
    }

    fn cycle_monitoring_tab(&mut self, delta: i32) {
        let current = MonitoringTab::ALL
            .iter()
            .position(|t| *t == self.monitoring.tab)
            .unwrap_or(0) as i32;
        let len = MonitoringTab::ALL.len() as i32;
        self.monitoring.tab = MonitoringTab::ALL[(current + delta).rem_euclid(len) as usize];
        self.monitoring.focus = ListFocus::Right;
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
}
