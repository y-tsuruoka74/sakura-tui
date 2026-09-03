//! さくらのクラウド IaaS のゾーンとサーバー。
//!
//! コンテナレジストリと違いサーバーはゾーンに属するため、エンドポイントの
//! ゾーン部分を切り替えて呼ぶ。

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{ResourceId, SacloudClient, null_as_default};

/// 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が実態と違う総件数を返しても止まるようにする。
const MAX_PAGES: usize = 100;

/// 本番環境のゾーン。
///
/// 認証情報を作る前はゾーン一覧を引けないため、既知のものを並べておく。
/// （sacloud/iaas-api-go の `types::ZoneNames` と同じ並び）
const PRODUCTION_ZONES: [(&str, &str); 6] = [
    ("tk1a", "東京第1"),
    ("tk1b", "東京第2"),
    ("is1a", "石狩第1"),
    ("is1b", "石狩第2"),
    ("is1c", "石狩第3"),
    ("tk1v", "サンドボックス"),
];

/// 社内テスト環境（cloud-test）のゾーン。本番とは名前が全く違う。
const TEST_ZONES: [(&str, &str); 4] = [
    ("is1x", "開発Xゾーン(Sandbox)"),
    ("is1y", "開発Yゾーン"),
    ("is1z", "開発Zゾーン"),
    ("tk1s", "開発Sandbox"),
];

fn to_zones(pairs: &[(&str, &str)]) -> Vec<Zone> {
    pairs
        .iter()
        .map(|(name, description)| Zone {
            name: name.to_string(),
            description: description.to_string(),
        })
        .collect()
}

/// 接続先に応じた既知のゾーン一覧。
///
/// 環境ごとにゾーン名が違うため、接続先を選び直したらこちらも入れ替える。
pub fn known_zones_for(api_root: &str) -> Vec<Zone> {
    if api_root == crate::config::TEST_API_ROOT {
        to_zones(&TEST_ZONES)
    } else {
        to_zones(&PRODUCTION_ZONES)
    }
}

/// ゾーン 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    pub name: String,
    pub description: String,
}

impl Zone {
    /// ピッカーやヘッダーに出す表示名。
    pub fn label(&self) -> String {
        if self.description.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.description)
        }
    }
}

/// サーバーの電源状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerStatus {
    Up,
    Down,
    Cleaning,
    Unknown,
}

impl PowerStatus {
    pub fn label(self) -> &'static str {
        match self {
            PowerStatus::Up => "起動中",
            PowerStatus::Down => "停止",
            PowerStatus::Cleaning => "処理中",
            PowerStatus::Unknown => "不明",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "up" => PowerStatus::Up,
            "down" => PowerStatus::Down,
            "cleaning" => PowerStatus::Cleaning,
            _ => PowerStatus::Unknown,
        }
    }
}

/// サーバー 1 件。
#[derive(Debug, Clone)]
pub struct Server {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub host_name: String,
    pub availability: String,
    pub power: PowerStatus,
    pub plan_name: String,
    pub cpu: u32,
    pub memory_mb: u32,
    /// 接続されている NIC の IP アドレス（グローバル / 個別割当の順で拾う）。
    pub ip_addresses: Vec<String>,
    pub disk_names: Vec<String>,
    /// eth0 に付いているパケットフィルタの名前。
    pub packet_filter_name: Option<String>,
    pub nics: Vec<Nic>,
    pub zone: String,
    pub created_at: Option<String>,
}

/// サーバーの NIC。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nic {
    pub id: ResourceId,
    /// `eth0` のような並び順。
    pub index: usize,
    pub connection: NicConnection,
    /// 割り当てられている IP。共有セグメントは自動、スイッチは自分で決めた値。
    pub ip_address: String,
    pub mac_address: String,
    /// 付いているパケットフィルタの名前。外すのに ID は要らない。
    pub packet_filter: Option<String>,
}

impl Nic {
    pub fn name(&self) -> String {
        format!("eth{}", self.index)
    }
}

/// 今の NIC の繋ぎ先。名前を出すためだけに使う。
///
/// 繋ぎ替えの指定は [`NicAttach`] のほうを使う。読むときは ID が
/// 応答に無いこともあるが、繋ぎ替えでは必ず要るため、型を分けている。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NicConnection {
    Shared,
    Switch(String),
    None,
}

impl NicConnection {
    pub fn label(&self) -> String {
        match self {
            Self::Shared => "共有セグメント".to_string(),
            Self::Switch(name) if name.is_empty() => "スイッチ".to_string(),
            Self::Switch(name) => name.clone(),
            Self::None => "未接続".to_string(),
        }
    }
}

/// NIC の繋ぎ替え先。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicAttach {
    Shared,
    Switch(ResourceId),
    None,
}

impl Server {
    pub fn memory_gb(&self) -> f64 {
        self.memory_mb as f64 / 1024.0
    }
}

/// 電源操作の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Boot,
    /// ACPI シャットダウン（OS に依頼する）。
    Shutdown,
    /// 電源を落とす（電源ケーブルを抜くのと同じ）。
    PowerOff,
    Reset,
}

impl PowerAction {
    pub fn label(self) -> &'static str {
        match self {
            PowerAction::Boot => "起動",
            PowerAction::Shutdown => "シャットダウン",
            PowerAction::PowerOff => "強制停止",
            PowerAction::Reset => "強制リセット",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            PowerAction::Boot => "サーバーを起動します。",
            PowerAction::Shutdown => "OS に ACPI シャットダウンを依頼します。",
            PowerAction::PowerOff => {
                "電源を即座に切ります。電源ケーブルを抜くのと同じで、データが失われることがあります。"
            }
            PowerAction::Reset => {
                "電源を入れ直します。電源ボタンの長押しと同じで、データが失われることがあります。"
            }
        }
    }

    /// 取り返しがつかない可能性がある操作か（確認ダイアログの色分けに使う）。
    pub fn is_risky(self) -> bool {
        matches!(self, PowerAction::PowerOff | PowerAction::Reset)
    }
}

// --- API のレスポンス形状 ---

#[derive(Debug, Deserialize)]
struct ZoneListResponse {
    #[serde(rename = "Zones", default, deserialize_with = "null_as_default")]
    zones: Vec<NakedZone>,
}

#[derive(Debug, Deserialize)]
struct NakedZone {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
}

#[derive(Debug, Deserialize)]
struct ServerFindResponse {
    #[serde(rename = "Servers", default, deserialize_with = "null_as_default")]
    servers: Vec<NakedServer>,
    #[serde(rename = "Total", default)]
    total: usize,
}

#[derive(Debug, Deserialize)]
struct NakedServer {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
    #[serde(rename = "HostName", default, deserialize_with = "null_as_default")]
    host_name: String,
    #[serde(rename = "Availability", default, deserialize_with = "null_as_default")]
    availability: String,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "ServerPlan")]
    server_plan: Option<NakedServerPlan>,
    #[serde(rename = "Instance")]
    instance: Option<NakedInstance>,
    #[serde(rename = "Interfaces", default, deserialize_with = "null_as_default")]
    interfaces: Vec<NakedInterface>,
    #[serde(rename = "Disks", default, deserialize_with = "null_as_default")]
    disks: Vec<NakedDisk>,
    #[serde(rename = "Zone")]
    zone: Option<NakedZone>,
}

#[derive(Debug, Deserialize)]
struct NakedServerPlan {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "CPU", default)]
    cpu: u32,
    #[serde(rename = "MemoryMB", default)]
    memory_mb: u32,
}

