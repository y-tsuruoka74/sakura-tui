//! クラウドHSM画面の状態と操作。
//!
//! クライアントは HSM を、ドキュメントはライセンスを親とする子リソース。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, child_id_to_load, fmt_error, matches};
use crate::cloudhsm::{CloudHsm, CloudHsmClient, CloudHsmDocument, CloudHsmLicense};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudHsmTab {
    #[default]
    Hsms,
    Clients,
    Licenses,
    Documents,
}

impl CloudHsmTab {
    pub const ALL: [CloudHsmTab; 4] = [
        CloudHsmTab::Hsms,
        CloudHsmTab::Clients,
        CloudHsmTab::Licenses,
        CloudHsmTab::Documents,
    ];

    pub fn title(self) -> &'static str {
        match self {
            CloudHsmTab::Hsms => "HSM",
            CloudHsmTab::Clients => "クライアント",
            CloudHsmTab::Licenses => "ライセンス",
            CloudHsmTab::Documents => "ドキュメント",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Default)]
pub struct CloudHsmView {
    pub tab: CloudHsmTab,
    /// ゾーンごとの HSM 一覧。キーはゾーン名。
    pub hsms: HashMap<String, Loadable<Vec<CloudHsm>>>,
    pub hsm_state: TableState,
    /// HSM ごとのクライアント。キーは HSM の ID。
    pub clients: HashMap<String, Loadable<Vec<CloudHsmClient>>>,
    pub client_state: TableState,
    /// ゾーンごとのライセンス一覧。
    pub licenses: HashMap<String, Loadable<Vec<CloudHsmLicense>>>,
    pub license_state: TableState,
    /// ライセンスごとのドキュメント。キーはライセンスの ID。
    pub documents: HashMap<String, Loadable<Vec<CloudHsmDocument>>>,
    pub document_state: TableState,
}

