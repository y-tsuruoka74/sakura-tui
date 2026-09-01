//! モニタリングスイートの状態と操作。
//!
//! アラート・ログ計測・ログ/メトリクス転送・ダッシュボード・通知・保管先を扱う。
//! DNS・シンプル監視・シークレットマネージャは `observability.rs` に残してある。

use super::*;
use crate::monitoring::StorageKind;
// 表示・解析のヘルパは DNS 側と共用しているので、そちらから借りる。
use super::observability::{format_match_labels, notification_service_label, parse_match_labels};

impl App {
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

    pub fn visible_log_measure_rules(&self) -> Loadable<Vec<LogMeasureRule>> {
        let Some(project) = self.selected_project() else {
            return Loadable::Idle;
        };
        let loadable = self
            .monitoring
            .log_measure_rules
            .get(&(self.zone.clone(), project.resource_id))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|rule| {
                    matches(
                        self.filters.get(Pane::LogMeasureRules),
                        &[&rule.name, &rule.description, &rule.rule.to_string()],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_log_measure_rule(&self) -> Option<LogMeasureRule> {
        let index = self.monitoring.log_measure_rule_state.selected()?;
        self.visible_log_measure_rules()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_log_routings(&self) -> Loadable<Vec<LogRouting>> {
        let loadable = self
            .monitoring
            .log_routings
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|routing| {
                    matches(
                        self.filters.get(Pane::LogRoutings),
                        &[
                            &routing.publisher_code,
                            &routing.publisher_description,
                            &routing.variant,
                            &routing
                                .resource_id
                                .map(|id| id.to_string())
                                .unwrap_or_default(),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_log_routing(&self) -> Option<LogRouting> {
        let index = self.monitoring.log_routing_state.selected()?;
        self.visible_log_routings().ready()?.get(index).cloned()
    }

    pub fn visible_metrics_routings(&self) -> Loadable<Vec<MetricsRouting>> {
        let loadable = self
            .monitoring
            .metrics_routings
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|routing| {
                    matches(
                        self.filters.get(Pane::MetricsRoutings),
                        &[
                            &routing.publisher_code,
                            &routing.publisher_description,
                            &routing.variant,
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_metrics_routing(&self) -> Option<MetricsRouting> {
        self.visible_metrics_routings()
            .ready()?
            .get(self.monitoring.metrics_routing_state.selected()?)
            .cloned()
    }

    pub fn visible_dashboard_projects(&self) -> Loadable<Vec<DashboardProject>> {
        let loadable = self
            .monitoring
            .dashboard_projects
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|project| {
                    matches(
                        self.filters.get(Pane::Dashboards),
                        &[&project.name, &project.description],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_dashboard_project(&self) -> Option<DashboardProject> {
        self.visible_dashboard_projects()
            .ready()?
            .get(self.monitoring.dashboard_state.selected()?)
            .cloned()
    }

    fn routing_publishers(&self, storage: &str) -> Vec<Publisher> {
        self.monitoring
            .publishers
            .get(&self.zone)
            .and_then(Loadable::ready)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|publisher| {
                        let mut publisher = publisher.clone();
                        publisher
                            .variants
                            .retain(|variant| variant.storage == storage);
                        (!publisher.variants.is_empty()).then_some(publisher)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn visible_notification_targets(&self) -> Loadable<Vec<NotificationTarget>> {
        let Some(project) = self.selected_project() else {
            return Loadable::Idle;
        };
        let loadable = self
            .monitoring
            .notification_targets
            .get(&(self.zone.clone(), project.resource_id))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|target| {
                    matches(
                        self.filters.get(Pane::NotificationTargets),
                        &[&target.service_type, &target.url, &target.description],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_notification_target(&self) -> Option<NotificationTarget> {
        let index = self.monitoring.notification_target_state.selected()?;
        self.visible_notification_targets()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_notification_routings(&self) -> Loadable<Vec<NotificationRouting>> {
        let Some(project) = self.selected_project() else {
            return Loadable::Idle;
        };
        let loadable = self
            .monitoring
            .notification_routings
            .get(&(self.zone.clone(), project.resource_id))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|routing| {
                    let labels = format_match_labels(&routing.match_labels);
                    matches(
                        self.filters.get(Pane::NotificationRoutings),
                        &[
                            &routing.target_service_type,
                            &routing.target_description,
                            &labels,
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_notification_routing(&self) -> Option<NotificationRouting> {
        let index = self.monitoring.notification_routing_state.selected()?;
        self.visible_notification_routings()
            .ready()?
            .get(index)
            .cloned()
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

    pub fn visible_storage_access_keys(&self) -> Loadable<Vec<StorageAccessKey>> {
        let Some(storage) = self.selected_storage() else {
            return Loadable::Idle;
        };
        if !storage.supports_access_keys() {
            return Loadable::Ready(Vec::new());
        }
        let cache_key = (self.zone.clone(), storage.kind, storage.resource_id);
        let loadable = self
            .monitoring
            .storage_keys
            .get(&cache_key)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            items
                .into_iter()
                .filter(|key| {
                    matches(
                        self.filters.get(Pane::StorageKeys),
                        &[&key.uid, &key.token, &key.description],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_storage_access_key(&self) -> Option<StorageAccessKey> {
        let index = self.monitoring.storage_key_state.selected()?;
        self.visible_storage_access_keys()
            .ready()?
            .get(index)
            .cloned()
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
                let request_zone = zone.clone();
                tokio::spawn(async move {
                    let result = client.list_storages(&request_zone).await.map_err(fmt_error);
                    let _ = tx.send(Message::Storages {
                        zone: request_zone,
                        result,
                    });
                });
            } else {
                self.fill_selection(Pane::Storages);
            }
            let Some(storage) = self.selected_storage() else {
                return;
            };
            if !storage.supports_access_keys() {
                self.monitoring.storage_key_state.select(None);
                return;
            }
            let cache_key = (zone.clone(), storage.kind, storage.resource_id);
            if self
                .monitoring
                .storage_keys
                .get(&cache_key)
                .is_none_or(Loadable::is_idle)
            {
                self.monitoring
                    .storage_keys
                    .insert(cache_key, Loadable::Loading);
                self.inflight += 1;
                let client = self.monitoring_client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .list_storage_access_keys(&zone, &storage)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::StorageAccessKeys {
                        zone,
                        storage,
                        result,
                    });
                });
            } else {
                self.fill_selection(Pane::StorageKeys);
            }
            return;
        }

        if self.monitoring.tab == MonitoringTab::LogRoutings {
            self.ensure_publishers(&zone);
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
                let request_zone = zone.clone();
                tokio::spawn(async move {
                    let result = client.list_storages(&request_zone).await.map_err(fmt_error);
                    let _ = tx.send(Message::Storages {
                        zone: request_zone,
                        result,
                    });
                });
            }
            if self
                .monitoring
                .log_routings
                .get(&zone)
                .is_none_or(Loadable::is_idle)
            {
                self.monitoring
                    .log_routings
                    .insert(zone.clone(), Loadable::Loading);
                self.inflight += 1;
                let client = self.monitoring_client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = client.list_log_routings(&zone).await.map_err(fmt_error);
                    let _ = tx.send(Message::LogRoutings { zone, result });
                });
            } else {
                self.fill_selection(Pane::LogRoutings);
            }
            return;
        }
        if self.monitoring.tab == MonitoringTab::MetricsRoutings {
            self.ensure_publishers(&zone);
            self.ensure_monitoring_storages(&zone);
            if self
                .monitoring
                .metrics_routings
                .get(&zone)
                .is_none_or(Loadable::is_idle)
            {
                self.monitoring
                    .metrics_routings
                    .insert(zone.clone(), Loadable::Loading);
                self.inflight += 1;
                let client = self.monitoring_client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = client.list_metrics_routings(&zone).await.map_err(fmt_error);
                    let _ = tx.send(Message::MetricsRoutings { zone, result });
                });
            } else {
                self.fill_selection(Pane::MetricsRoutings);
            }
            return;
        }
        if self.monitoring.tab == MonitoringTab::Dashboards {
            if self
                .monitoring
                .dashboard_projects
                .get(&zone)
                .is_none_or(Loadable::is_idle)
            {
                self.monitoring
                    .dashboard_projects
                    .insert(zone.clone(), Loadable::Loading);
                self.inflight += 1;
                let client = self.monitoring_client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .list_dashboard_projects(&zone)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::DashboardProjects { zone, result });
                });
            } else {
                self.fill_selection(Pane::Dashboards);
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
            MonitoringTab::NotificationTargets => {
                if self
                    .monitoring
                    .notification_targets
                    .get(&key)
                    .is_none_or(Loadable::is_idle)
                {
                    self.monitoring
                        .notification_targets
                        .insert(key, Loadable::Loading);
                    self.inflight += 1;
                    let client = self.monitoring_client.clone();
                    let tx = self.tx.clone();
                    let id = project.resource_id;
                    tokio::spawn(async move {
                        let result = client
                            .list_notification_targets(&zone, id)
                            .await
                            .map_err(fmt_error);
                        let _ = tx.send(Message::NotificationTargets {
                            zone,
                            project: id,
                            result,
                        });
                    });
                } else {
                    self.fill_selection(Pane::NotificationTargets);
                }
            }
            MonitoringTab::NotificationRoutings => {
                if self
                    .monitoring
                    .notification_targets
                    .get(&key)
                    .is_none_or(Loadable::is_idle)
                {
                    self.monitoring
                        .notification_targets
                        .insert(key.clone(), Loadable::Loading);
                    self.inflight += 1;
                    let client = self.monitoring_client.clone();
                    let tx = self.tx.clone();
                    let request_zone = zone.clone();
                    let id = project.resource_id;
                    tokio::spawn(async move {
                        let result = client
                            .list_notification_targets(&request_zone, id)
                            .await
                            .map_err(fmt_error);
                        let _ = tx.send(Message::NotificationTargets {
                            zone: request_zone,
                            project: id,
                            result,
                        });
                    });
                }
                if self
                    .monitoring
                    .notification_routings
                    .get(&key)
                    .is_none_or(Loadable::is_idle)
                {
                    self.monitoring
                        .notification_routings
                        .insert(key, Loadable::Loading);
                    self.inflight += 1;
                    let client = self.monitoring_client.clone();
                    let tx = self.tx.clone();
                    let id = project.resource_id;
                    tokio::spawn(async move {
                        let result = client
                            .list_notification_routings(&zone, id)
                            .await
                            .map_err(fmt_error);
                        let _ = tx.send(Message::NotificationRoutings {
                            zone,
                            project: id,
                            result,
                        });
                    });
                } else {
                    self.fill_selection(Pane::NotificationRoutings);
                }
            }
            MonitoringTab::LogMeasureRules => {
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
                    let request_zone = zone.clone();
                    tokio::spawn(async move {
                        let result = client.list_storages(&request_zone).await.map_err(fmt_error);
                        let _ = tx.send(Message::Storages {
                            zone: request_zone,
                            result,
                        });
                    });
                }
                if self
                    .monitoring
                    .log_measure_rules
                    .get(&key)
                    .is_none_or(Loadable::is_idle)
                {
                    self.monitoring
                        .log_measure_rules
                        .insert(key, Loadable::Loading);
                    self.inflight += 1;
                    let client = self.monitoring_client.clone();
                    let tx = self.tx.clone();
                    let id = project.resource_id;
                    tokio::spawn(async move {
                        let result = client
                            .list_log_measure_rules(&zone, id)
                            .await
                            .map_err(fmt_error);
                        let _ = tx.send(Message::LogMeasureRules {
                            zone,
                            project: id,
                            result,
                        });
                    });
                } else {
                    self.fill_selection(Pane::LogMeasureRules);
                }
            }
            MonitoringTab::Storages => {}
            MonitoringTab::LogRoutings => unreachable!(),
            MonitoringTab::MetricsRoutings | MonitoringTab::Dashboards => unreachable!(),
        }
    }

    fn ensure_publishers(&mut self, zone: &str) {
        if self
            .monitoring
            .publishers
            .get(zone)
            .is_none_or(Loadable::is_idle)
        {
            self.monitoring
                .publishers
                .insert(zone.to_string(), Loadable::Loading);
            self.inflight += 1;
            let client = self.monitoring_client.clone();
            let tx = self.tx.clone();
            let zone = zone.to_string();
            tokio::spawn(async move {
                let result = client.list_publishers(&zone).await.map_err(fmt_error);
                let _ = tx.send(Message::Publishers { zone, result });
            });
        }
    }

    fn ensure_monitoring_storages(&mut self, zone: &str) {
        if self
            .monitoring
            .storages
            .get(zone)
            .is_none_or(Loadable::is_idle)
        {
            self.monitoring
                .storages
                .insert(zone.to_string(), Loadable::Loading);
            self.inflight += 1;
            let client = self.monitoring_client.clone();
            let tx = self.tx.clone();
            let zone = zone.to_string();
            tokio::spawn(async move {
                let result = client.list_storages(&zone).await.map_err(fmt_error);
                let _ = tx.send(Message::Storages { zone, result });
            });
        }
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
            KeyCode::Char('4') => self.set_monitoring_tab(MonitoringTab::NotificationTargets),
            KeyCode::Char('5') => self.set_monitoring_tab(MonitoringTab::NotificationRoutings),
            KeyCode::Char('6') => self.set_monitoring_tab(MonitoringTab::LogMeasureRules),
            KeyCode::Char('7') => self.set_monitoring_tab(MonitoringTab::LogRoutings),
            KeyCode::Char('8') => self.set_monitoring_tab(MonitoringTab::MetricsRoutings),
            KeyCode::Char('9') => self.set_monitoring_tab(MonitoringTab::Dashboards),
            KeyCode::Char('n')
                if self.monitoring.focus == ListFocus::Left
                    && !matches!(
                        self.monitoring.tab,
                        MonitoringTab::Storages
                            | MonitoringTab::LogRoutings
                            | MonitoringTab::MetricsRoutings
                            | MonitoringTab::Dashboards
                    ) =>
            {
                self.open_create_alert_project()
            }
            KeyCode::Char('E')
                if self.monitoring.focus == ListFocus::Left
                    && !matches!(
                        self.monitoring.tab,
                        MonitoringTab::Storages
                            | MonitoringTab::LogRoutings
                            | MonitoringTab::MetricsRoutings
                            | MonitoringTab::Dashboards
                    ) =>
            {
                self.open_edit_alert_project()
            }
            KeyCode::Char('D')
                if self.monitoring.focus == ListFocus::Left
                    && !matches!(
                        self.monitoring.tab,
                        MonitoringTab::Storages
                            | MonitoringTab::LogRoutings
                            | MonitoringTab::MetricsRoutings
                            | MonitoringTab::Dashboards
                    ) =>
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
            KeyCode::Char('a')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::LogMeasureRules =>
            {
                self.open_create_log_measure_rule()
            }
            KeyCode::Char('e')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::LogMeasureRules =>
            {
                self.open_edit_log_measure_rule()
            }
            KeyCode::Char('d')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::LogMeasureRules =>
            {
                self.confirm_delete_log_measure_rule()
            }
            KeyCode::Char('a')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::LogRoutings =>
            {
                self.open_create_log_routing()
            }
            KeyCode::Char('e')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::LogRoutings =>
            {
                self.open_edit_log_routing()
            }
            KeyCode::Char('d')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::LogRoutings =>
            {
                self.confirm_delete_log_routing()
            }
            KeyCode::Char('a') if self.monitoring.tab == MonitoringTab::MetricsRoutings => {
                self.open_create_metrics_routing()
            }
            KeyCode::Char('e') if self.monitoring.tab == MonitoringTab::MetricsRoutings => {
                self.open_edit_metrics_routing()
            }
            KeyCode::Char('d') if self.monitoring.tab == MonitoringTab::MetricsRoutings => {
                self.confirm_delete_metrics_routing()
            }
            KeyCode::Char('a') if self.monitoring.tab == MonitoringTab::Dashboards => {
                self.open_create_dashboard()
            }
            KeyCode::Char('e') if self.monitoring.tab == MonitoringTab::Dashboards => {
                self.open_edit_dashboard()
            }
            KeyCode::Char('d') if self.monitoring.tab == MonitoringTab::Dashboards => {
                self.confirm_delete_dashboard()
            }
            KeyCode::Char('a')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationTargets =>
            {
                self.open_create_notification_target()
            }
            KeyCode::Char('e')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationTargets =>
            {
                self.open_edit_notification_target()
            }
            KeyCode::Char('d')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationTargets =>
            {
                self.confirm_delete_notification_target()
            }
            KeyCode::Char('a')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationRoutings =>
            {
                self.open_create_notification_routing()
            }
            KeyCode::Char('e')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationRoutings =>
            {
                self.open_edit_notification_routing()
            }
            KeyCode::Char('d')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationRoutings =>
            {
                self.confirm_delete_notification_routing()
            }
            KeyCode::Char('[')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationRoutings =>
            {
                self.move_notification_routing(-1)
            }
            KeyCode::Char(']')
                if self.monitoring.focus == ListFocus::Right
                    && self.monitoring.tab == MonitoringTab::NotificationRoutings =>
            {
                self.move_notification_routing(1)
            }
            KeyCode::Char('n')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Left =>
            {
                self.open_create_storage()
            }
            KeyCode::Char('E')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Left =>
            {
                self.open_edit_storage()
            }
            KeyCode::Char('D')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Left =>
            {
                self.confirm_delete_storage()
            }
            KeyCode::Char('t')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Left =>
            {
                self.open_storage_retention()
            }
            KeyCode::Char('a')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Right =>
            {
                self.open_create_storage_access_key()
            }
            KeyCode::Char('e')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Right =>
            {
                self.open_edit_storage_access_key()
            }
            KeyCode::Char('d')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Right =>
            {
                self.confirm_delete_storage_access_key()
            }
            KeyCode::Char('u')
                if self.monitoring.tab == MonitoringTab::Storages
                    && self.monitoring.focus == ListFocus::Right =>
            {
                self.confirm_reveal_storage_access_key()
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

    fn open_create_log_measure_rule(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_project() else {
            self.set_status("プロジェクトを選択してください", StatusKind::Info);
            return;
        };
        let storages = self
            .monitoring
            .storages
            .get(&self.zone)
            .and_then(Loadable::ready);
        let log_storage_id = storages
            .and_then(|items| items.iter().find(|item| item.kind == StorageKind::Logs))
            .map(|storage| storage.resource_id.to_string())
            .unwrap_or_default();
        let metrics_storage_id = storages
            .and_then(|items| items.iter().find(|item| item.kind == StorageKind::Metrics))
            .map(|storage| storage.resource_id.to_string())
            .unwrap_or_default();
        self.overlay = Some(Overlay::LogMeasureRuleForm(LogMeasureRuleForm {
            mode: LogMeasureRuleFormMode::Create,
            project,
            target: None,
            log_storage_id,
            metrics_storage_id,
            name: String::new(),
            description: String::new(),
            rule_json: r#"{"version":"v1","query":{"matchers":[]}}"#.to_string(),
            field: 0,
        }));
    }

    fn open_edit_log_measure_rule(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(rule)) =
            (self.selected_project(), self.selected_log_measure_rule())
        else {
            self.set_status("ログ計測ルールを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::LogMeasureRuleForm(LogMeasureRuleForm {
            mode: LogMeasureRuleFormMode::Edit,
            project,
            log_storage_id: rule.log_storage_id.to_string(),
            metrics_storage_id: rule.metrics_storage_id.to_string(),
            name: rule.name.clone(),
            description: rule.description.clone(),
            rule_json: rule.rule.to_string(),
            target: Some(rule),
            field: 0,
        }));
    }

    fn confirm_delete_log_measure_rule(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(rule)) =
            (self.selected_project(), self.selected_log_measure_rule())
        else {
            self.set_status("ログ計測ルールを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "ログ計測ルールの削除".to_string(),
            body: format!(
                "ログ計測ルール「{}」を削除します。\nログからのメトリクス生成が停止します。",
                rule.name
            ),
            verify: Some(rule.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteLogMeasureRule {
                zone: self.zone.clone(),
                project: project.resource_id,
                uid: rule.uid,
                name: rule.name,
            },
        });
    }

    fn open_create_log_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let publishers = self.routing_publishers("logs");
        if publishers.is_empty() {
            self.set_status(
                "ログ用パブリッシャーを読み込み中です。少し待って再実行してください",
                StatusKind::Info,
            );
            return;
        }
        let log_storage_id = self
            .monitoring
            .storages
            .get(&self.zone)
            .and_then(Loadable::ready)
            .and_then(|items| items.iter().find(|item| item.kind == StorageKind::Logs))
            .map(|storage| storage.resource_id.to_string())
            .unwrap_or_default();
        self.overlay = Some(Overlay::LogRoutingForm(LogRoutingForm {
            mode: LogRoutingFormMode::Create,
            target: None,
            publisher_code: String::new(),
            variant: String::new(),
            resource_id: String::new(),
            log_storage_id,
            publishers,
            publisher_index: 0,
            variant_index: 0,
            field: 0,
        }));
    }

    fn open_edit_log_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(routing) = self.selected_log_routing() else {
            self.set_status("ログ転送設定を選択してください", StatusKind::Info);
            return;
        };
        let publishers = self.routing_publishers("logs");
        let publisher_index = publishers
            .iter()
            .position(|publisher| publisher.code == routing.publisher_code)
            .unwrap_or(0);
        let variant_index = publishers
            .get(publisher_index)
            .and_then(|publisher| {
                publisher
                    .variants
                    .iter()
                    .position(|variant| variant.name == routing.variant)
            })
            .unwrap_or(0);
        self.overlay = Some(Overlay::LogRoutingForm(LogRoutingForm {
            mode: LogRoutingFormMode::Edit,
            publisher_code: routing.publisher_code.clone(),
            variant: routing.variant.clone(),
            resource_id: routing
                .resource_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            log_storage_id: routing.log_storage_id.to_string(),
            publishers,
            publisher_index,
            variant_index,
            target: Some(routing),
            field: 0,
        }));
    }

    fn confirm_delete_log_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(routing) = self.selected_log_routing() else {
            self.set_status("ログ転送設定を選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "ログ転送設定の削除".to_string(),
            body: format!(
                "{} / {} のログ転送を削除します。\n対象リソースからのログ収集が停止します。",
                routing.publisher_code, routing.variant
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteLogRouting {
                zone: self.zone.clone(),
                routing,
            },
        });
    }

    fn open_create_metrics_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let publishers = self.routing_publishers("metrics");
        if publishers.is_empty() {
            self.set_status(
                "メトリクス用パブリッシャーを読み込み中です。少し待って再実行してください",
                StatusKind::Info,
            );
            return;
        }
        let metrics_storage_id = self
            .monitoring
            .storages
            .get(&self.zone)
            .and_then(Loadable::ready)
            .and_then(|items| items.iter().find(|item| item.kind == StorageKind::Metrics))
            .map(|storage| storage.resource_id.to_string())
            .unwrap_or_default();
        self.overlay = Some(Overlay::MetricsRoutingForm(MetricsRoutingForm {
            mode: MetricsRoutingFormMode::Create,
            target: None,
            publisher_code: String::new(),
            variant: String::new(),
            resource_id: String::new(),
            metrics_storage_id,
            publishers,
            publisher_index: 0,
            variant_index: 0,
            field: 0,
        }));
    }

    fn open_edit_metrics_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(routing) = self.selected_metrics_routing() else {
            return;
        };
        let publishers = self.routing_publishers("metrics");
        let publisher_index = publishers
            .iter()
            .position(|p| p.code == routing.publisher_code)
            .unwrap_or(0);
        let variant_index = publishers
            .get(publisher_index)
            .and_then(|p| p.variants.iter().position(|v| v.name == routing.variant))
            .unwrap_or(0);
        self.overlay = Some(Overlay::MetricsRoutingForm(MetricsRoutingForm {
            mode: MetricsRoutingFormMode::Edit,
            publisher_code: routing.publisher_code.clone(),
            variant: routing.variant.clone(),
            resource_id: routing
                .resource_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            metrics_storage_id: routing.metrics_storage_id.to_string(),
            target: Some(routing),
            publishers,
            publisher_index,
            variant_index,
            field: 0,
        }));
    }

    fn confirm_delete_metrics_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(routing) = self.selected_metrics_routing() else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "メトリクス転送設定の削除".to_string(),
            body: format!(
                "{} / {} のメトリクス転送を削除します。",
                routing.publisher_code, routing.variant
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteMetricsRouting {
                zone: self.zone.clone(),
                routing,
            },
        });
    }

    fn open_create_dashboard(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::DashboardForm(DashboardForm {
            mode: DashboardFormMode::Create,
            target: None,
            name: String::new(),
            description: String::new(),
            field: 0,
        }));
    }

    fn open_edit_dashboard(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_dashboard_project() else {
            return;
        };
        self.overlay = Some(Overlay::DashboardForm(DashboardForm {
            mode: DashboardFormMode::Edit,
            name: project.name.clone(),
            description: project.description.clone(),
            target: Some(project),
            field: 0,
        }));
    }

    fn confirm_delete_dashboard(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_dashboard_project() else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "ダッシュボードプロジェクトの削除".to_string(),
            body: format!(
                "ダッシュボードプロジェクト「{}」を削除します。",
                project.name
            ),
            verify: Some(project.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteDashboardProject {
                zone: self.zone.clone(),
                project,
            },
        });
    }

    fn open_create_notification_target(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_project() else {
            self.set_status("プロジェクトを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::NotificationTargetForm(NotificationTargetForm {
            mode: NotificationTargetFormMode::Create,
            project,
            target: None,
            service_type: 0,
            url: String::new(),
            description: String::new(),
            field: 0,
        }));
    }

    fn open_edit_notification_target(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(target)) =
            (self.selected_project(), self.selected_notification_target())
        else {
            self.set_status("通知先を選択してください", StatusKind::Info);
            return;
        };
        let service_type = NotificationTargetForm::SERVICE_TYPES
            .iter()
            .position(|value| *value == target.service_type)
            .unwrap_or(0);
        self.overlay = Some(Overlay::NotificationTargetForm(NotificationTargetForm {
            mode: NotificationTargetFormMode::Edit,
            project,
            service_type,
            url: target.url.clone(),
            description: target.description.clone(),
            target: Some(target),
            field: 0,
        }));
    }

    fn confirm_delete_notification_target(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(target)) =
            (self.selected_project(), self.selected_notification_target())
        else {
            self.set_status("通知先を選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "通知先の削除".to_string(),
            body: format!(
                "通知先 {} ({}) を削除します。\nこの通知先を参照する経路がある場合、APIが削除を拒否します。",
                notification_service_label(&target.service_type),
                target.description
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteNotificationTarget {
                zone: self.zone.clone(),
                project: project.resource_id,
                target,
            },
        });
    }

    fn routing_targets(&self, project: i64) -> Vec<NotificationTarget> {
        self.monitoring
            .notification_targets
            .get(&(self.zone.clone(), project))
            .and_then(Loadable::ready)
            .cloned()
            .unwrap_or_default()
    }

    fn open_create_notification_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(project) = self.selected_project() else {
            self.set_status("プロジェクトを選択してください", StatusKind::Info);
            return;
        };
        let targets = self.routing_targets(project.resource_id);
        if targets.is_empty() {
            self.set_status("先に通知先を作成してください", StatusKind::Info);
            return;
        }
        self.overlay = Some(Overlay::NotificationRoutingForm(NotificationRoutingForm {
            mode: NotificationRoutingFormMode::Create,
            project,
            target: None,
            targets,
            target_index: 0,
            resend_interval: "60".to_string(),
            match_labels: String::new(),
            field: 0,
        }));
    }

    fn open_edit_notification_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(routing)) = (
            self.selected_project(),
            self.selected_notification_routing(),
        ) else {
            self.set_status("通知経路を選択してください", StatusKind::Info);
            return;
        };
        let targets = self.routing_targets(project.resource_id);
        let target_index = targets
            .iter()
            .position(|target| target.uid == routing.target_uid)
            .unwrap_or(0);
        self.overlay = Some(Overlay::NotificationRoutingForm(NotificationRoutingForm {
            mode: NotificationRoutingFormMode::Edit,
            project,
            target_index,
            targets,
            resend_interval: routing
                .resend_interval_minutes
                .map(|minutes| minutes.to_string())
                .unwrap_or_default(),
            match_labels: format_match_labels(&routing.match_labels),
            target: Some(routing),
            field: 0,
        }));
    }

    fn confirm_delete_notification_routing(&mut self) {
        if !self.require_write() {
            return;
        }
        let (Some(project), Some(routing)) = (
            self.selected_project(),
            self.selected_notification_routing(),
        ) else {
            self.set_status("通知経路を選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "通知経路の削除".to_string(),
            body: format!(
                "{} 宛ての通知経路を削除します。\n一致するアラートはこの経路から通知されなくなります。",
                routing.target_description
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteNotificationRouting {
                zone: self.zone.clone(),
                project: project.resource_id,
                routing,
            },
        });
    }

    fn move_notification_routing(&mut self, delta: i32) {
        if !self.require_write() {
            return;
        }
        if !self.filters.get(Pane::NotificationRoutings).is_empty() {
            self.set_status("並べ替える前に絞り込みを解除してください", StatusKind::Info);
            return;
        }
        let (Some(project), Some(selected)) = (
            self.selected_project(),
            self.selected_notification_routing(),
        ) else {
            self.set_status("通知経路を選択してください", StatusKind::Info);
            return;
        };
        let cache_key = (self.zone.clone(), project.resource_id);
        let Some(mut routings) = self
            .monitoring
            .notification_routings
            .get(&cache_key)
            .and_then(Loadable::ready)
            .cloned()
        else {
            return;
        };
        routings.sort_by_key(|routing| routing.order);
        let Some(index) = routings
            .iter()
            .position(|routing| routing.uid == selected.uid)
        else {
            return;
        };
        let next =
            (index as i32 + delta).clamp(0, routings.len().saturating_sub(1) as i32) as usize;
        if index == next {
            return;
        }
        let mut slots = routings
            .iter()
            .map(|routing| routing.order)
            .collect::<Vec<_>>();
        if slots.windows(2).any(|pair| pair[0] >= pair[1]) {
            slots = (0..routings.len() as i64).collect();
        }
        routings.swap(index, next);
        let orders = routings
            .into_iter()
            .zip(slots)
            .map(|(routing, order)| (routing.uid, order))
            .collect::<Vec<_>>();
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let project_id = project.resource_id;
        self.inflight += 1;
        self.set_status("並べ替え中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .reorder_notification_routings(&zone, project_id, &orders)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::NotificationAction {
                zone,
                project: project_id,
                label: "通知経路を並べ替え".to_string(),
                result,
            });
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

    fn open_storage_retention(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(storage) = self.selected_storage() else {
            self.set_status("ストレージを選択してください", StatusKind::Info);
            return;
        };
        if storage.kind == StorageKind::Metrics {
            self.set_status("メトリクスストレージの保持期間は固定です", StatusKind::Info);
            return;
        }
        let days = storage
            .retention_days
            .map(|days| days.to_string())
            .unwrap_or_else(|| "30".to_string());
        self.overlay = Some(Overlay::StorageRetentionForm(StorageRetentionForm {
            storage,
            days,
        }));
    }

    fn open_create_storage_access_key(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(storage) = self.selected_storage() else {
            self.set_status("ストレージを選択してください", StatusKind::Info);
            return;
        };
        if !storage.supports_access_keys() {
            self.set_status(
                "システム領域のストレージではアクセスキーを利用できません",
                StatusKind::Info,
            );
            return;
        }
        self.overlay = Some(Overlay::StorageAccessKeyForm(StorageAccessKeyForm {
            mode: StorageAccessKeyFormMode::Create,
            storage,
            target: None,
            description: String::new(),
        }));
    }

    fn open_edit_storage_access_key(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(storage) = self.selected_storage() else {
            self.set_status("ストレージを選択してください", StatusKind::Info);
            return;
        };
        if !storage.supports_access_keys() {
            self.set_status(
                "システム領域のストレージではアクセスキーを利用できません",
                StatusKind::Info,
            );
            return;
        }
        let Some(key) = self.selected_storage_access_key() else {
            self.set_status("アクセスキーを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::StorageAccessKeyForm(StorageAccessKeyForm {
            mode: StorageAccessKeyFormMode::Edit,
            storage,
            description: key.description.clone(),
            target: Some(key),
        }));
    }

    fn confirm_delete_storage_access_key(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(storage) = self.selected_storage() else {
            self.set_status("ストレージを選択してください", StatusKind::Info);
            return;
        };
        if !storage.supports_access_keys() {
            self.set_status(
                "システム領域のストレージではアクセスキーを利用できません",
                StatusKind::Info,
            );
            return;
        }
        let Some(key) = self.selected_storage_access_key() else {
            self.set_status("アクセスキーを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "アクセスキーの削除".to_string(),
            body: format!(
                "{}ストレージ「{}」のアクセスキー {} を削除します。\nこのキーを使う送信処理は直ちに認証できなくなります。",
                storage.kind.label(),
                storage.name,
                key.uid
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteStorageAccessKey {
                zone: self.zone.clone(),
                storage,
                key,
            },
        });
    }

    fn confirm_reveal_storage_access_key(&mut self) {
        let Some(storage) = self.selected_storage() else {
            self.set_status("ストレージを選択してください", StatusKind::Info);
            return;
        };
        if !storage.supports_access_keys() {
            self.set_status(
                "システム領域のストレージではアクセスキーを利用できません",
                StatusKind::Info,
            );
            return;
        }
        let Some(key) = self.selected_storage_access_key() else {
            self.set_status("アクセスキーを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "アクセスキーのシークレットを表示".to_string(),
            body: format!(
                "{}ストレージ「{}」のアクセスキーの秘密情報を取得して画面に表示します。\n肩越しに覗かれていないか確認してください。",
                storage.kind.label(),
                storage.name
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::RevealStorageAccessKey {
                zone: self.zone.clone(),
                storage,
                key,
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

    pub(super) fn submit_log_measure_rule_form(&mut self, form: LogMeasureRuleForm) {
        let input = match form.input() {
            Ok(input) => input,
            Err(err) => {
                self.set_status(err, StatusKind::Error);
                self.overlay = Some(Overlay::LogMeasureRuleForm(form));
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
            LogMeasureRuleFormMode::Create => tokio::spawn(async move {
                let result = client
                    .create_log_measure_rule(&zone, project, &input)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::LogMeasureRuleAction {
                    zone,
                    project,
                    label: format!("ログ計測ルール「{name}」を作成"),
                    result,
                });
            }),
            LogMeasureRuleFormMode::Edit => {
                let Some(rule) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let result = client
                        .update_log_measure_rule(&zone, project, &rule.uid, &input)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::LogMeasureRuleAction {
                        zone,
                        project,
                        label: format!("ログ計測ルール「{name}」を更新"),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_log_measure_rule(
        &mut self,
        zone: String,
        project: i64,
        uid: String,
        name: String,
    ) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_log_measure_rule(&zone, project, &uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::LogMeasureRuleAction {
                zone,
                project,
                label: format!("ログ計測ルール「{name}」を削除"),
                result,
            });
        });
    }

    pub(super) fn submit_log_routing_form(&mut self, form: LogRoutingForm) {
        let input = match form.input() {
            Ok(input) => input,
            Err(err) => {
                self.set_status(err, StatusKind::Error);
                self.overlay = Some(Overlay::LogRoutingForm(form));
                return;
            }
        };
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let description = format!("{} / {}", input.publisher_code, input.variant);
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            LogRoutingFormMode::Create => tokio::spawn(async move {
                let result = client
                    .create_log_routing(&zone, &input)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::LogRoutingAction {
                    zone,
                    label: format!("ログ転送「{description}」を作成"),
                    result,
                });
            }),
            LogRoutingFormMode::Edit => {
                let Some(target) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let result = client
                        .update_log_routing(&zone, &target.uid, &input)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::LogRoutingAction {
                        zone,
                        label: format!("ログ転送「{description}」を更新"),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_log_routing(&mut self, zone: String, routing: LogRouting) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let label = format!(
            "ログ転送「{} / {}」を削除",
            routing.publisher_code, routing.variant
        );
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_log_routing(&zone, &routing.uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::LogRoutingAction {
                zone,
                label,
                result,
            });
        });
    }

    pub(super) fn submit_metrics_routing_form(&mut self, form: MetricsRoutingForm) {
        let input = match form.input() {
            Ok(input) => input,
            Err(err) => {
                self.set_status(err, StatusKind::Error);
                self.overlay = Some(Overlay::MetricsRoutingForm(form));
                return;
            }
        };
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let label = format!(
            "メトリクス転送「{} / {}」",
            input.publisher_code, input.variant
        );
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            MetricsRoutingFormMode::Create => tokio::spawn(async move {
                let result = client
                    .create_metrics_routing(&zone, &input)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::MetricsRoutingAction {
                    zone,
                    label: format!("{label}を作成"),
                    result,
                });
            }),
            MetricsRoutingFormMode::Edit => {
                let Some(target) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    return;
                };
                tokio::spawn(async move {
                    let result = client
                        .update_metrics_routing(&zone, &target.uid, &input)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::MetricsRoutingAction {
                        zone,
                        label: format!("{label}を更新"),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_metrics_routing(&mut self, zone: String, routing: MetricsRouting) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        tokio::spawn(async move {
            let result = client
                .delete_metrics_routing(&zone, &routing.uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::MetricsRoutingAction {
                zone,
                label: "メトリクス転送を削除".to_string(),
                result,
            });
        });
    }

    pub(super) fn submit_dashboard_form(&mut self, form: DashboardForm) {
        let name = form.name.trim().to_string();
        let description = form.description.trim().to_string();
        if name.is_empty() {
            self.set_status("名前を入力してください", StatusKind::Error);
            self.overlay = Some(Overlay::DashboardForm(form));
            return;
        }
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        self.inflight += 1;
        match form.mode {
            DashboardFormMode::Create => tokio::spawn(async move {
                let result = client
                    .create_dashboard_project(&zone, &name, &description)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::DashboardAction {
                    zone,
                    label: format!("ダッシュボード「{name}」を作成"),
                    result,
                });
            }),
            DashboardFormMode::Edit => {
                let Some(target) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    return;
                };
                tokio::spawn(async move {
                    let result = client
                        .update_dashboard_project(&zone, target.resource_id, &name, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::DashboardAction {
                        zone,
                        label: format!("ダッシュボード「{name}」を更新"),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_dashboard(&mut self, zone: String, project: DashboardProject) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        tokio::spawn(async move {
            let result = client
                .delete_dashboard_project(&zone, project.resource_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::DashboardAction {
                zone,
                label: format!("ダッシュボード「{}」を削除", project.name),
                result,
            });
        });
    }

    pub(super) fn submit_notification_target_form(&mut self, mut form: NotificationTargetForm) {
        form.url = form.url.trim().to_string();
        form.description = form.description.trim().to_string();
        if !form.url.is_empty() {
            let valid = reqwest::Url::parse(&form.url)
                .is_ok_and(|url| matches!(url.scheme(), "http" | "https"));
            if !valid {
                self.set_status(
                    "URLは http:// または https:// で入力してください",
                    StatusKind::Error,
                );
                self.overlay = Some(Overlay::NotificationTargetForm(form));
                return;
            }
        }
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let project = form.project.resource_id;
        let service_type = form.service_type().to_string();
        let url = form.url;
        let description = form.description;
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            NotificationTargetFormMode::Create => tokio::spawn(async move {
                let result = client
                    .create_notification_target(&zone, project, &service_type, &url, &description)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::NotificationAction {
                    zone,
                    project,
                    label: "通知先を作成".to_string(),
                    result,
                });
            }),
            NotificationTargetFormMode::Edit => {
                let Some(target) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let result = client
                        .update_notification_target(
                            &zone,
                            project,
                            &target.uid,
                            &service_type,
                            &url,
                            &description,
                        )
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::NotificationAction {
                        zone,
                        project,
                        label: "通知先を更新".to_string(),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn submit_notification_routing_form(&mut self, form: NotificationRoutingForm) {
        let Some(target_uid) = form.selected_target().map(|target| target.uid.clone()) else {
            self.set_status("通知先を選択してください", StatusKind::Error);
            self.overlay = Some(Overlay::NotificationRoutingForm(form));
            return;
        };
        let resend_interval = if form.resend_interval.trim().is_empty() {
            None
        } else {
            match form.resend_interval.trim().parse::<i64>() {
                Ok(minutes) if minutes > 0 => Some(minutes),
                _ => {
                    self.set_status(
                        "再送間隔は1分以上の整数で入力してください",
                        StatusKind::Error,
                    );
                    self.overlay = Some(Overlay::NotificationRoutingForm(form));
                    return;
                }
            }
        };
        let match_labels = match parse_match_labels(&form.match_labels) {
            Ok(labels) => labels,
            Err(err) => {
                self.set_status(err, StatusKind::Error);
                self.overlay = Some(Overlay::NotificationRoutingForm(form));
                return;
            }
        };
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let project = form.project.resource_id;
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            NotificationRoutingFormMode::Create => tokio::spawn(async move {
                let result = client
                    .create_notification_routing(
                        &zone,
                        project,
                        &target_uid,
                        &match_labels,
                        resend_interval,
                    )
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::NotificationAction {
                    zone,
                    project,
                    label: "通知経路を作成".to_string(),
                    result,
                });
            }),
            NotificationRoutingFormMode::Edit => {
                let Some(routing) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let result = client
                        .update_notification_routing(
                            &zone,
                            project,
                            &routing.uid,
                            &target_uid,
                            &match_labels,
                            resend_interval,
                        )
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::NotificationAction {
                        zone,
                        project,
                        label: "通知経路を更新".to_string(),
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_delete_notification_target(
        &mut self,
        zone: String,
        project: i64,
        target: NotificationTarget,
    ) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_notification_target(&zone, project, &target.uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::NotificationAction {
                zone,
                project,
                label: "通知先を削除".to_string(),
                result,
            });
        });
    }

    pub(super) fn run_delete_notification_routing(
        &mut self,
        zone: String,
        project: i64,
        routing: NotificationRouting,
    ) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_notification_routing(&zone, project, &routing.uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::NotificationAction {
                zone,
                project,
                label: "通知経路を削除".to_string(),
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

    pub(super) fn submit_storage_retention_form(&mut self, form: StorageRetentionForm) {
        let Ok(days) = form.days.trim().parse::<i64>() else {
            self.set_status(
                "保持期間は1日以上の整数で入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::StorageRetentionForm(form));
            return;
        };
        if days <= 0 {
            self.set_status(
                "保持期間は1日以上の整数で入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::StorageRetentionForm(form));
            return;
        }
        let extra = if days >= 41 {
            "\n\n41日以上は追加料金が発生します。"
        } else {
            ""
        };
        self.overlay = Some(Overlay::Confirm {
            title: "ストレージ保持期間の変更".to_string(),
            body: format!(
                "{}ストレージ「{}」の保持期間を {} 日に変更します。{}",
                form.storage.kind.label(),
                form.storage.name,
                days,
                extra
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::SetStorageRetention {
                zone: self.zone.clone(),
                storage: form.storage,
                days,
            },
        });
    }

    pub(super) fn submit_storage_access_key_form(&mut self, form: StorageAccessKeyForm) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let storage = form.storage;
        let description = form.description.trim().to_string();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        match form.mode {
            StorageAccessKeyFormMode::Create => tokio::spawn(async move {
                let result = client
                    .create_storage_access_key(&zone, &storage, &description)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::StorageAccessKeySecret {
                    zone,
                    storage,
                    title: "アクセスキーを作成しました".to_string(),
                    result,
                });
            }),
            StorageAccessKeyFormMode::Edit => {
                let Some(key) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    self.set_status("更新対象がありません", StatusKind::Error);
                    return;
                };
                tokio::spawn(async move {
                    let label = "アクセスキーの説明を更新".to_string();
                    let result = client
                        .update_storage_access_key(&zone, &storage, &key.uid, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::StorageAccessKeyAction {
                        zone,
                        storage,
                        label,
                        result,
                    });
                })
            }
        };
    }

    pub(super) fn run_set_storage_retention(&mut self, zone: String, storage: Storage, days: i64) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        let label = format!(
            "{}ストレージの保持期間を {days} 日に変更",
            storage.kind.label()
        );
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .set_storage_retention(&zone, &storage, days)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::StorageAction {
                zone,
                label,
                result,
            });
        });
    }

    pub(super) fn run_delete_storage_access_key(
        &mut self,
        zone: String,
        storage: Storage,
        key: StorageAccessKey,
    ) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .delete_storage_access_key(&zone, &storage, &key.uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::StorageAccessKeyAction {
                zone,
                storage,
                label: "アクセスキーを削除".to_string(),
                result,
            });
        });
    }

    pub(super) fn run_reveal_storage_access_key(
        &mut self,
        zone: String,
        storage: Storage,
        key: StorageAccessKey,
    ) {
        let client = self.monitoring_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("取得中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = client
                .read_storage_access_key_secret(&zone, &storage, &key.uid)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::StorageAccessKeySecret {
                zone,
                storage,
                title: "アクセスキーの秘密情報".to_string(),
                result,
            });
        });
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
        self.monitoring.focus = if tab == MonitoringTab::Storages {
            ListFocus::Left
        } else {
            ListFocus::Right
        };
    }

    fn cycle_monitoring_tab(&mut self, delta: i32) {
        let current = MonitoringTab::ALL
            .iter()
            .position(|t| *t == self.monitoring.tab)
            .unwrap_or(0) as i32;
        let len = MonitoringTab::ALL.len() as i32;
        self.monitoring.tab = MonitoringTab::ALL[(current + delta).rem_euclid(len) as usize];
        self.monitoring.focus = if self.monitoring.tab == MonitoringTab::Storages {
            ListFocus::Left
        } else {
            ListFocus::Right
        };
    }
}