#[derive(Debug, Deserialize)]
struct NakedInstance {
    #[serde(rename = "Status", default, deserialize_with = "null_as_default")]
    status: String,
}

#[derive(Debug, Deserialize)]
struct NakedInterface {
    // 応答に無いことは実際には無いが、1枚欠けただけで一覧全部が
    // 読めなくなるのは困るので、無ければその NIC を飛ばす。
    #[serde(rename = "ID")]
    id: Option<ResourceId>,
    #[serde(rename = "IPAddress", default, deserialize_with = "null_as_default")]
    ip_address: String,
    #[serde(
        rename = "UserIPAddress",
        default,
        deserialize_with = "null_as_default"
    )]
    user_ip_address: String,
    #[serde(rename = "MACAddress", default, deserialize_with = "null_as_default")]
    mac_address: String,
    #[serde(rename = "Switch")]
    switch: Option<NakedNicSwitch>,
    #[serde(rename = "PacketFilter")]
    packet_filter: Option<NakedNamed>,
}

#[derive(Debug, Deserialize)]
struct NakedNicSwitch {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    /// `shared` なら共有セグメント。
    #[serde(rename = "Scope", default, deserialize_with = "null_as_default")]
    scope: String,
}

/// 名前だけ使う入れ子。
#[derive(Debug, Deserialize)]
struct NakedNamed {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct NakedDisk {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
}

impl From<NakedServer> for Server {
    fn from(naked: NakedServer) -> Self {
        let plan = naked.server_plan;
        Server {
            id: naked.id,
            name: naked.name,
            description: naked.description,
            tags: naked.tags,
            host_name: naked.host_name,
            availability: naked.availability,
            power: naked
                .instance
                .map_or(PowerStatus::Unknown, |i| PowerStatus::parse(&i.status)),
            plan_name: plan.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
            cpu: plan.as_ref().map_or(0, |p| p.cpu),
            memory_mb: plan.map_or(0, |p| p.memory_mb),
            // フィルタは NIC ごとに付くが、一覧の隣には eth0 のものだけ出す。
            packet_filter_name: naked
                .interfaces
                .first()
                .and_then(|nic| nic.packet_filter.as_ref())
                .map(|f| f.name.clone()),
            nics: naked
                .interfaces
                .iter()
                .enumerate()
                .filter_map(|(index, nic)| {
                    Some(Nic {
                        id: nic.id?,
                        index,
                        connection: match &nic.switch {
                            None => NicConnection::None,
                            Some(sw) if sw.scope == "shared" => NicConnection::Shared,
                            Some(sw) => NicConnection::Switch(sw.name.clone()),
                        },
                        ip_address: if nic.ip_address.is_empty() {
                            nic.user_ip_address.clone()
                        } else {
                            nic.ip_address.clone()
                        },
                        mac_address: nic.mac_address.clone(),
                        packet_filter: nic.packet_filter.as_ref().map(|f| f.name.clone()),
                    })
                })
                .collect(),
            ip_addresses: naked
                .interfaces
                .iter()
                // 共有セグメントは IPAddress、スイッチ接続は UserIPAddress に入る。
                .map(|nic| {
                    if nic.ip_address.is_empty() {
                        &nic.user_ip_address
                    } else {
                        &nic.ip_address
                    }
                })
                .filter(|ip| !ip.is_empty())
                .cloned()
                .collect(),
            disk_names: naked.disks.into_iter().map(|d| d.name).collect(),
            zone: naked.zone.map(|z| z.name).unwrap_or_default(),
            created_at: naked.created_at,
        }
    }
}

impl SacloudClient {
    /// 利用できるゾーンの一覧。
    pub async fn list_zones(&self) -> Result<Vec<Zone>> {
        let res: ZoneListResponse = self
            .request_in_zone(self.default_zone(), Method::GET, "zone", None)
            .await?;
        Ok(res
            .zones
            .into_iter()
            .map(|z| Zone {
                name: z.name,
                description: z.description,
            })
            .collect())
    }

    /// 指定ゾーンのサーバーを全件取得する。
    pub async fn list_servers(&self, zone: &str) -> Result<Vec<Server>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let res: ServerFindResponse = self
                .request_in_zone(zone, Method::GET, "server", Some(body))
                .await?;
            let received = res.servers.len();
            out.extend(res.servers.into_iter().map(Server::from));
            if received == 0 || out.len() >= res.total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    /// 指定ゾーンのサーバー件数だけを数える。
    ///
    /// 一覧を全部引かずに済むよう、1 件だけ要求して `Total` を読む。
    pub async fn count_servers(&self, zone: &str) -> Result<usize> {
        let body = json!({ "From": 0, "Count": 1 });
        let res: ServerFindResponse = self
            .request_in_zone(zone, Method::GET, "server", Some(body))
            .await?;
        Ok(res.total)
    }

