//! NoSQL 画面の状態と操作。
//!
//! ノードタブだけは 2 つの API を合成する。`/nosql/nodes/health` は全体の
//! 健全性しか返さず、ノード個別の情報は `/appliance/{id}/status` 側にある。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, child_id_to_load, fmt_error, matches};
use crate::nosql::{
    NoSqlBackup, NoSqlDatabase, NoSqlNode, NoSqlNodeHealth, NoSqlParameter, NoSqlStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoSqlTab {
    #[default]
    Databases,
    Nodes,
    Backups,
    Parameters,
}

impl NoSqlTab {
    pub const ALL: [NoSqlTab; 4] = [
        NoSqlTab::Databases,
        NoSqlTab::Nodes,
        NoSqlTab::Backups,
        NoSqlTab::Parameters,
    ];

    pub fn title(self) -> &'static str {
        match self {
            NoSqlTab::Databases => "DB",
            NoSqlTab::Nodes => "ノード",
            NoSqlTab::Backups => "バックアップ",
            NoSqlTab::Parameters => "パラメータ",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Default)]
pub struct NoSqlView {
    pub tab: NoSqlTab,
    pub databases: Loadable<Vec<NoSqlDatabase>>,
    pub database_state: TableState,
    /// DB ごとのノード構成とバージョン。
    pub statuses: HashMap<String, Loadable<NoSqlStatus>>,
    pub node_state: TableState,
    /// DB ごとの全体健全性。ノードタブのヘッダに出す。
    pub healths: HashMap<String, Loadable<NoSqlNodeHealth>>,
    pub backups: HashMap<String, Loadable<Vec<NoSqlBackup>>>,
    pub backup_state: TableState,
    pub parameters: HashMap<String, Loadable<Vec<NoSqlParameter>>>,
    pub parameter_state: TableState,
}

impl App {
    /// NoSQL の問い合わせ先ゾーン。
    ///
    /// 東京第2ゾーン限定でゾーン切り替えの対象外にしているため、
    /// どこを見ているかが隠れないよう画面に出す。
    pub fn nosql_zone(&self) -> &str {
        self.sacloud.nosql_zone()
    }

    pub fn visible_nosql_databases(&self) -> Loadable<Vec<NoSqlDatabase>> {
        let Loadable::Ready(items) = &self.nosql.databases else {
            return self.nosql.databases.clone();
        };
        let filter = self.filters.get(Pane::NoSqlDatabases);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.description,
                            &item.tags.join(","),
                            &item.status_label(),
                            &item.plan.label(),
                            &item.version,
                            &item.engine,
                            &item.ip_addresses.join(","),
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_nosql_database(&self) -> Option<NoSqlDatabase> {
        let index = self.nosql.database_state.selected()?;
        self.visible_nosql_databases().ready()?.get(index).cloned()
    }

    /// 選択中 DB の状態。ノードタブのヘッダに使う。
    pub fn selected_nosql_status(&self) -> Loadable<NoSqlStatus> {
        let Some(database) = self.selected_nosql_database() else {
            return Loadable::Idle;
        };
        self.nosql
            .statuses
            .get(&database.id)
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    /// 選択中 DB の全体健全性。
    pub fn selected_nosql_node_health(&self) -> Loadable<NoSqlNodeHealth> {
        let Some(database) = self.selected_nosql_database() else {
            return Loadable::Idle;
        };
        self.nosql
            .healths
            .get(&database.id)
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_nosql_nodes(&self) -> Loadable<Vec<NoSqlNode>> {
        // ノードは状態APIの中に入っているため、読み込み状態はそちらへ従わせる。
        let status = match self.selected_nosql_status() {
            Loadable::Ready(status) => status,
            Loadable::Idle => return Loadable::Idle,
            Loadable::Loading => return Loadable::Loading,
            Loadable::Failed(err) => return Loadable::Failed(err),
        };
        let filter = self.filters.get(Pane::NoSqlNodes);
        Loadable::Ready(
            status
                .nodes
                .into_iter()
                .filter(|node| {
                    matches(
                        filter,
                        &[
                            &node.index.to_string(),
                            &node.ip_address,
                            &node.node_type_label(),
                            node.group_label(),
                            &node.appliance_id,
                            &node.availability,
                            &node.zone,
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_nosql_node(&self) -> Option<NoSqlNode> {
        let index = self.nosql.node_state.selected()?;
        self.visible_nosql_nodes().ready()?.get(index).cloned()
    }

    pub fn visible_nosql_backups(&self) -> Loadable<Vec<NoSqlBackup>> {
        let Some(database) = self.selected_nosql_database() else {
            return Loadable::Idle;
        };
        let loadable = self
            .nosql
            .backups
            .get(&database.id)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::NoSqlBackups);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.destination,
                            &item.backup_at,
                            &item.restore_at,
                            &item.delete_status_label(),
                            &item.restore_status_label(),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_nosql_backup(&self) -> Option<NoSqlBackup> {
        let index = self.nosql.backup_state.selected()?;
        self.visible_nosql_backups().ready()?.get(index).cloned()
    }

    pub fn visible_nosql_parameters(&self) -> Loadable<Vec<NoSqlParameter>> {
        let Some(database) = self.selected_nosql_database() else {
            return Loadable::Idle;
        };
        let loadable = self
            .nosql
            .parameters
            .get(&database.id)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::NoSqlParameters);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.value,
                            &item.default_value,
                            &item.description,
                            &item.options.join(","),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_nosql_parameter(&self) -> Option<NoSqlParameter> {
        let index = self.nosql.parameter_state.selected()?;
        self.visible_nosql_parameters().ready()?.get(index).cloned()
    }

    pub(super) fn nosql_ensure_loaded(&mut self) {
        if self.nosql.databases.is_idle() {
            self.load_nosql_databases();
        } else {
            self.fill_selection(Pane::NoSqlDatabases);
        }

        let selected_id = self.selected_nosql_database().map(|database| database.id);
        let Some(database_id) = selected_id else {
            return;
        };

        if let Some(id) = child_id_to_load(Some(database_id.clone()), &self.nosql.statuses) {
            self.load_nosql_status(id);
        } else {
            self.fill_selection(Pane::NoSqlNodes);
        }
        if let Some(id) = child_id_to_load(Some(database_id.clone()), &self.nosql.healths) {
            self.load_nosql_node_health(id);
        }
        if let Some(id) = child_id_to_load(Some(database_id.clone()), &self.nosql.backups) {
            self.load_nosql_backups(id);
        } else {
            self.fill_selection(Pane::NoSqlBackups);
        }
        if let Some(id) = child_id_to_load(Some(database_id), &self.nosql.parameters) {
            self.load_nosql_parameters(id);
        } else {
            self.fill_selection(Pane::NoSqlParameters);
        }
    }

    fn load_nosql_databases(&mut self) {
        self.nosql.databases = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_nosql_databases().await.map_err(fmt_error);
            let _ = tx.send(Message::NoSqlDatabases { result });
        });
    }

    fn load_nosql_status(&mut self, database_id: String) {
        self.nosql
            .statuses
            .insert(database_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.nosql_status(&database_id).await.map_err(fmt_error);
            let _ = tx.send(Message::NoSqlStatus {
                database_id,
                result,
            });
        });
    }

    fn load_nosql_node_health(&mut self, database_id: String) {
        self.nosql
            .healths
            .insert(database_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .nosql_node_health(&database_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::NoSqlNodeHealth {
                database_id,
                result,
            });
        });
    }

    fn load_nosql_backups(&mut self, database_id: String) {
        self.nosql
            .backups
            .insert(database_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .list_nosql_backups(&database_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::NoSqlBackups {
                database_id,
                result,
            });
        });
    }

    fn load_nosql_parameters(&mut self, database_id: String) {
        self.nosql
            .parameters
            .insert(database_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .list_nosql_parameters(&database_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::NoSqlParameters {
                database_id,
                result,
            });
        });
    }

    pub(super) fn on_key_nosql(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.cycle_nosql_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_nosql_tab(1),
            KeyCode::Char('1') => self.nosql.tab = NoSqlTab::Databases,
            KeyCode::Char('2') => self.nosql.tab = NoSqlTab::Nodes,
            KeyCode::Char('3') => self.nosql.tab = NoSqlTab::Backups,
            KeyCode::Char('4') => self.nosql.tab = NoSqlTab::Parameters,
            _ => {}
        }
    }

    fn cycle_nosql_tab(&mut self, delta: i32) {
        self.nosql.tab = self.nosql.tab.cycled(delta);
    }

    /// 選択中の DB が変わったら、その DB にぶら下がる選択位置を捨てる。
    pub(super) fn nosql_reset_child_selection(&mut self) {
        self.nosql.node_state.select(None);
        self.nosql.backup_state.select(None);
        self.nosql.parameter_state.select(None);
    }

    pub(super) fn nosql_refresh(&mut self) {
        self.nosql.databases = Loadable::Idle;
        self.nosql.statuses.clear();
        self.nosql.healths.clear();
        self.nosql.backups.clear();
        self.nosql.parameters.clear();
        self.nosql.database_state.select(None);
        self.nosql_reset_child_selection();
        self.nosql_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// タブの並びと巡回。両端で折り返すこと。
    #[test]
    fn tabs_cycle_in_order_and_wrap() {
        let titles: Vec<&str> = NoSqlTab::ALL.iter().map(|tab| tab.title()).collect();
        assert_eq!(titles, vec!["DB", "ノード", "バックアップ", "パラメータ"]);

        assert_eq!(NoSqlTab::Databases.cycled(1), NoSqlTab::Nodes);
        assert_eq!(NoSqlTab::Parameters.cycled(1), NoSqlTab::Databases);
        assert_eq!(NoSqlTab::Databases.cycled(-1), NoSqlTab::Parameters);
    }

    /// 既定は DB タブ。
    #[test]
    fn default_tab_is_the_database_list() {
        assert_eq!(NoSqlTab::default(), NoSqlTab::Databases);
    }
}
