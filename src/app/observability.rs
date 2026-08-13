//! DNS・シンプル監視・シークレットマネージャ・モニタリングスイートの状態（すべて閲覧のみ）。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Overlay, Pane, StatusKind, fmt_error, matches};
use crate::commonservice::{DnsRecord, DnsZone, SimpleMonitor};
use crate::monitoring::{AlertHistory, AlertProject, AlertRule, Storage};
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
    /// ゾーンごとの Vault 一覧。
    pub vaults: HashMap<String, Loadable<Vec<Vault>>>,
    pub vault_state: TableState,
    pub focus: ListFocus,
    /// `(ゾーン, VaultID)` をキーにしたシークレット一覧。値は含まない。
    pub secrets: HashMap<(String, String), Loadable<Vec<Secret>>>,
    pub secret_state: TableState,
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

    // --- シークレットマネージャ ---

    pub fn visible_vaults(&self) -> Vec<Vault> {
        let Some(items) = self
            .secrets
            .vaults
            .get(&self.zone)
            .and_then(Loadable::ready)
        else {
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
            .get(&(self.zone.clone(), vault.id))
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
                    matches(
                        self.filters.get(Pane::Storages),
                        &[&s.name, s.kind.label(), &s.classification],
                    )
                })
                .collect(),
        )
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
        let zone = self.zone.clone();
        if self.secrets.vaults.get(&zone).is_none_or(Loadable::is_idle) {
            self.secrets.vaults.insert(zone.clone(), Loadable::Loading);
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.list_vaults(&zone).await.map_err(fmt_error);
                let _ = tx.send(Message::Vaults { zone, result });
            });
            return;
        }
        self.fill_selection(Pane::Vaults);
        let Some(vault) = self.selected_vault() else {
            return;
        };
        let key = (zone.clone(), vault.id.clone());
        if self.secrets.secrets.get(&key).is_none_or(Loadable::is_idle) {
            self.secrets.secrets.insert(key, Loadable::Loading);
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            let vault_id = vault.id;
            tokio::spawn(async move {
                let result = client
                    .list_secrets(&zone, &vault_id)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::Secrets {
                    zone,
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
            _ => {}
        }
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
            _ => {}
        }
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
                zone: self.zone.clone(),
                vault: vault.id,
                name: secret.name,
            },
        });
    }

    pub(super) fn run_unveil(&mut self, zone: String, vault: String, name: String) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        let secret_name = name.clone();
        tokio::spawn(async move {
            let result = client
                .unveil_secret(&zone, &vault, &name, None)
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
            _ => {}
        }
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
}