    /// 電源操作。
    pub async fn power_action(
        &self,
        zone: &str,
        id: ResourceId,
        action: PowerAction,
    ) -> Result<()> {
        let (method, path, body) = match action {
            PowerAction::Boot => (Method::PUT, format!("server/{id}/power"), None),
            // ACPI シャットダウンと強制停止は同じエンドポイントで Force で分ける。
            PowerAction::Shutdown => (
                Method::DELETE,
                format!("server/{id}/power"),
                Some(json!({ "Force": false })),
            ),
            PowerAction::PowerOff => (
                Method::DELETE,
                format!("server/{id}/power"),
                Some(json!({ "Force": true })),
            ),
            PowerAction::Reset => (Method::PUT, format!("server/{id}/reset"), None),
        };
        let _: serde_json::Value = self.request_in_zone(zone, method, &path, body).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// プラン一覧は共有CPUの使えるものだけを出し、
    /// 同じ構成が世代違いで並ぶのを1つに畳む。
    #[test]
    fn server_plans_are_deduplicated_by_size() {
        let value = serde_json::json!({"ServerPlans": [
            {"Name": "旧2c4g", "CPU": 2, "MemoryMB": 4096,
             "Commitment": "standard", "Generation": 100, "Availability": "available"},
            {"Name": "新2c4g", "CPU": 2, "MemoryMB": 4096,
             "Commitment": "standard", "Generation": 200, "Availability": "available"},
            {"Name": "1c1g", "CPU": 1, "MemoryMB": 1024,
             "Commitment": "standard", "Generation": 200, "Availability": "available"},
            {"Name": "専有", "CPU": 4, "MemoryMB": 8192,
             "Commitment": "dedicatedcpu", "Generation": 200, "Availability": "available"},
            {"Name": "販売終了", "CPU": 8, "MemoryMB": 16384,
             "Commitment": "standard", "Generation": 200, "Availability": "discontinued"}
        ]});
        let plans = parse_server_plans(&value);
        let labels: Vec<String> = plans.iter().map(ServerPlan::label).collect();
        assert_eq!(labels, vec!["1 コア / 1 GB", "2 コア / 4 GB"]);
        // 畳むときは新しい世代を残す。
        assert_eq!(plans[1].generation, 200);
    }

    /// コア数とメモリを別々に選べるよう、重複なく取り出せること。
    #[test]
    fn plan_choices_are_split_into_cpu_and_memory() {
        let plans = sample_plans();
        assert_eq!(cpu_choices(&plans), vec![1, 2, 4]);
        assert_eq!(memory_choices(&plans, 2), vec![2048, 4096]);
        // 無いコア数を聞かれても落ちない。
        assert!(memory_choices(&plans, 3).is_empty());
    }

    /// コア数を変えたとき、その構成にあるメモリのうち一番近いものを返すこと。
    /// そのまま持ち越すと存在しない組み合わせで作成してしまう。
    #[test]
    fn memory_snaps_to_what_the_cpu_actually_offers() {
        let plans = sample_plans();
        assert_eq!(nearest_memory(&plans, 4, 2048), 8192);
        assert_eq!(nearest_memory(&plans, 1, 4096), 2048);
        // 選べるものが無ければ希望のまま返す。
        assert_eq!(nearest_memory(&plans, 3, 4096), 4096);
    }

    #[test]
    fn plan_exists_checks_the_pair_not_each_side() {
        let plans = sample_plans();
        assert!(plan_exists(&plans, 2, 4096));
        // 2 コアも 8GB もあるが、その組み合わせは無い。
        assert!(!plan_exists(&plans, 2, 8192));
    }

    fn sample_plans() -> Vec<ServerPlan> {
        [(1, 1024), (1, 2048), (2, 2048), (2, 4096), (4, 8192)]
            .into_iter()
            .map(|(cpu, memory_mb)| ServerPlan {
                name: String::new(),
                cpu,
                memory_mb,
                commitment: "standard".to_string(),
                generation: 200,
                availability: "available".to_string(),
            })
            .collect()
    }

    /// NIC を接続先つきで読めること。共有セグメントとスイッチを取り違えない。
    #[test]
    fn nics_report_where_they_are_connected() {
        let value = serde_json::json!({"ID": "1", "Name": "web", "Interfaces": [
            {"ID": "10", "IPAddress": "203.0.113.5", "MACAddress": "9c:a3:xx",
             "Switch": {"ID": "1", "Name": "共有セグメント", "Scope": "shared"}},
            {"ID": "11", "UserIPAddress": "192.168.0.10",
             "Switch": {"ID": "77", "Name": "sw-01", "Scope": "user"},
             "PacketFilter": {"ID": "9", "Name": "web-filter"}},
            {"ID": "12"}
        ]});
        let server: Server = serde_json::from_value::<NakedServer>(value).unwrap().into();
        assert_eq!(server.nics.len(), 3);
        assert_eq!(server.nics[0].name(), "eth0");
        assert_eq!(server.nics[0].connection, NicConnection::Shared);
        assert_eq!(server.nics[0].ip_address, "203.0.113.5");
        // スイッチ接続は UserIPAddress のほうに入る。
        assert_eq!(
            server.nics[1].connection,
            NicConnection::Switch("sw-01".to_string())
        );
        assert_eq!(server.nics[1].ip_address, "192.168.0.10");
        assert_eq!(server.nics[1].packet_filter.as_deref(), Some("web-filter"));
        // どこにも繋がっていない NIC。
        assert_eq!(server.nics[2].connection, NicConnection::None);
        assert_eq!(server.nics[2].connection.label(), "未接続");
    }

    /// ID の無い NIC があっても、他が読めなくならないこと。
    #[test]
    fn a_nic_without_an_id_is_skipped_not_fatal() {
        let value = serde_json::json!({"ID": "1", "Name": "web", "Interfaces": [
            {"IPAddress": "203.0.113.5"},
            {"ID": "11", "IPAddress": "203.0.113.6"}
        ]});
        let server: Server = serde_json::from_value::<NakedServer>(value).unwrap().into();
        // 操作できない NIC は落とすが、IP の一覧には残す。
        assert_eq!(server.nics.len(), 1);
        assert_eq!(server.nics[0].id, ResourceId(11));
        assert_eq!(server.ip_addresses, ["203.0.113.5", "203.0.113.6"]);
    }

    /// eth0 のパケットフィルタを詳細に出せること。
    /// 付けたつもりで付いていない、を画面から確かめられるようにする。
    #[test]
    fn the_first_nic_reports_its_packet_filter() {
        let with_filter = serde_json::json!({"ID": "1", "Name": "web", "Interfaces": [
            {"IPAddress": "192.0.2.1", "PacketFilter": {"ID": "9", "Name": "probe-filter"}}
        ]});
        let server: Server = serde_json::from_value::<NakedServer>(with_filter)
            .unwrap()
            .into();
        assert_eq!(server.packet_filter_name.as_deref(), Some("probe-filter"));
        assert_eq!(server.ip_addresses, ["192.0.2.1"]);

        let without = serde_json::json!({"ID": "2", "Name": "db", "Interfaces": [
            {"IPAddress": "192.0.2.2"}
        ]});
        let server: Server = serde_json::from_value::<NakedServer>(without)
            .unwrap()
            .into();
        assert_eq!(server.packet_filter_name, None);
    }

    /// NIC の繋ぎ先で送る中身が変わること。
    #[test]
    fn the_nic_plan_decides_what_the_server_connects_to() {
        assert_eq!(
            NicPlan::Shared.connected_switches(),
            serde_json::json!([{ "Scope": "shared" }])
        );
        let on_switch = NicPlan::Switch {
            id: ResourceId(9),
            ip_address: "192.168.0.10".to_string(),
            mask_len: 24,
            gateway: "192.168.0.1".to_string(),
        };
        assert_eq!(
            on_switch.connected_switches(),
            serde_json::json!([{ "ID": 9 }])
        );
        // 繋がない場合も NIC 自体は作るので、空の要素を1つ送る。
        assert_eq!(
            NicPlan::None.connected_switches(),
            serde_json::json!([null])
        );
    }

    /// スイッチに繋ぐときだけ、ディスクの修正で IP を書き込むこと。
    /// 共有セグメントは DHCP で降ってくるので書いてはいけない。
    #[test]
    fn only_a_switch_needs_the_ip_written_into_the_disk() {
        assert!(NicPlan::Shared.disk_network_config().is_none());
        assert!(NicPlan::None.disk_network_config().is_none());
        let on_switch = NicPlan::Switch {
            id: ResourceId(9),
            ip_address: "192.168.0.10".to_string(),
            mask_len: 24,
            gateway: "192.168.0.1".to_string(),
        };
        assert_eq!(
            on_switch.disk_network_config(),
            Some(("192.168.0.10".to_string(), 24, "192.168.0.1".to_string()))
        );
    }

    /// スタートアップスクリプトは Notes から拾うこと。
    #[test]
    fn startup_scripts_come_from_the_notes_list() {
        let value = serde_json::json!({"Notes": [
            {"ID": "1", "Name": "さくら提供", "Class": "shell", "Scope": "shared"},
            {"ID": 9, "Name": "パケットフィルタのひな形", "Class": "json_packetfilter"},
            {"ID": 10, "Name": "Terraformのひな形", "Class": "hcl_terraform"},
            {"ID": 2, "Name": "cloud-init", "Class": "yaml_cloud_config", "Scope": "shared"},
            {"ID": 3, "Name": "自分の初期設定", "Class": "shell", "Scope": "user",
             "Description": "パッケージを入れる", "Tags": ["web"]}
        ]});
        let scripts = parse_startup_scripts(&value);
        // 流せない種別は落とす。
        assert_eq!(scripts.len(), 3);
        assert!(!scripts.iter().any(|s| s.class == "hcl_terraform"));
        // 自分で作ったものが先頭に来る。共有のものに埋もれさせない。
        assert_eq!(scripts[0].name, "自分の初期設定");
        assert!(scripts[0].is_own());
        assert_eq!(scripts[0].description, "パッケージを入れる");
        assert_eq!(scripts[0].tags, ["web"]);
        // 残りは名前順。
        assert_eq!(scripts[1].name, "cloud-init");
        assert!(!scripts[1].is_own());
    }

    /// 公開鍵は本体が無いものを捨て、名前が空でも指紋で見分けられること。
    #[test]
    fn ssh_keys_drop_entries_without_a_key() {
        let value = serde_json::json!({"SSHKeys": [
            {"ID": "1", "Name": "手元", "PublicKey": "ssh-ed25519 AAAA...\n",
             "Fingerprint": "aa:bb"},
            {"ID": 2, "Name": "", "PublicKey": "", "Fingerprint": "cc:dd"}
        ]});
        let keys = parse_ssh_keys(&value);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].name, "手元");
        // 前後の空白は落として1行にする。
        assert_eq!(keys[0].public_key, "ssh-ed25519 AAAA...");
    }

