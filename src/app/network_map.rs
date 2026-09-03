//! 接続マップ。ゾーン内のネットワークと、そこに繋がっているものを並べる。
//!
//! 端末では自由配置の図が読みづらいので、ネットワークを見出しにして
//! 繋がっているものをぶら下げる形にしている。既存のサーバー・スイッチ・
//! ルータの取得結果を組み替えているだけで、専用の API は使わない。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Service, fmt_error};
use crate::cloud_resources::{CloudResource, CloudResourceKind};
use crate::iaas::{NicConnection, PowerStatus, Server};
use crate::sacloud::ResourceId;
use crate::switch::Switch;

#[derive(Debug, Default)]
pub struct NetworkMapView {
    /// ゾーンごとの取得結果。3種類そろって初めて組み立てられる。
    pub maps: HashMap<String, Loadable<MapData>>,
    pub state: TableState,
}

/// 組み立ての材料。
#[derive(Debug, Clone, Default)]
pub struct MapData {
    pub servers: Vec<Server>,
    pub switches: Vec<Switch>,
    /// ルータ＋スイッチ。グローバル側の入り口になる。
    pub routers: Vec<CloudResource>,
}

/// ネットワークの種類。見出しの印と並び順に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkKind {
    /// ルータ＋スイッチ。外向きの入り口。
    Router,
    /// 共有セグメント。
    Shared,
    /// ゾーン内のスイッチ。
    Switch,
    /// どこにも繋がっていない NIC の置き場。
    Unconnected,
}

impl NetworkKind {
    pub fn mark(self) -> &'static str {
        match self {
            Self::Router => "☁",
            Self::Shared => "◉",
            Self::Switch => "▣",
            Self::Unconnected => "⚠",
        }
    }
}

/// 一覧に出す1行。
#[derive(Debug, Clone)]
pub enum MapRow {
    /// ネットワークの見出し。
    Network {
        kind: NetworkKind,
        name: String,
        /// 帯域やネットワークアドレスなど、見出しの右に出す補足。
        note: String,
        nics: usize,
        appliances: usize,
    },
    /// 繋がっている NIC。
    Nic {
        server_id: ResourceId,
        server: String,
        power: PowerStatus,
        nic: String,
        ip: String,
        filter: Option<String>,
        last: bool,
    },
    /// サーバー以外がぶら下がっていること。数だけ分かる。
    Appliances { count: usize, last: bool },
    /// 何も繋がっていないネットワーク。
    Empty,
    /// 見出しのあいだの空行。
    Spacer,
}

impl MapRow {
    /// 選べる行か。見出しと空行は飛ばす。
    pub fn is_selectable(&self) -> bool {
        matches!(self, Self::Nic { .. })
    }
}

impl App {
    /// 表示中ゾーンの接続マップ。
    pub fn visible_network_map(&self) -> Loadable<Vec<MapRow>> {
        let loadable = self
            .network_map
            .maps
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        match loadable {
            Loadable::Ready(data) => Loadable::Ready(build_rows(&data)),
            Loadable::Idle => Loadable::Idle,
            Loadable::Loading => Loadable::Loading,
            Loadable::Failed(err) => Loadable::Failed(err),
        }
    }

    /// 選んでいる行のサーバー。Enter でその画面へ飛ぶのに使う。
    pub fn selected_map_server(&self) -> Option<(ResourceId, String)> {
        let index = self.network_map.state.selected()?;
        match self.visible_network_map().ready()?.get(index)? {
            MapRow::Nic {
                server_id, server, ..
            } => Some((*server_id, server.clone())),
            _ => None,
        }
    }

    pub(super) fn network_map_ensure_loaded(&mut self) {
        let zone = self.zone.clone();
        if self
            .network_map
            .maps
            .get(&zone)
            .is_none_or(Loadable::is_idle)
        {
            self.load_network_map(zone);
            return;
        }
        let rows = self
            .visible_network_map()
            .ready()
            .cloned()
            .unwrap_or_default();
        // 見出しの行は選べないので、選べる行まで送る。
        if self.network_map.state.selected().is_none()
            && let Some(first) = rows.iter().position(MapRow::is_selectable)
        {
            self.network_map.state.select(Some(first));
        }
    }

