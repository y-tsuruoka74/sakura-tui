//! ネットワークスイート (CR) 画面の状態と操作。
//!
//! サブネットグループ → サブネット → アドレス の3階層。
//! 一覧APIが親の SRN を要求するので、子は選択中の親の分だけ取りに行く。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, child_id_to_load, fmt_error, matches};
use crate::networking_suite::{Subnet, SubnetAddress, SubnetGroup};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkingSuiteTab {
    #[default]
    Groups,
    Subnets,
    Addresses,
}

impl NetworkingSuiteTab {
    pub const ALL: [NetworkingSuiteTab; 3] = [
        NetworkingSuiteTab::Groups,
        NetworkingSuiteTab::Subnets,
        NetworkingSuiteTab::Addresses,
    ];

    pub fn title(self) -> &'static str {
        match self {
            NetworkingSuiteTab::Groups => "サブネットグループ",
            NetworkingSuiteTab::Subnets => "サブネット",
            NetworkingSuiteTab::Addresses => "アドレス",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Default)]
pub struct NetworkingSuiteView {
    pub tab: NetworkingSuiteTab,
    /// サブネットグループ一覧。受付ゾーンが固定なのでゾーン別に持たない。
    pub groups: Loadable<Vec<SubnetGroup>>,
    pub group_state: TableState,
    /// サブネットグループごとのサブネット。キーはグループの SRN。
    pub subnets: HashMap<String, Loadable<Vec<Subnet>>>,
    pub subnet_state: TableState,
    /// サブネットごとのアドレス。キーはサブネットの SRN。
    pub addresses: HashMap<String, Loadable<Vec<SubnetAddress>>>,
    pub address_state: TableState,
}

impl App {
    /// ネットワークスイートの問い合わせ先ゾーン。
    ///
    /// 本番では is1c 固定でゾーン切り替えの対象外にしているため、
    /// どこを見ているかが隠れないよう画面に出す。
    pub fn networking_suite_zone(&self) -> &str {
        self.sacloud.networking_suite_zone()
    }

    pub fn visible_networking_suite_groups(&self) -> Loadable<Vec<SubnetGroup>> {
        let Loadable::Ready(items) = &self.networking_suite.groups else {
            return self.networking_suite.groups.clone();
        };
        let items = items.clone();
        let filter = self.filters.get(Pane::NetworkingSuiteGroups);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.srn,
                            &item.id(),
                            &item.name,
                            &item.description,
                            &item.cidr,
                            &item.region,
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_networking_suite_group(&self) -> Option<SubnetGroup> {
        let index = self.networking_suite.group_state.selected()?;
        self.visible_networking_suite_groups()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_networking_suite_subnets(&self) -> Loadable<Vec<Subnet>> {
        let Some(group) = self.selected_networking_suite_group() else {
            return Loadable::Idle;
        };
        let loadable = self
            .networking_suite
            .subnets
            .get(&group.srn)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::NetworkingSuiteSubnets);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.srn,
                            &item.id(),
                            &item.name,
                            &item.description,
                            &item.cidr,
                            &item.zone,
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_networking_suite_subnet(&self) -> Option<Subnet> {
        let index = self.networking_suite.subnet_state.selected()?;
        self.visible_networking_suite_subnets()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_networking_suite_addresses(&self) -> Loadable<Vec<SubnetAddress>> {
        let Some(subnet) = self.selected_networking_suite_subnet() else {
            return Loadable::Idle;
        };
        let loadable = self
            .networking_suite
            .addresses
            .get(&subnet.srn)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::NetworkingSuiteAddresses);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.srn,
                            &item.id(),
                            &item.ip_address,
                            &item.ip_version,
                            &item.address_type_label(),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_networking_suite_address(&self) -> Option<SubnetAddress> {
        let index = self.networking_suite.address_state.selected()?;
        self.visible_networking_suite_addresses()
            .ready()?
            .get(index)
            .cloned()
    }

    pub(super) fn networking_suite_ensure_loaded(&mut self) {
        if self.networking_suite.groups.is_idle() {
            self.load_networking_suite_groups();
        } else {
            self.fill_selection(Pane::NetworkingSuiteGroups);
        }

        let group_srn = self.selected_networking_suite_group().map(|g| g.srn);
        if let Some(srn) = child_id_to_load(group_srn.clone(), &self.networking_suite.subnets) {
            self.load_networking_suite_subnets(srn);
        } else if group_srn.is_some() {
            self.fill_selection(Pane::NetworkingSuiteSubnets);
        }

        let subnet_srn = self.selected_networking_suite_subnet().map(|s| s.srn);
        if let Some(srn) = child_id_to_load(subnet_srn.clone(), &self.networking_suite.addresses) {
            self.load_networking_suite_addresses(srn);
        } else if subnet_srn.is_some() {
            self.fill_selection(Pane::NetworkingSuiteAddresses);
        }
    }

    fn load_networking_suite_groups(&mut self) {
        self.networking_suite.groups = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_subnet_groups().await.map_err(fmt_error);
            let _ = tx.send(Message::NetworkingSuiteGroups { result });
        });
    }

    fn load_networking_suite_subnets(&mut self, group_srn: String) {
        self.networking_suite
            .subnets
            .insert(group_srn.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_subnets(&group_srn).await.map_err(fmt_error);
            let _ = tx.send(Message::NetworkingSuiteSubnets { group_srn, result });
        });
    }

    fn load_networking_suite_addresses(&mut self, subnet_srn: String) {
        self.networking_suite
            .addresses
            .insert(subnet_srn.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .list_subnet_addresses(&subnet_srn)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::NetworkingSuiteAddresses { subnet_srn, result });
        });
    }

    pub(super) fn on_key_networking_suite(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.cycle_networking_suite_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_networking_suite_tab(1),
            KeyCode::Char('1') => self.networking_suite.tab = NetworkingSuiteTab::Groups,
            KeyCode::Char('2') => self.networking_suite.tab = NetworkingSuiteTab::Subnets,
            KeyCode::Char('3') => self.networking_suite.tab = NetworkingSuiteTab::Addresses,
            _ => {}
        }
    }

    fn cycle_networking_suite_tab(&mut self, delta: i32) {
        self.networking_suite.tab = self.networking_suite.tab.cycled(delta);
    }

    pub(super) fn networking_suite_refresh(&mut self) {
        self.networking_suite.groups = Loadable::Idle;
        self.networking_suite.subnets.clear();
        self.networking_suite.addresses.clear();
        self.networking_suite.group_state.select(None);
        self.networking_suite.subnet_state.select(None);
        self.networking_suite.address_state.select(None);
        self.networking_suite_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// タブの並びと巡回。両端で折り返すこと。
    #[test]
    fn tabs_cycle_in_order_and_wrap() {
        let titles: Vec<&str> = NetworkingSuiteTab::ALL
            .iter()
            .map(|tab| tab.title())
            .collect();
        assert_eq!(titles, vec!["サブネットグループ", "サブネット", "アドレス"]);

        assert_eq!(
            NetworkingSuiteTab::Groups.cycled(1),
            NetworkingSuiteTab::Subnets
        );
        assert_eq!(
            NetworkingSuiteTab::Addresses.cycled(1),
            NetworkingSuiteTab::Groups
        );
        assert_eq!(
            NetworkingSuiteTab::Groups.cycled(-1),
            NetworkingSuiteTab::Addresses
        );
    }

    /// 既定はサブネットグループ一覧。
    #[test]
    fn default_tab_is_the_subnet_group_list() {
        assert_eq!(NetworkingSuiteTab::default(), NetworkingSuiteTab::Groups);
    }
}