    /// ディスクプランは SSD と HDD だけを拾い、使えるサイズだけを残す。
    #[test]
    fn disk_plans_keep_only_available_sizes() {
        let value = serde_json::json!({"DiskPlans": [
            {"ID": 4, "Name": "SSD", "Size": [
                {"SizeMB": 20480, "Availability": "available"},
                {"SizeMB": 40960, "Availability": "available"},
                {"SizeMB": 102400, "Availability": "discontinued"}
            ]},
            {"ID": 2, "Name": "HDD", "Size": [{"SizeMB": 40960, "Availability": "available"}]},
            {"ID": 99, "Name": "未知", "Size": []}
        ]});
        let plans = parse_disk_plans(&value);
        assert_eq!(plans.len(), 2);
        assert!(plans[0].is_ssd());
        assert_eq!(plans[0].sizes_mb, vec![20480, 40960]);
        assert!(!plans[1].is_ssd());
    }

    /// OS の選択肢は公式SDKのタグ条件をそのまま持つ。
    /// 綴りを間違えるとテンプレートが引けなくなる。
    #[test]
    fn os_choices_carry_the_sdk_tags() {
        assert_eq!(OS_CHOICES.len(), 6);
        let ubuntu = OS_CHOICES[0];
        assert_eq!(ubuntu.tags, &["current-stable", "distro-ubuntu"]);
        // 版を固定する選択肢は単独タグ。
        let pinned = OS_CHOICES[5];
        assert_eq!(pinned.tags, &["ubuntu-24.04-latest"]);
    }

    /// 途中で失敗しても、そこまでに作ったものが分かること。
    #[test]
    fn progress_records_what_was_created() {
        let mut progress = ServerCreateProgress::default();
        assert!(progress.server_id.is_none() && progress.disk_id.is_none());
        progress.server_id = Some(ResourceId(1));
        assert!(progress.disk_id.is_none());
    }

    #[test]
    fn parses_server_list() {
        let body = r#"{
            "Total": 1, "From": 0, "Count": 1,
            "Servers": [{
                "ID": "113000000000",
                "Name": "web-01",
                "Description": null,
                "Tags": [],
                "HostName": "web-01",
                "Availability": "available",
                "ServerPlan": {"Name": "2Core-4GB", "CPU": 2, "MemoryMB": 4096},
                "Instance": {"Status": "up"},
                "Interfaces": [
                    {"IPAddress": "203.0.113.10", "UserIPAddress": null},
                    {"IPAddress": null, "UserIPAddress": "192.168.0.5"}
                ],
                "Disks": [{"Name": "web-01-disk"}],
                "Zone": {"Name": "is1a", "Description": "石狩第1ゾーン"}
            }]
        }"#;
        let res: ServerFindResponse = serde_json::from_str(body).unwrap();
        let server = Server::from(res.servers.into_iter().next().unwrap());
        assert_eq!(server.name, "web-01");
        assert_eq!(server.power, PowerStatus::Up);
        assert_eq!(server.cpu, 2);
        assert_eq!(server.memory_gb(), 4.0);
        assert_eq!(server.ip_addresses, vec!["203.0.113.10", "192.168.0.5"]);
        assert_eq!(server.zone, "is1a");
    }

    #[test]
    fn unknown_power_status_does_not_panic() {
        assert_eq!(PowerStatus::parse("migrating"), PowerStatus::Unknown);
        assert_eq!(PowerStatus::parse("down"), PowerStatus::Down);
    }

    #[test]
    fn parses_zone_list() {
        let body = r#"{"Zones": [
            {"Name": "is1a", "Description": "石狩第1ゾーン"},
            {"Name": "tk1b", "Description": null}
        ]}"#;
        let res: ZoneListResponse = serde_json::from_str(body).unwrap();
        assert_eq!(res.zones.len(), 2);
        let zone = Zone {
            name: res.zones[1].name.clone(),
            description: res.zones[1].description.clone(),
        };
        // 説明が無ければ名前だけを表示する。
        assert_eq!(zone.label(), "tk1b");
    }

    #[test]
    fn risky_actions_are_marked() {
        assert!(PowerAction::PowerOff.is_risky());
        assert!(PowerAction::Reset.is_risky());
        assert!(!PowerAction::Boot.is_risky());
        assert!(!PowerAction::Shutdown.is_risky());
    }
}

#[cfg(test)]
mod count_tests {
    use super::*;

    /// 件数だけを知りたいので、1 件だけ返る応答から Total を読めること。
    #[test]
    fn reads_total_from_single_record_response() {
        let body = r#"{"Total": 42, "From": 0, "Count": 1, "Servers": [
            {"ID": "1", "Name": "web-01"}
        ]}"#;
        let res: ServerFindResponse = serde_json::from_str(body).unwrap();
        assert_eq!(res.total, 42);
        assert_eq!(res.servers.len(), 1, "1件だけ受け取る");
    }

    /// 0 件のゾーンでも落ちないこと。
    #[test]
    fn zero_servers_is_fine() {
        let body = r#"{"Total": 0, "From": 0, "Count": 0, "Servers": []}"#;
        let res: ServerFindResponse = serde_json::from_str(body).unwrap();
        assert_eq!(res.total, 0);
        assert!(res.servers.is_empty());
    }
}

#[cfg(test)]
mod zone_list_tests {
    use super::*;

    /// 環境ごとにゾーン名が違う。cloud-test は本番と重ならない。
    #[test]
    fn environments_have_distinct_zones() {
        let production = known_zones_for(crate::config::DEFAULT_API_ROOT);
        let test = known_zones_for(crate::config::TEST_API_ROOT);

        assert!(production.iter().any(|z| z.name == "is1a"));
        assert!(test.iter().any(|z| z.name == "is1x"));

        // 本番のゾーンは cloud-test には存在しない（404 の原因だった）。
        for zone in &production {
            assert!(
                !test.iter().any(|t| t.name == zone.name),
                "{} が両方にある",
                zone.name
            );
        }
    }

    /// 知らない接続先は本番のゾーンで代用する。
    #[test]
    fn unknown_root_falls_back_to_production() {
        let zones = known_zones_for("https://example.internal/api/zone");
        assert!(zones.iter().any(|z| z.name == "is1a"));
    }

    #[test]
    fn test_zones_match_the_environment() {
        let names: Vec<&str> = known_zones_for(crate::config::TEST_API_ROOT)
            .iter()
            .map(|z| z.name.clone())
            .map(|n| Box::leak(n.into_boxed_str()) as &str)
            .collect();
        assert_eq!(names, vec!["is1x", "is1y", "is1z", "tk1s"]);
    }