impl App {
    fn cloudhsm_hsms(&self) -> Loadable<Vec<CloudHsm>> {
        self.cloudhsm
            .hsms
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    fn cloudhsm_licenses(&self) -> Loadable<Vec<CloudHsmLicense>> {
        self.cloudhsm
            .licenses
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_cloudhsm_hsms(&self) -> Loadable<Vec<CloudHsm>> {
        let loadable = self.cloudhsm_hsms();
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::CloudHsmHsms);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.description,
                            &item.tags.join(","),
                            &item.availability,
                            &item.ipv4_address,
                            &item.network_label(),
                            &item.service_class,
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_cloudhsm_hsm(&self) -> Option<CloudHsm> {
        let index = self.cloudhsm.hsm_state.selected()?;
        self.visible_cloudhsm_hsms().ready()?.get(index).cloned()
    }

    pub fn visible_cloudhsm_clients(&self) -> Loadable<Vec<CloudHsmClient>> {
        let Some(hsm) = self.selected_cloudhsm_hsm() else {
            return Loadable::Idle;
        };
        let loadable = self
            .cloudhsm
            .clients
            .get(&hsm.id)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::CloudHsmClients);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| matches(filter, &[&item.id, &item.name, &item.availability]))
                .collect(),
        )
    }

    pub fn selected_cloudhsm_client(&self) -> Option<CloudHsmClient> {
        let index = self.cloudhsm.client_state.selected()?;
        self.visible_cloudhsm_clients().ready()?.get(index).cloned()
    }

    pub fn visible_cloudhsm_licenses(&self) -> Loadable<Vec<CloudHsmLicense>> {
        let loadable = self.cloudhsm_licenses();
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::CloudHsmLicenses);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.description,
                            &item.tags.join(","),
                            &item.service_class,
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_cloudhsm_license(&self) -> Option<CloudHsmLicense> {
        let index = self.cloudhsm.license_state.selected()?;
        self.visible_cloudhsm_licenses()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_cloudhsm_documents(&self) -> Loadable<Vec<CloudHsmDocument>> {
        let Some(license) = self.selected_cloudhsm_license() else {
            return Loadable::Idle;
        };
        let loadable = self
            .cloudhsm
            .documents
            .get(&license.id)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::CloudHsmDocuments);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| matches(filter, &[&item.id, &item.name, &item.license_id]))
                .collect(),
        )
    }

    pub fn selected_cloudhsm_document(&self) -> Option<CloudHsmDocument> {
        let index = self.cloudhsm.document_state.selected()?;
        self.visible_cloudhsm_documents()
            .ready()?
            .get(index)
            .cloned()
    }

    pub(super) fn cloudhsm_ensure_loaded(&mut self) {
        if self.cloudhsm_hsms().is_idle() {
            self.load_cloudhsm_hsms();
        } else {
            self.fill_selection(Pane::CloudHsmHsms);
        }
        if self.cloudhsm_licenses().is_idle() {
            self.load_cloudhsm_licenses();
        } else {
            self.fill_selection(Pane::CloudHsmLicenses);
        }

        let hsm_id = self.selected_cloudhsm_hsm().map(|hsm| hsm.id);
        if let Some(id) = child_id_to_load(hsm_id.clone(), &self.cloudhsm.clients) {
            self.load_cloudhsm_clients(id);
        } else if hsm_id.is_some() {
            self.fill_selection(Pane::CloudHsmClients);
        }

        let license_id = self.selected_cloudhsm_license().map(|license| license.id);
        if let Some(id) = child_id_to_load(license_id.clone(), &self.cloudhsm.documents) {
            self.load_cloudhsm_documents(id);
        } else if license_id.is_some() {
            self.fill_selection(Pane::CloudHsmDocuments);
        }
    }

    fn load_cloudhsm_hsms(&mut self) {
        let zone = self.zone.clone();
        self.cloudhsm.hsms.insert(zone.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_cloudhsms(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::CloudHsmHsms { zone, result });
        });
    }

    fn load_cloudhsm_licenses(&mut self) {
        let zone = self.zone.clone();
        self.cloudhsm
            .licenses
            .insert(zone.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .list_cloudhsm_licenses(&zone)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::CloudHsmLicenses { zone, result });
        });
    }

    fn load_cloudhsm_clients(&mut self, hsm_id: String) {
        self.cloudhsm
            .clients
            .insert(hsm_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client
                .list_cloudhsm_clients(&zone, &hsm_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::CloudHsmClients { hsm_id, result });
        });
    }

    fn load_cloudhsm_documents(&mut self, license_id: String) {
        self.cloudhsm
            .documents
            .insert(license_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client
                .list_cloudhsm_documents(&zone, &license_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::CloudHsmDocuments { license_id, result });
        });
    }

    pub(super) fn on_key_cloudhsm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.cycle_cloudhsm_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_cloudhsm_tab(1),
            KeyCode::Char('1') => self.cloudhsm.tab = CloudHsmTab::Hsms,
            KeyCode::Char('2') => self.cloudhsm.tab = CloudHsmTab::Clients,
            KeyCode::Char('3') => self.cloudhsm.tab = CloudHsmTab::Licenses,
            KeyCode::Char('4') => self.cloudhsm.tab = CloudHsmTab::Documents,
            _ => {}
        }
    }

    fn cycle_cloudhsm_tab(&mut self, delta: i32) {
        self.cloudhsm.tab = self.cloudhsm.tab.cycled(delta);
    }

    pub(super) fn cloudhsm_refresh(&mut self) {
        // 現在ゾーンだけ捨てる。他ゾーンのキャッシュは有効なまま残す。
        self.cloudhsm.hsms.remove(&self.zone);
        self.cloudhsm.licenses.remove(&self.zone);
        self.cloudhsm.clients.clear();
        self.cloudhsm.documents.clear();
        self.cloudhsm.hsm_state.select(None);
        self.cloudhsm.license_state.select(None);
        self.cloudhsm.client_state.select(None);
        self.cloudhsm.document_state.select(None);
        self.cloudhsm_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// タブの並びと巡回。両端で折り返すこと。
    #[test]
    fn tabs_cycle_in_order_and_wrap() {
        let titles: Vec<&str> = CloudHsmTab::ALL.iter().map(|tab| tab.title()).collect();
        assert_eq!(
            titles,
            vec!["HSM", "クライアント", "ライセンス", "ドキュメント"]
        );

        assert_eq!(CloudHsmTab::Hsms.cycled(1), CloudHsmTab::Clients);
        assert_eq!(CloudHsmTab::Documents.cycled(1), CloudHsmTab::Hsms);
        assert_eq!(CloudHsmTab::Hsms.cycled(-1), CloudHsmTab::Documents);
    }

    /// 既定は HSM 一覧。
    #[test]
    fn default_tab_is_the_hsm_list() {
        assert_eq!(CloudHsmTab::default(), CloudHsmTab::Hsms);
    }
}