    fn load_network_map(&mut self, zone: String) {
        self.network_map
            .maps
            .insert(zone.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            // 3つそろって初めて図になるので、1つでも失敗したら失敗にする。
            let result = async {
                let servers = client.list_servers(&zone).await?;
                let switches = client.list_switches(&zone).await?;
                let routers = client
                    .list_cloud_resources(&zone, CloudResourceKind::Internet)
                    .await?;
                Ok::<_, anyhow::Error>(MapData {
                    servers,
                    switches,
                    routers,
                })
            }
            .await
            .map_err(fmt_error);
            let _ = tx.send(Message::NetworkMap { zone, result });
        });
    }

    pub(super) fn network_map_refresh(&mut self) {
        let zone = self.zone.clone();
        self.load_network_map(zone);
    }

    pub(super) fn network_map_invalidate(&mut self) {
        self.network_map = NetworkMapView::default();
    }

    pub(super) fn on_key_network_map(&mut self, key: KeyEvent) {
        // 気になるサーバーを見つけたら、そのままサーバー画面へ移れるようにする。
        if key.code == KeyCode::Enter {
            self.jump_to_map_server();
        }
    }

    fn jump_to_map_server(&mut self) {
        let Some((id, name)) = self.selected_map_server() else {
            return;
        };
        self.service = Service::Server;
        self.ensure_loaded();
        // 一覧がまだ来ていなければ、届いたときに選び直す。
        let index = self
            .visible_servers()
            .ready()
            .and_then(|servers| servers.iter().position(|s| s.id == id));
        match index {
            Some(index) => self.server.server_state.select(Some(index)),
            None => self.server.reselect = Some(id),
        }
        self.set_status(
            format!("サーバー「{name}」へ移動しました"),
            super::StatusKind::Info,
        );
    }
}

/// 材料からネットワークごとの行を組み立てる。
///
/// 並びは 外向き → 共有セグメント → スイッチ → 未接続。
/// 上にあるものほど外に近い、という並びにしている。
pub fn build_rows(data: &MapData) -> Vec<MapRow> {
    // ルータ＋スイッチが持つスイッチは、ローカルのスイッチ一覧にも出てくる。
    // 二重に出さないよう、ルータ側の見出しにまとめる。
    let router_switch_ids: HashMap<ResourceId, &CloudResource> = data
        .routers
        .iter()
        .filter_map(|router| switch_id_of(router).map(|id| (id, router)))
        .collect();

    let mut rows = Vec::new();
    let mut first = true;
    let mut push_group = |rows: &mut Vec<MapRow>,
                          kind,
                          name: String,
                          note: String,
                          nics: Vec<MapRow>,
                          appliances| {
        if !first {
            rows.push(MapRow::Spacer);
        }
        first = false;
        rows.push(MapRow::Network {
            kind,
            name,
            note,
            nics: nics.len(),
            appliances,
        });
        if nics.is_empty() && appliances == 0 {
            rows.push(MapRow::Empty);
            return;
        }
        let last_nic = nics.len().saturating_sub(1);
        for (i, nic) in nics.into_iter().enumerate() {
            // アプライアンスの行が続くなら、まだ終端ではない。
            rows.push(mark_last(nic, i == last_nic && appliances == 0));
        }
        if appliances > 0 {
            rows.push(MapRow::Appliances {
                count: appliances,
                last: true,
            });
        }
    };

    // 1. ルータ＋スイッチ。
    for router in &data.routers {
        let Some(switch_id) = switch_id_of(router) else {
            continue;
        };
        let switch = data.switches.iter().find(|s| s.id == switch_id);
        push_group(
            &mut rows,
            NetworkKind::Router,
            router.name.clone(),
            router_note(router),
            nics_on(&data.servers, |c| c.switch_id() == Some(switch_id)),
            switch.map_or(0, |s| s.appliance_count),
        );
    }

    // 2. 共有セグメント。繋がっているものがあるときだけ出す。
    let shared = nics_on(&data.servers, |c| *c == NicConnection::Shared);
    if !shared.is_empty() {
        push_group(
            &mut rows,
            NetworkKind::Shared,
            "共有セグメント".to_string(),
            "グローバルIPが自動で割り当てられます".to_string(),
            shared,
            0,
        );
    }

    // 3. ローカルのスイッチ。繋がっていないものも、あることが分かるよう出す。
    for switch in &data.switches {
        if router_switch_ids.contains_key(&switch.id) {
            continue;
        }
        push_group(
            &mut rows,
            NetworkKind::Switch,
            switch.name.clone(),
            String::new(),
            nics_on(&data.servers, |c| c.switch_id() == Some(switch.id)),
            switch.appliance_count,
        );
    }

    // 4. どこにも繋がっていない NIC。
    let free = nics_on(&data.servers, |c| *c == NicConnection::None);
    if !free.is_empty() {
        push_group(
            &mut rows,
            NetworkKind::Unconnected,
            "未接続".to_string(),
            "どのネットワークにも繋がっていません".to_string(),
            free,
            0,
        );
    }

    rows
}