    /// 説明が空でないこと（ピッカーで見分けるため）。
    #[test]
    fn every_zone_has_a_description() {
        for root in [
            crate::config::DEFAULT_API_ROOT,
            crate::config::TEST_API_ROOT,
        ] {
            for zone in known_zones_for(root) {
                assert!(!zone.description.is_empty(), "{}", zone.name);
                assert!(zone.label().contains(&zone.name));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// サーバーの作成
// ---------------------------------------------------------------------------

/// ディスクプランの ID。公式ドキュメントには無く、公式SDKが持っている値。
const DISK_PLAN_SSD: u32 = 4;
const DISK_PLAN_HDD: u32 = 2;

/// ディスクのコピー完了を待つ間隔と上限。公式SDKに合わせる。
const DISK_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
const DISK_POLL_MAX: usize = 240;

/// サーバープラン 1 件。`/product/server` から引く。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerPlan {
    pub name: String,
    pub cpu: u32,
    pub memory_mb: u32,
    /// `standard` / `dedicatedcpu`。
    pub commitment: String,
    pub generation: u32,
    pub availability: String,
}

impl ServerPlan {
    pub fn label(&self) -> String {
        format!("{} コア / {} GB", self.cpu, self.memory_mb / 1024)
    }

    pub fn is_available(&self) -> bool {
        self.availability.is_empty() || self.availability == "available"
    }
}

/// プラン一覧から選べるコア数を昇順で取り出す。
///
/// プランは100通り以上あるので、一覧をそのまま送るとフォームで選べない。
/// コア数とメモリに分けて絞り込めるようにする。
pub fn cpu_choices(plans: &[ServerPlan]) -> Vec<u32> {
    let mut cpus: Vec<u32> = plans.iter().map(|p| p.cpu).collect();
    cpus.sort_unstable();
    cpus.dedup();
    cpus
}

/// そのコア数と組み合わせられるメモリ（MB）を昇順で取り出す。
pub fn memory_choices(plans: &[ServerPlan], cpu: u32) -> Vec<u32> {
    let mut mem: Vec<u32> = plans
        .iter()
        .filter(|p| p.cpu == cpu)
        .map(|p| p.memory_mb)
        .collect();
    mem.sort_unstable();
    mem.dedup();
    mem
}

/// コア数を変えたとき、その構成で選べるメモリのうち希望に一番近いものを返す。
///
/// コア数ごとに選べるメモリが違うので、そのまま持ち越すと存在しない組み合わせになる。
pub fn nearest_memory(plans: &[ServerPlan], cpu: u32, want_mb: u32) -> u32 {
    memory_choices(plans, cpu)
        .into_iter()
        .min_by_key(|m| m.abs_diff(want_mb))
        .unwrap_or(want_mb)
}

/// その組み合わせのプランが実際にあるか。
pub fn plan_exists(plans: &[ServerPlan], cpu: u32, memory_mb: u32) -> bool {
    plans
        .iter()
        .any(|p| p.cpu == cpu && p.memory_mb == memory_mb)
}

/// 単体で作るディスクの指定。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskCreateInput {
    pub name: String,
    pub description: String,
    pub plan_id: u32,
    pub size_mb: u32,
    /// OS テンプレートのタグ。空なら OS テンプレートは使わない。
    pub os_tags: Vec<String>,
    /// 元にするアーカイブ。`os_tags` とどちらか一方だけ使う。
    pub source_archive: Option<ResourceId>,
}

/// スタートアップスクリプト（API 上は Note）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupScript {
    pub id: ResourceId,
    pub name: String,
    /// `shell` / `yaml_cloud_config` など。
    pub class: String,
    /// `user`（自分で作ったもの）か `shared`（さくらの公開スクリプト）。
    pub scope: String,
    /// 本文から作られる要約。何をするスクリプトかの手がかりになる。
    pub description: String,
    pub tags: Vec<String>,
}

impl StartupScript {
    /// 自分で作ったものか。共有のものより先に出す。
    pub fn is_own(&self) -> bool {
        self.scope == "user"
    }
}

/// アカウントに登録済みの SSH 公開鍵。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshKey {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    pub public_key: String,
    pub fingerprint: String,
    pub created_at: Option<String>,
}

/// ディスクプランと、そのプランで選べるサイズ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskPlan {
    pub id: u32,
    pub name: String,
    /// 選べるサイズ（MB）。ここに無い値は指定できない。
    pub sizes_mb: Vec<u32>,
}

impl DiskPlan {
    pub fn is_ssd(&self) -> bool {
        self.id == DISK_PLAN_SSD
    }
}

/// OS テンプレート（パブリックアーカイブ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsTemplate {
    pub id: ResourceId,
    pub name: String,
    pub size_mb: u32,
}

/// `--os-type` 相当の選択肢。公式SDKのタグ条件をそのまま持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsChoice {
    pub label: &'static str,
    /// AND 条件で使うタグ。
    pub tags: &'static [&'static str],
}

/// 画面に出す OS の選択肢。
///
/// `current-stable` と `distro-*` の AND でその配布物の最新安定版が引ける。
pub const OS_CHOICES: [OsChoice; 6] = [
    OsChoice {
        label: "Ubuntu (最新安定版)",
        tags: &["current-stable", "distro-ubuntu"],
    },
    OsChoice {
        label: "Rocky Linux (最新安定版)",
        tags: &["current-stable", "distro-rocky"],
    },
    OsChoice {
        label: "AlmaLinux (最新安定版)",
        tags: &["current-stable", "distro-alma"],
    },
    OsChoice {
        label: "Debian (最新安定版)",
        tags: &["current-stable", "distro-debian"],
    },
    OsChoice {
        label: "MIRACLE LINUX (最新安定版)",
        tags: &["current-stable", "distro-miracle"],
    },
    OsChoice {
        label: "Ubuntu 24.04",
        tags: &["ubuntu-24.04-latest"],
    },
];

/// サーバー作成の入力。
#[derive(Debug, Clone, Default)]
pub struct ServerCreateInput {
    pub name: String,
    pub description: String,
    pub cpu: u32,
    pub memory_mb: u32,
    /// OS テンプレートを引くためのタグ。空ならディスクを作らない。
    pub os_tags: Vec<String>,
    pub disk_size_mb: u32,
    pub disk_plan_id: u32,
    pub host_name: String,
    pub password: String,
    /// 公開鍵。空なら送らない。
    pub ssh_public_key: String,
    /// パスワード認証を止めるか。公開鍵を入れたときだけ意味がある。
    pub disable_password_auth: bool,
    /// eth0 の接続先。
    pub nic: NicPlan,
    /// eth0 に付けるパケットフィルタ。作成後に別の呼び出しで付ける。
    pub packet_filter_id: Option<ResourceId>,
    /// ディスクの修正で流すスタートアップスクリプト。
    pub startup_script_id: Option<ResourceId>,
    /// 作成後に起動するか。
    pub boot_after_create: bool,
}

/// eth0 をどこに繋ぐか。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NicPlan {
    /// 共有セグメント。IP は自動で割り当てられる。
    #[default]
    Shared,
    /// スイッチ。IP は自分で決める必要がある。
    Switch {
        id: ResourceId,
        ip_address: String,
        mask_len: u32,
        gateway: String,
    },
    /// どこにも繋がない。
    None,
}

