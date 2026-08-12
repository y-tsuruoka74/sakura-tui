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

/// API から取得できないときに使うゾーン一覧。
///
/// 認証情報を作る前はゾーン一覧を引けないため、既知のものを並べておく。
/// （sacloud/iaas-api-go の `types::ZoneNames` と同じ並び）
pub const KNOWN_ZONES: [(&str, &str); 6] = [
    ("tk1a", "東京第1"),
    ("tk1b", "東京第2"),
    ("is1a", "石狩第1"),
    ("is1b", "石狩第2"),
    ("is1c", "石狩第3"),
    ("tk1v", "サンドボックス"),
];

/// 既知のゾーンを `Zone` として返す。
pub fn known_zones() -> Vec<Zone> {
    KNOWN_ZONES
        .iter()
        .map(|(name, description)| Zone {
            name: name.to_string(),
            description: description.to_string(),
        })
        .collect()
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
    pub zone: String,
    pub created_at: Option<String>,
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
    #[serde(rename = "IPAddress", default, deserialize_with = "null_as_default")]
    ip_address: String,
    #[serde(
        rename = "UserIPAddress",
        default,
        deserialize_with = "null_as_default"
    )]
    user_ip_address: String,
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
            ip_addresses: naked
                .interfaces
                .into_iter()
                // 共有セグメントは IPAddress、スイッチ接続は UserIPAddress に入る。
                .map(|nic| {
                    if nic.ip_address.is_empty() {
                        nic.user_ip_address
                    } else {
                        nic.ip_address
                    }
                })
                .filter(|ip| !ip.is_empty())
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