/// 条件に合う NIC を、サーバー名の順で集める。
fn nics_on(servers: &[Server], matches: impl Fn(&NicConnection) -> bool) -> Vec<MapRow> {
    let mut rows: Vec<MapRow> = servers
        .iter()
        .flat_map(|server| {
            server
                .nics
                .iter()
                .filter(|nic| matches(&nic.connection))
                .map(move |nic| MapRow::Nic {
                    server_id: server.id,
                    server: server.name.clone(),
                    power: server.power,
                    nic: nic.name(),
                    ip: nic.ip_address.clone(),
                    filter: nic.packet_filter.clone(),
                    last: false,
                })
        })
        .collect();
    rows.sort_by(|a, b| match (a, b) {
        (
            MapRow::Nic {
                server: a, nic: an, ..
            },
            MapRow::Nic {
                server: b, nic: bn, ..
            },
        ) => a.cmp(b).then_with(|| an.cmp(bn)),
        _ => std::cmp::Ordering::Equal,
    });
    rows
}

fn mark_last(row: MapRow, is_last: bool) -> MapRow {
    match row {
        MapRow::Nic {
            server_id,
            server,
            power,
            nic,
            ip,
            filter,
            ..
        } => MapRow::Nic {
            server_id,
            server,
            power,
            nic,
            ip,
            filter,
            last: is_last,
        },
        other => other,
    }
}

/// ルータ＋スイッチが持つスイッチの ID。
fn switch_id_of(router: &CloudResource) -> Option<ResourceId> {
    router
        .details
        .iter()
        .find(|(label, _)| label == "スイッチ")
        .and_then(|(_, value)| value.parse().ok())
        .map(ResourceId)
}