impl NicPlan {
    /// サーバー作成に渡す `ConnectedSwitches`。
    fn connected_switches(&self) -> serde_json::Value {
        match self {
            // 共有セグメントは eth0 にしか付けられない。
            Self::Shared => json!([{ "Scope": "shared" }]),
            Self::Switch { id, .. } => json!([{ "ID": id.0 }]),
            // 空の要素で NIC だけ作る。
            Self::None => json!([null]),
        }
    }

    /// スイッチに繋ぐときは DHCP が無いので、ディスクの修正で IP を書き込む。
    fn disk_network_config(&self) -> Option<(String, u32, String)> {
        match self {
            Self::Switch {
                ip_address,
                mask_len,
                gateway,
                ..
            } if !ip_address.is_empty() => Some((ip_address.clone(), *mask_len, gateway.clone())),
            _ => None,
        }
    }
}

/// 作成の途中経過。失敗しても、どこまで作ったかを呼び出し側へ返す。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerCreateProgress {
    pub server_id: Option<ResourceId>,
    pub disk_id: Option<ResourceId>,
}

#[derive(Debug, Deserialize)]
struct NakedProductServerPlan {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "CPU", default, deserialize_with = "null_as_default")]
    cpu: u32,
    #[serde(rename = "MemoryMB", default, deserialize_with = "null_as_default")]
    memory_mb: u32,
    #[serde(rename = "Commitment", default, deserialize_with = "null_as_default")]
    commitment: String,
    #[serde(rename = "Generation", default, deserialize_with = "null_as_default")]
    generation: u32,
    #[serde(rename = "Availability", default, deserialize_with = "null_as_default")]
    availability: String,
}

#[derive(Debug, Deserialize)]
struct NakedNote {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Class", default, deserialize_with = "null_as_default")]
    class: String,
    #[serde(rename = "Scope", default, deserialize_with = "null_as_default")]
    scope: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NakedSshKey {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "PublicKey", default, deserialize_with = "null_as_default")]
    public_key: String,
    #[serde(rename = "Fingerprint", default, deserialize_with = "null_as_default")]
    fingerprint: String,
    #[serde(rename = "CreatedAt", default, deserialize_with = "null_as_default")]
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct NakedDiskPlan {
    #[serde(rename = "ID", default, deserialize_with = "null_as_default")]
    id: u32,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Size", default, deserialize_with = "null_as_default")]
    size: Vec<NakedDiskSize>,
}

#[derive(Debug, Deserialize)]
struct NakedDiskSize {
    #[serde(rename = "SizeMB", default, deserialize_with = "null_as_default")]
    size_mb: u32,
    #[serde(rename = "Availability", default, deserialize_with = "null_as_default")]
    availability: String,
}

#[derive(Debug, Deserialize)]
struct NakedArchive {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "SizeMB", default, deserialize_with = "null_as_default")]
    size_mb: u32,
}

fn parse_server_plans(value: &serde_json::Value) -> Vec<ServerPlan> {
    let raw: Vec<NakedProductServerPlan> = value
        .get("ServerPlans")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mut plans: Vec<ServerPlan> = raw
        .into_iter()
        .map(|p| ServerPlan {
            name: p.name,
            cpu: p.cpu,
            memory_mb: p.memory_mb,
            commitment: p.commitment,
            generation: p.generation,
            availability: p.availability,
        })
        // 共有CPUの、今使えるプランだけを出す。
        .filter(|p| p.commitment == "standard" && p.is_available())
        .collect();
    // 同じ構成が世代違いで複数あるので、コア・メモリで1つに畳む。
    plans.sort_by_key(|p| (p.cpu, p.memory_mb, std::cmp::Reverse(p.generation)));
    plans.dedup_by_key(|p| (p.cpu, p.memory_mb));
    plans
}

/// Note のうち、サーバー作成で流せないもの。
const NOT_STARTUP_SCRIPT_CLASSES: [&str; 2] = ["json_packetfilter", "hcl_terraform"];

fn parse_startup_scripts(value: &serde_json::Value) -> Vec<StartupScript> {
    let raw: Vec<NakedNote> = value
        .get("Notes")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mut scripts: Vec<StartupScript> = raw
        .into_iter()
        // Note にはパケットフィルタや Terraform のひな形も入っている。
        // ディスクの修正で流せないものは出しても選べないので落とす。
        // 知らない種別は残す（増えたときに黙って消えない方がよい）。
        .filter(|n| !NOT_STARTUP_SCRIPT_CLASSES.contains(&n.class.as_str()))
        .map(|n| StartupScript {
            id: n.id,
            name: n.name,
            class: n.class,
            scope: n.scope,
            description: n.description,
            tags: n.tags,
        })
        .collect();
    // 自分で作ったものを先に出す。共有のものは数十件あって埋もれる。
    scripts.sort_by(|a, b| {
        b.is_own()
            .cmp(&a.is_own())
            .then_with(|| a.name.cmp(&b.name))
    });
    scripts
}

fn parse_ssh_keys(value: &serde_json::Value) -> Vec<SshKey> {
    let raw: Vec<NakedSshKey> = value
        .get("SSHKeys")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    raw.into_iter()
        .filter(|k| !k.public_key.trim().is_empty())
        .map(|k| SshKey {
            id: k.id,
            name: k.name,
            description: k.description,
            public_key: k.public_key.trim().to_string(),
            fingerprint: k.fingerprint,
            created_at: (!k.created_at.is_empty()).then_some(k.created_at),
        })
        .collect()
}

fn parse_disk_plans(value: &serde_json::Value) -> Vec<DiskPlan> {
    let raw: Vec<NakedDiskPlan> = value
        .get("DiskPlans")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    raw.into_iter()
        .map(|p| DiskPlan {
            id: p.id,
            name: p.name,
            sizes_mb: p
                .size
                .into_iter()
                .filter(|s| s.availability.is_empty() || s.availability == "available")
                .map(|s| s.size_mb)
                .collect(),
        })
        .filter(|p| p.id == DISK_PLAN_SSD || p.id == DISK_PLAN_HDD)
        .collect()
}

impl SacloudClient {
    /// 選べるサーバープラン。
    pub async fn list_server_plans(&self, zone: &str) -> Result<Vec<ServerPlan>> {
        let body = json!({ "Count": 1000, "Sort": ["-Generation"] });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, "product/server", Some(body))
            .await?;
        Ok(parse_server_plans(&value))
    }

    /// 選べるディスクプランとサイズ。
    pub async fn list_disk_plans(&self, zone: &str) -> Result<Vec<DiskPlan>> {
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, "product/disk", None)
            .await?;
        Ok(parse_disk_plans(&value))
    }

    /// アカウントに登録済みの SSH 公開鍵。
    ///
    /// 鍵はゾーンをまたいで共通だが、API はゾーン付きのパスにしかないので
    /// 表示中のゾーンに聞く。
    pub async fn list_ssh_keys(&self, zone: &str) -> Result<Vec<SshKey>> {
        let body = json!({ "Count": 1000, "Sort": ["Name"] });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, "sshkey", Some(body))
            .await?;
        Ok(parse_ssh_keys(&value))
    }

    /// SSH 公開鍵を登録する。
    pub async fn create_ssh_key(
        &self,
        zone: &str,
        name: &str,
        description: &str,
        public_key: &str,
    ) -> Result<ResourceId> {
        let body = json!({
            "SSHKey": {
                "Name": name,
                "Description": description,
                "PublicKey": public_key,
            }
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "sshkey", Some(body))
            .await?;
        value
            .pointer("/SSHKey/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("公開鍵の登録応答にIDがありませんでした"))
    }

    /// 登録済みの鍵の名前と説明を書き換える。鍵そのものは変えられない。
    pub async fn update_ssh_key(
        &self,
        zone: &str,
        id: ResourceId,
        name: &str,
        description: &str,
    ) -> Result<()> {
        let body = json!({ "SSHKey": { "Name": name, "Description": description } });
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::PUT, &format!("sshkey/{id}"), Some(body))
            .await?;
        Ok(())
    }

    pub async fn delete_ssh_key(&self, zone: &str, id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &format!("sshkey/{id}"), None)
            .await?;
        Ok(())
    }

    /// タグから OS テンプレートを1件引く。
    ///
    /// 公式SDKと同じく、タグの AND 条件と `Scope: shared` で絞って先頭を採る。
    pub async fn find_os_template(&self, zone: &str, tags: &[String]) -> Result<OsTemplate> {
        // 入れ子の配列にすると AND 条件になる。
        let body = json!({
            "Filter": { "Tags.Name": [tags], "Scope": ["shared"] },
            "Count": 1,
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, "archive", Some(body))
            .await?;
        let archives: Vec<NakedArchive> = value
            .get("Archives")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let archive = archives.into_iter().next().ok_or_else(|| {
            anyhow::anyhow!(
                "該当するOSテンプレートが見つかりませんでした: {}",
                tags.join(", ")
            )
        })?;
        Ok(OsTemplate {
            id: archive.id,
            name: archive.name,
            size_mb: archive.size_mb,
        })
    }
}

impl SacloudClient {
    /// サーバーを作る。
    ///
    /// 途中で失敗しても、そこまでに作ったものの ID を返す。呼び出し側が
    /// 後始末を案内できるようにするため、`Result` ではなく途中経過を添える。
    ///
    /// 手順は公式SDKに合わせた。ディスクは作成・修正・サーバー接続を
    /// 1 回の POST で済ませ、コピー完了を待ってから起動する。
    pub async fn create_server(
        &self,
        zone: &str,
        input: &ServerCreateInput,
    ) -> (ServerCreateProgress, Result<ResourceId>) {
        let mut progress = ServerCreateProgress::default();

        let server_id = match self.create_server_only(zone, input).await {
            Ok(id) => id,
            Err(err) => return (progress, Err(err)),
        };
        progress.server_id = Some(server_id);

        // OS を選んでいなければディスクは作らない。
        if input.os_tags.is_empty() {
            return (progress, Ok(server_id));
        }

        let template = match self.find_os_template(zone, &input.os_tags).await {
            Ok(t) => t,
            Err(err) => return (progress, Err(err)),
        };
        let disk_id = match self
            .create_disk_for_server(zone, input, server_id, template.id)
            .await
        {
            Ok(id) => id,
            Err(err) => return (progress, Err(err)),
        };
        progress.disk_id = Some(disk_id);

        if let Err(err) = self.wait_disk_ready(zone, disk_id).await {
            return (progress, Err(err));
        }
        if let Some(filter_id) = input.packet_filter_id
            && let Err(err) = self.attach_packet_filter(zone, server_id, filter_id).await
        {
            return (progress, Err(err));
        }
        if input.boot_after_create
            && let Err(err) = self.power_action(zone, server_id, PowerAction::Boot).await
        {
            return (progress, Err(err));
        }
        (progress, Ok(server_id))
    }

    /// サーバーに NIC を1枚足す。追加できるのは停止中のときだけ。
    pub async fn add_nic(&self, zone: &str, server_id: ResourceId) -> Result<ResourceId> {
        let body = json!({ "Interface": { "Server": { "ID": server_id.0 } } });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "interface", Some(body))
            .await?;
        value
            .pointer("/Interface/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("NICの追加応答にIDがありませんでした"))
    }