/// 見出しの右に出す補足。帯域とネットワークアドレスが分かると当たりを付けやすい。
fn router_note(router: &CloudResource) -> String {
    let detail = |label: &str| {
        router
            .details
            .iter()
            .find(|(key, _)| key == label)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    };
    let network = detail("ネットワーク");
    let mask = detail("マスク長");
    let bandwidth = detail("帯域(Mbps)");
    let mut parts = Vec::new();
    if !network.is_empty() {
        parts.push(if mask.is_empty() {
            network
        } else {
            format!("{network}/{mask}")
        });
    }
    if !bandwidth.is_empty() {
        parts.push(format!("{bandwidth} Mbps"));
    }
    parts.join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iaas::Nic;

    fn nic(id: u64, index: usize, connection: NicConnection, ip: &str) -> Nic {
        Nic {
            id: ResourceId(id),
            index,
            connection,
            ip_address: ip.to_string(),
            mac_address: String::new(),
            packet_filter: None,
        }
    }

    fn server(id: u64, name: &str, power: PowerStatus, nics: Vec<Nic>) -> Server {
        Server {
            id: ResourceId(id),
            name: name.to_string(),
            description: String::new(),
            tags: Vec::new(),
            host_name: String::new(),
            availability: "available".to_string(),
            power,
            plan_name: String::new(),
            cpu: 1,
            memory_mb: 1024,
            ip_addresses: Vec::new(),
            disk_names: Vec::new(),
            packet_filter_name: None,
            nics,
            zone: "is1a".to_string(),
            created_at: None,
        }
    }

    fn switch(id: u64, name: &str, appliances: usize) -> Switch {
        Switch {
            id: ResourceId(id),
            name: name.to_string(),
            description: String::new(),
            tags: Vec::new(),
            zone: "is1a".to_string(),
            server_count: 0,
            appliance_count: appliances,
            created_at: None,
        }
    }

    fn names(rows: &[MapRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                MapRow::Network { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// ネットワークごとにまとめ、外に近いものから並べること。
    #[test]
    fn networks_are_ordered_from_the_outside_in() {
        let data = MapData {
            servers: vec![
                server(
                    1,
                    "web-01",
                    PowerStatus::Up,
                    vec![
                        nic(10, 0, NicConnection::Shared, "133.242.0.10"),
                        nic(
                            11,
                            1,
                            NicConnection::Switch {
                                id: Some(ResourceId(77)),
                                name: "sw-private".to_string(),
                            },
                            "192.168.0.10",
                        ),
                    ],
                ),
                server(
                    2,
                    "old-01",
                    PowerStatus::Down,
                    vec![nic(12, 0, NicConnection::None, "")],
                ),
            ],
            switches: vec![switch(77, "sw-private", 1)],
            routers: Vec::new(),
        };
        assert_eq!(
            names(&build_rows(&data)),
            ["共有セグメント", "sw-private", "未接続"]
        );
    }

    /// スイッチは ID で突き合わせること。名前は重複しうる。
    #[test]
    fn nics_are_grouped_by_switch_id_not_name() {
        let data = MapData {
            servers: vec![server(
                1,
                "web-01",
                PowerStatus::Up,
                vec![nic(
                    10,
                    0,
                    NicConnection::Switch {
                        id: Some(ResourceId(78)),
                        name: "sw".to_string(),
                    },
                    "192.168.1.10",
                )],
            )],
            // 同じ名前のスイッチが2つある。
            switches: vec![switch(77, "sw", 0), switch(78, "sw", 0)],
            routers: Vec::new(),
        };
        let rows = build_rows(&data);
        // 2つ目のスイッチにだけ NIC がぶら下がる。
        let counts: Vec<usize> = rows
            .iter()
            .filter_map(|row| match row {
                MapRow::Network { nics, .. } => Some(*nics),
                _ => None,
            })
            .collect();
        assert_eq!(counts, [0, 1]);
    }

    /// 何も繋がっていないスイッチも、あることが分かるよう出すこと。
    #[test]
    fn an_empty_switch_is_still_shown() {
        let data = MapData {
            servers: Vec::new(),
            switches: vec![switch(77, "sw-unused", 0)],
            routers: Vec::new(),
        };
        let rows = build_rows(&data);
        assert_eq!(names(&rows), ["sw-unused"]);
        assert!(rows.iter().any(|row| matches!(row, MapRow::Empty)));
    }

    /// 枝の終端は最後の1つだけにすること。アプライアンスが続くなら NIC は終端でない。
    #[test]
    fn only_the_last_entry_closes_the_branch() {
        let data = MapData {
            servers: vec![server(
                1,
                "web-01",
                PowerStatus::Up,
                vec![nic(
                    10,
                    0,
                    NicConnection::Switch {
                        id: Some(ResourceId(77)),
                        name: "sw".to_string(),
                    },
                    "192.168.0.10",
                )],
            )],
            switches: vec![switch(77, "sw", 2)],
            routers: Vec::new(),
        };
        let rows = build_rows(&data);
        let nic_last = rows.iter().find_map(|row| match row {
            MapRow::Nic { last, .. } => Some(*last),
            _ => None,
        });
        assert_eq!(nic_last, Some(false), "後ろにアプライアンスが続く");
        assert!(rows.iter().any(|row| matches!(
            row,
            MapRow::Appliances {
                count: 2,
                last: true
            }
        )));
    }

    /// 選べるのは NIC の行だけ。見出しや空行に止まらないようにする。
    #[test]
    fn only_nic_rows_are_selectable() {
        assert!(!MapRow::Empty.is_selectable());
        assert!(!MapRow::Spacer.is_selectable());
        assert!(
            !MapRow::Network {
                kind: NetworkKind::Switch,
                name: "sw".to_string(),
                note: String::new(),
                nics: 0,
                appliances: 0,
            }
            .is_selectable()
        );
    }
}