    pub async fn delete_nic(&self, zone: &str, nic_id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &format!("interface/{nic_id}"), None)
            .await?;
        Ok(())
    }

    /// NIC の繋ぎ先を変える。共有セグメントだけパスが別。
    pub async fn connect_nic(&self, zone: &str, nic_id: ResourceId, to: NicAttach) -> Result<()> {
        let path = match to {
            NicAttach::Shared => format!("interface/{nic_id}/to/switch/shared"),
            NicAttach::Switch(id) => format!("interface/{nic_id}/to/switch/{id}"),
            NicAttach::None => return self.disconnect_nic(zone, nic_id).await,
        };
        let _: serde_json::Value = self.request_in_zone(zone, Method::PUT, &path, None).await?;
        Ok(())
    }

    pub async fn disconnect_nic(&self, zone: &str, nic_id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(
                zone,
                Method::DELETE,
                &format!("interface/{nic_id}/to/switch"),
                None,
            )
            .await?;
        Ok(())
    }

    /// NIC にパケットフィルタを付ける。`None` なら外す。
    pub async fn set_nic_packet_filter(
        &self,
        zone: &str,
        nic_id: ResourceId,
        filter_id: Option<ResourceId>,
    ) -> Result<()> {
        let (method, path) = match filter_id {
            Some(id) => (
                Method::PUT,
                format!("interface/{nic_id}/to/packetfilter/{id}"),
            ),
            None => (
                Method::DELETE,
                format!("interface/{nic_id}/to/packetfilter"),
            ),
        };
        let _: serde_json::Value = self.request_in_zone(zone, method, &path, None).await?;
        Ok(())
    }

    /// eth0 にパケットフィルタを付ける。
    ///
    /// フィルタは NIC 単位なので、サーバーを作ってから最初の NIC の ID を
    /// 引いて付ける。NIC が無ければ何もしない。
    async fn attach_packet_filter(
        &self,
        zone: &str,
        server_id: ResourceId,
        filter_id: ResourceId,
    ) -> Result<()> {
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, &format!("server/{server_id}"), None)
            .await?;
        let Some(interface_id) = value
            .pointer("/Server/Interfaces/0/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
        else {
            return Ok(());
        };
        self.set_nic_packet_filter(zone, interface_id, Some(filter_id))
            .await
    }

    /// 自分で作ったアーカイブ。ディスクの作成元に選べるもの。
    ///
    /// 共有のもの（OS テンプレート）は [`OS_CHOICES`] から引くので出さない。
    pub async fn list_own_archives(&self, zone: &str) -> Result<Vec<OsTemplate>> {
        let body = json!({
            "Filter": { "Scope": ["user"] },
            "Count": 1000,
            "Sort": ["-CreatedAt"],
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, "archive", Some(body))
            .await?;
        let archives: Vec<NakedArchive> = value
            .get("Archives")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        Ok(archives
            .into_iter()
            .map(|a| OsTemplate {
                id: a.id,
                name: a.name,
                size_mb: a.size_mb,
            })
            .collect())
    }

    /// ディスクからアーカイブを取る。
    ///
    /// コピーが走るので、使えるようになるまで時間がかかる。ここでは待たない。
    pub async fn create_archive(
        &self,
        zone: &str,
        name: &str,
        description: &str,
        source_disk_id: ResourceId,
    ) -> Result<ResourceId> {
        let body = json!({
            "Archive": {
                "Name": name,
                "Description": description,
                "SourceDisk": { "ID": source_disk_id.0 },
            }
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "archive", Some(body))
            .await?;
        value
            .pointer("/Archive/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("アーカイブの作成応答にIDがありませんでした"))
    }

    pub async fn delete_archive(&self, zone: &str, id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &format!("archive/{id}"), None)
            .await?;
        Ok(())
    }

    /// スタートアップスクリプト（Note）の一覧。
    pub async fn list_startup_scripts(&self, zone: &str) -> Result<Vec<StartupScript>> {
        let body = json!({ "Count": 1000, "Sort": ["Name"] });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, "note", Some(body))
            .await?;
        Ok(parse_startup_scripts(&value))
    }

    async fn create_server_only(
        &self,
        zone: &str,
        input: &ServerCreateInput,
    ) -> Result<ResourceId> {
        // プランは 2025 年の仕様変更で ID 指定が廃止され、CPU とメモリを直接渡す。
        let body = json!({
            "Server": {
                "Name": input.name,
                "Description": input.description,
                "ServerPlan": {
                    "CPU": input.cpu,
                    "MemoryMB": input.memory_mb,
                    "Commitment": "standard",
                },
                "InterfaceDriver": "virtio",
                "ConnectedSwitches": input.nic.connected_switches(),
            }
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "server", Some(body))
            .await?;
        value
            .pointer("/Server/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("サーバーの作成応答にIDがありませんでした"))
    }

    async fn create_disk_for_server(
        &self,
        zone: &str,
        input: &ServerCreateInput,
        server_id: ResourceId,
        archive_id: ResourceId,
    ) -> Result<ResourceId> {
        let mut config = json!({
            "Background": true,
            "HostName": input.host_name,
            "Password": input.password,
        });
        if !input.ssh_public_key.is_empty() {
            config["SSHKeys"] = json!([{ "PublicKey": input.ssh_public_key }]);
            config["DisablePWAuth"] = json!(input.disable_password_auth);
        }
        if let Some(id) = input.startup_script_id {
            config["Notes"] = json!([{ "ID": id.0 }]);
        }
        // スイッチに繋いだ NIC は DHCP が無いので、ここで IP を焼き込む。
        if let Some((ip, mask_len, gateway)) = input.nic.disk_network_config() {
            config["UserIPAddress"] = json!(ip);
            config["UserSubnet"] = json!({
                "DefaultRoute": gateway,
                "NetworkMaskLen": mask_len,
            });
        }
        let body = json!({
            "Disk": {
                "Name": format!("{}-disk", input.name),
                "Plan": { "ID": input.disk_plan_id },
                "SizeMB": input.disk_size_mb,
                "Connection": "virtio",
                "SourceArchive": { "ID": archive_id.0 },
                // ここでサーバーへの接続まで済ませる。
                "Server": { "ID": server_id.0 },
            },
            "Config": config,
            "BootAtAvailable": false,
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "disk", Some(body))
            .await?;
        value
            .pointer("/Disk/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("ディスクの作成応答にIDがありませんでした"))
    }

    /// ディスクのコピーが終わるまで待つ。
    pub async fn wait_disk_ready(&self, zone: &str, id: ResourceId) -> Result<()> {
        for _ in 0..DISK_POLL_MAX {
            let (availability, _) = self.disk_progress(zone, id).await?;
            match availability.as_str() {
                "available" => return Ok(()),
                "failed" => anyhow::bail!("ディスクの作成に失敗しました"),
                _ => {}
            }
            tokio::time::sleep(DISK_POLL_INTERVAL).await;
        }
        anyhow::bail!("ディスクの作成が時間内に終わりませんでした")
    }

    /// ディスクの状態と、コピーの進み具合（0.0〜1.0）。
    pub async fn disk_progress(&self, zone: &str, id: ResourceId) -> Result<(String, f64)> {
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, &format!("disk/{id}"), None)
            .await?;
        let availability = value
            .pointer("/Disk/Availability")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let migrated = value
            .pointer("/Disk/MigratedMB")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let total = value
            .pointer("/Disk/SizeMB")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0);
        let ratio = if total > 0.0 { migrated / total } else { 0.0 };
        Ok((availability, ratio.clamp(0.0, 1.0)))
    }

    /// サーバーを消す。接続中のディスクも一緒に消す。
    ///
    /// 起動中は消せないので、呼び出し側で停止を確認しておくこと。
    pub async fn delete_server(
        &self,
        zone: &str,
        id: ResourceId,
        disk_ids: &[ResourceId],
    ) -> Result<()> {
        let with_disk: Vec<u64> = disk_ids.iter().map(|d| d.0).collect();
        let body = json!({ "WithDisk": with_disk });
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &format!("server/{id}"), Some(body))
            .await?;
        Ok(())
    }

    /// ディスクを単体で作る。
    ///
    /// OS テンプレートを指定するとコピーが走るので、完了まで数分かかる。
    /// ここでは待たずに ID を返し、進み具合は一覧の「状態」で見せる。
    pub async fn create_disk(&self, zone: &str, input: &DiskCreateInput) -> Result<ResourceId> {
        let mut disk = json!({
            "Name": input.name,
            "Description": input.description,
            "Plan": { "ID": input.plan_id },
            "SizeMB": input.size_mb,
            "Connection": "virtio",
        });
        // 元にするものは1つだけ。タグから引くか、指定された ID をそのまま使う。
        let source = match input.source_archive {
            Some(id) => Some(id),
            None if input.os_tags.is_empty() => None,
            None => Some(self.find_os_template(zone, &input.os_tags).await?.id),
        };
        if let Some(id) = source {
            disk["SourceArchive"] = json!({ "ID": id.0 });
        }
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "disk", Some(json!({ "Disk": disk })))
            .await?;
        value
            .pointer("/Disk/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("ディスクの作成応答にIDがありませんでした"))
    }

    pub async fn delete_disk(&self, zone: &str, id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &format!("disk/{id}"), None)
            .await?;
        Ok(())
    }

    /// ディスクをサーバーに繋ぐ。相手のサーバーは停止中でなければならない。
    pub async fn connect_disk(
        &self,
        zone: &str,
        id: ResourceId,
        server_id: ResourceId,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(
                zone,
                Method::PUT,
                &format!("disk/{id}/to/server/{server_id}"),
                None,
            )
            .await?;
        Ok(())
    }

    /// ディスクをサーバーから外す。接続先はパスに含めない。
    pub async fn disconnect_disk(&self, zone: &str, id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &format!("disk/{id}/to/server"), None)
            .await?;
        Ok(())
    }

    /// サーバーのプランを変える。
    ///
    /// 停止中しか受け付けない。またプランを変えると **サーバーの ID が変わる**
    /// ので、新しい ID を返す（画面側で選び直すのに使う）。
    /// ディスクと NIC はそのまま引き継がれる。
    ///
    /// プラン ID 指定は 2025-04-17 に廃止されたため、作成と同じく
    /// コア数とメモリを直接送る。
    pub async fn change_server_plan(
        &self,
        zone: &str,
        id: ResourceId,
        cpu: u32,
        memory_mb: u32,
    ) -> Result<ResourceId> {
        let body = json!({
            "CPU": cpu,
            "MemoryMB": memory_mb,
            "Commitment": "standard",
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::PUT, &format!("server/{id}/plan"), Some(body))
            .await?;
        value
            .pointer("/Server/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("プラン変更の応答にIDがありませんでした"))
    }

    /// サーバーに繋がっているディスクの ID。削除のときに使う。
    pub async fn server_disk_ids(&self, zone: &str, id: ResourceId) -> Result<Vec<ResourceId>> {
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, &format!("server/{id}"), None)
            .await?;
        Ok(value
            .pointer("/Server/Disks")
            .and_then(serde_json::Value::as_array)
            .map(|disks| {
                disks
                    .iter()
                    .filter_map(|d| d.get("ID"))
                    .filter_map(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default())
    }
}
