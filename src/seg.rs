//! さくらのクラウド サービスエンドポイントゲートウェイ（SEG）の読み取り専用クライアント。
//!
//! プライベートネットワークから、さくらのマネージドサービス（オブジェクト
//! ストレージ、コンテナレジストリ、モニタリングスイート、AppRun専有型の
//! コントロールプレーン）へグローバル経路を通さずに到達するためのゲートウェイ。
//!
//! NoSQL と同じく IaaS API 1.1 のアプライアンス（`/appliance`、`Class` は
//! `serviceendpointgateway`）なので [`SacloudClient`] をそのまま使う。
//!
//! 読み取り系APIは仕様上4本あるが、`/appliance/{id}`・`/appliance/{id}/power`・
//! `/appliance/{id}/interface/{ifID}` はいずれも一覧に含まれる情報の部分集合で、
//! 呼んでも新しい情報が増えない。そのため一覧の1本だけを使う。

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{SacloudClient, flexible_number, null_as_default};

/// 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が実態と違う総件数を返しても止まるようにする。
const MAX_PAGES: usize = 100;

/// アプライアンス一覧を SEG に絞り込むクラス名。
const SEG_CLASS: &str = "serviceendpointgateway";

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// サービスエンドポイントゲートウェイ 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seg {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// `available` / `unavailable` / `migrating`。
    pub availability: String,
    /// `up` / `down` / `cleaning`。
    pub status: String,
    pub status_changed_at: String,
    pub created_at: String,
    pub plan_id: u64,
    pub service_class: String,
    pub switch_id: String,
    pub switch_name: String,
    /// `user` / `shared`。
    pub switch_scope: String,
    pub zone: String,
    /// ゲートウェイ側のIPアドレス。
    pub ip_addresses: Vec<String>,
    pub user_ip_addresses: Vec<String>,
    pub network_mask_len: u32,
    /// 接続元サーバーのIPアドレス（`Remark.Servers`）。
    pub server_ip_addresses: Vec<String>,
    pub monitoring_suite_enabled: bool,
    pub dns_forwarding: Option<SegDnsForwarding>,
    /// 接続先マネージドサービス。エンドポイント単位に開いてある。
    pub services: Vec<SegService>,
}

impl Seg {
    /// 一覧の「状態」列に出す文字列。
    ///
    /// `Availability` と `Instance.Status` は別軸なので、食い違うときだけ併記する。
    pub fn status_label(&self) -> String {
        let availability = availability_label(&self.availability);
        let instance = instance_status_label(&self.status);
        match (availability.is_empty(), instance.is_empty()) {
            (true, true) => String::new(),
            (true, false) => instance,
            (false, true) => availability,
            (false, false) if availability == instance => availability,
            // 「移行中 / 停止」のように、可用性と稼働状態がずれている状況を隠さない。
            (false, false) => format!("{availability} / {instance}"),
        }
    }

    /// 一覧の「IPアドレス」列に出す文字列。
    pub fn ip_label(&self) -> String {
        if !self.ip_addresses.is_empty() {
            return self.ip_addresses.join(", ");
        }
        self.user_ip_addresses.join(", ")
    }
}

/// DNSプライベートホストゾーンの設定。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegDnsForwarding {
    pub enabled: bool,
    pub private_hosted_zone: String,
    /// フォワード先のDNSサーバ。空のものは落としてある。
    pub upstream_dns: Vec<String>,
}

/// 接続先マネージドサービス 1 エンドポイント。
///
/// 仕様上は 1 つの `Type` が複数のエンドポイントを持てるため、表に出しやすいよう
/// エンドポイント単位へ開いて持つ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegService {
    /// `ObjectStorage` / `ContainerRegistry` / `MonitoringSuite` /
    /// `AppRunDedicatedControlPlane`。
    pub kind: String,
    /// エンドポイント。自動設定のものは空。
    pub endpoint: String,
    /// `Mode: Managed`。エンドポイントを指定できない種別で立つ。
    pub managed: bool,
}

impl SegService {
    pub fn kind_label(&self) -> String {
        match self.kind.as_str() {
            "ObjectStorage" => "オブジェクトストレージ".to_string(),
            "ContainerRegistry" => "コンテナレジストリ".to_string(),
            "MonitoringSuite" => "モニタリングスイート".to_string(),
            "AppRunDedicatedControlPlane" => "AppRun専有型コントロールプレーン".to_string(),
            other => other.to_string(),
        }
    }

    /// 「設定方法」列に出す文字列。
    pub fn mode_label(&self) -> &'static str {
        if self.managed {
            "自動設定"
        } else {
            "手動指定"
        }
    }
}

// ---------------------------------------------------------------------------
// ラベル変換
// ---------------------------------------------------------------------------

fn availability_label(raw: &str) -> String {
    match raw {
        "available" => "稼働".to_string(),
        "unavailable" => "停止".to_string(),
        "migrating" => "移行中".to_string(),
        other => other.to_string(),
    }
}

fn instance_status_label(raw: &str) -> String {
    match raw {
        "up" => "起動".to_string(),
        "down" => "停止".to_string(),
        "cleaning" => "クリーニング中".to_string(),
        other => other.to_string(),
    }
}

/// 仕様上 `Enabled` は boolean ではなく文字列 `'True'` / `'False'` で返る。
fn parse_enabled(raw: &str) -> bool {
    raw.eq_ignore_ascii_case("true")
}

// ---------------------------------------------------------------------------
// デシリアライズ用の内部型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SegListResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Total")]
    total: usize,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Appliances")]
    appliances: Vec<RawSeg>,
}

#[derive(Debug, Deserialize)]
struct RawSeg {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Class")]
    class: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Description")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Tags")]
    tags: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Availability")]
    availability: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ServiceClass")]
    service_class: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "CreatedAt")]
    created_at: String,
    #[serde(rename = "Plan")]
    plan: Option<RawPlan>,
    #[serde(rename = "Instance")]
    instance: Option<RawInstance>,
    #[serde(rename = "Settings")]
    settings: Option<RawSettings>,
    #[serde(rename = "Remark")]
    remark: Option<RawRemark>,
    #[serde(rename = "Switch")]
    switch: Option<RawSwitch>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Interfaces")]
    interfaces: Vec<Option<RawInterface>>,
}

#[derive(Debug, Deserialize)]
struct RawPlan {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "ID")]
    id: u64,
}

#[derive(Debug, Deserialize)]
struct RawInstance {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Status")]
    status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "StatusChangedAt")]
    status_changed_at: String,
}

#[derive(Debug, Deserialize)]
struct RawSettings {
    #[serde(rename = "ServiceEndpointGateway")]
    seg: Option<RawSegSettings>,
}

#[derive(Debug, Deserialize)]
struct RawSegSettings {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "EnabledServices")]
    enabled_services: Vec<Option<RawEnabledService>>,
    #[serde(rename = "MonitoringSuite")]
    monitoring_suite: Option<RawEnabledFlag>,
    #[serde(rename = "DNSForwarding")]
    dns_forwarding: Option<RawDnsForwarding>,
}

#[derive(Debug, Deserialize)]
struct RawEnabledFlag {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Enabled")]
    enabled: String,
}

#[derive(Debug, Deserialize)]
struct RawDnsForwarding {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Enabled")]
    enabled: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "PrivateHostedZone")]
    private_hosted_zone: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "UpstreamDNS1")]
    upstream_dns1: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "UpstreamDNS2")]
    upstream_dns2: String,
}

#[derive(Debug, Deserialize)]
struct RawEnabledService {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "Config")]
    config: Option<RawServiceConfig>,
}

#[derive(Debug, Deserialize)]
struct RawServiceConfig {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Endpoints")]
    endpoints: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Mode")]
    mode: String,
}

#[derive(Debug, Deserialize)]
struct RawRemark {
    #[serde(rename = "Network")]
    network: Option<RawNetworkRemark>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Servers")]
    servers: Vec<Option<RawServerRemark>>,
}

#[derive(Debug, Deserialize)]
struct RawNetworkRemark {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "NetworkMaskLen")]
    network_mask_len: u32,
}

#[derive(Debug, Deserialize)]
struct RawServerRemark {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "IPAddress")]
    ip_address: String,
}

#[derive(Debug, Deserialize)]
struct RawSwitch {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Scope")]
    scope: String,
    #[serde(rename = "Zone")]
    zone: Option<RawZone>,
}

#[derive(Debug, Deserialize)]
struct RawZone {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawInterface {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "IPAddress")]
    ip_address: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "UserIPAddress")]
    user_ip_address: String,
}

// ---------------------------------------------------------------------------
// パース
// ---------------------------------------------------------------------------

/// `EnabledServices` をエンドポイント単位の行へ開く。
///
/// `AppRunDedicatedControlPlane` のようにエンドポイントを指定できない種別は、
/// 消えてしまわないよう空のエンドポイントで 1 行だけ残す。
fn flatten_services(raw: Vec<Option<RawEnabledService>>) -> Vec<SegService> {
    let mut out = Vec::new();
    for service in raw.into_iter().flatten() {
        let config = service.config;
        let managed = config
            .as_ref()
            .map(|c| c.mode.eq_ignore_ascii_case("managed"))
            .unwrap_or(false);
        let endpoints: Vec<String> = config
            .map(|c| c.endpoints)
            .unwrap_or_default()
            .into_iter()
            .filter(|endpoint| !endpoint.is_empty())
            .collect();
        if endpoints.is_empty() {
            out.push(SegService {
                kind: service.kind,
                endpoint: String::new(),
                managed,
            });
            continue;
        }
        for endpoint in endpoints {
            out.push(SegService {
                kind: service.kind.clone(),
                endpoint,
                managed,
            });
        }
    }
    out
}

impl From<RawSeg> for Seg {
    fn from(raw: RawSeg) -> Self {
        let instance = raw.instance;
        let switch = raw.switch;
        let remark = raw.remark;
        let settings = raw.settings.and_then(|s| s.seg);

        let interfaces: Vec<RawInterface> = raw.interfaces.into_iter().flatten().collect();

        let dns_forwarding = settings
            .as_ref()
            .and_then(|s| s.dns_forwarding.as_ref())
            .map(|d| SegDnsForwarding {
                enabled: parse_enabled(&d.enabled),
                private_hosted_zone: d.private_hosted_zone.clone(),
                upstream_dns: [d.upstream_dns1.clone(), d.upstream_dns2.clone()]
                    .into_iter()
                    .filter(|dns| !dns.is_empty())
                    .collect(),
            });

        Seg {
            id: raw.id,
            name: raw.name,
            description: raw.description,
            tags: raw.tags,
            availability: raw.availability,
            status: instance
                .as_ref()
                .map(|i| i.status.clone())
                .unwrap_or_default(),
            status_changed_at: instance
                .as_ref()
                .map(|i| i.status_changed_at.clone())
                .unwrap_or_default(),
            created_at: raw.created_at,
            plan_id: raw.plan.map(|p| p.id).unwrap_or_default(),
            service_class: raw.service_class,
            switch_id: switch.as_ref().map(|s| s.id.clone()).unwrap_or_default(),
            switch_name: switch.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            switch_scope: switch.as_ref().map(|s| s.scope.clone()).unwrap_or_default(),
            zone: switch
                .as_ref()
                .and_then(|s| s.zone.as_ref())
                .map(|z| z.name.clone())
                .unwrap_or_default(),
            ip_addresses: interfaces
                .iter()
                .map(|i| i.ip_address.clone())
                .filter(|ip| !ip.is_empty())
                .collect(),
            user_ip_addresses: interfaces
                .iter()
                .map(|i| i.user_ip_address.clone())
                .filter(|ip| !ip.is_empty())
                .collect(),
            network_mask_len: remark
                .as_ref()
                .and_then(|r| r.network.as_ref())
                .map(|n| n.network_mask_len)
                .unwrap_or_default(),
            server_ip_addresses: remark
                .as_ref()
                .map(|r| {
                    r.servers
                        .iter()
                        .flatten()
                        .map(|s| s.ip_address.clone())
                        .filter(|ip| !ip.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            monitoring_suite_enabled: settings
                .as_ref()
                .and_then(|s| s.monitoring_suite.as_ref())
                .map(|m| parse_enabled(&m.enabled))
                .unwrap_or(false),
            dns_forwarding,
            services: settings
                .map(|s| flatten_services(s.enabled_services))
                .unwrap_or_default(),
        }
    }
}

fn parse_segs(body: &str) -> Result<(Vec<Seg>, usize)> {
    use anyhow::Context;
    let body = if body.trim().is_empty() { "{}" } else { body };
    let parsed: SegListResponse = serde_json::from_str(body).with_context(|| {
        let head: String = body.chars().take(200).collect();
        format!("SEG APIレスポンスの解析に失敗しました: {head}")
    })?;
    let total = parsed.total;
    let items = parsed
        .appliances
        .into_iter()
        // API 側のフィルターを信用しきらず、別のアプライアンスの混入を防ぐ。
        .filter(|raw| raw.class.is_empty() || raw.class == SEG_CLASS)
        .map(Seg::from)
        .collect();
    Ok((items, total))
}

// ---------------------------------------------------------------------------
// API 呼び出し
// ---------------------------------------------------------------------------

impl SacloudClient {
    /// ゾーン内の SEG 一覧。
    ///
    /// 仕様には 1 件取得・電源状態・インターフェース取得もあるが、いずれも
    /// この一覧に含まれる情報の部分集合なので呼ばない。
    pub async fn list_segs(&self, zone: &str) -> Result<Vec<Seg>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({
                "Filter": { "Class": SEG_CLASS },
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let value: serde_json::Value = self
                .request_in_zone(zone, Method::GET, "appliance", Some(body))
                .await?;
            let (items, total) = parse_segs(&value.to_string())?;
            let received = items.len();
            out.extend(items);
            if received == 0 || from + received >= total {
                break;
            }
            from += received;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仕様の example をほぼそのまま流し、封筒剥がしと入れ子の取り出しを固定する。
    /// `Appliance.ID` は文字列、`Plan.ID` は数値。
    #[test]
    fn parses_gateway_list_from_the_specification_example() {
        let body = r#"{
            "From": 0, "Count": 1, "Total": 1,
            "Appliances": [{
                "ID": "123456789012",
                "Class": "serviceendpointgateway",
                "Name": "Service Endpoint Gateway (123456789012)",
                "Description": "",
                "Plan": {"ID": 1},
                "Settings": {
                    "ServiceEndpointGateway": {
                        "EnabledServices": [
                            {"Type": "ObjectStorage",
                             "Config": {"Endpoints": ["s3.isk01.sakurastorage.jp"]}},
                            {"Type": "AppRunDedicatedControlPlane",
                             "Config": {"Mode": "Managed"}}
                        ],
                        "MonitoringSuite": {"Enabled": "True"},
                        "DNSForwarding": {
                            "Enabled": "True",
                            "PrivateHostedZone": "internal.example.com",
                            "UpstreamDNS1": "ns1.gslbN.sakura.ne.jp",
                            "UpstreamDNS2": "ns2.gslbN.sakura.ne.jp"
                        }
                    }
                },
                "Remark": {
                    "Switch": {"ID": "123456789012"},
                    "Network": {"NetworkMaskLen": 24},
                    "Servers": [{"IPAddress": "192.0.2.15"}],
                    "Zone": {"ID": "21001"}
                },
                "Availability": "available",
                "Instance": {
                    "Status": "up",
                    "StatusChangedAt": "2024-01-01T00:00:00+09:00",
                    "Host": {"Name": "sac-tk1a-sv001", "InfoURL": ""}
                },
                "Disk": {"EncryptionAlgorithm": "none", "EncryptionKey": null,
                         "DedicatedStorageContract": null},
                "ServiceClass": "cloud/appliance/serviceendpointgateway/1",
                "Generation": 100,
                "CreatedAt": "2024-01-01T00:00:00+09:00",
                "Icon": null,
                "Switch": {
                    "ID": "123456789012", "Name": "Switch1", "Internet": null,
                    "Scope": "user", "Availability": "available",
                    "Zone": {"ID": 21001, "Name": "tk1a", "Region": {"ID": 210, "Name": "東京"}}
                },
                "Interfaces": [{
                    "IPAddress": "203.0.113.100", "UserIPAddress": null, "HostName": null,
                    "Switch": {"ID": "123456789012", "Name": "Switch", "Scope": "shared"}
                }],
                "Tags": []
            }],
            "is_ok": true
        }"#;
        let (items, total) = parse_segs(body).unwrap();
        assert_eq!(total, 1);
        let seg = &items[0];
        assert_eq!(seg.id, "123456789012");
        assert_eq!(seg.plan_id, 1);
        assert_eq!(seg.switch_name, "Switch1");
        assert_eq!(seg.switch_scope, "user");
        assert_eq!(seg.zone, "tk1a");
        assert_eq!(seg.ip_addresses, vec!["203.0.113.100".to_string()]);
        assert!(seg.user_ip_addresses.is_empty());
        assert_eq!(seg.network_mask_len, 24);
        assert_eq!(seg.server_ip_addresses, vec!["192.0.2.15".to_string()]);
        assert_eq!(seg.status_label(), "稼働 / 起動");
        assert!(seg.monitoring_suite_enabled);

        let dns = seg.dns_forwarding.as_ref().unwrap();
        assert!(dns.enabled);
        assert_eq!(dns.private_hosted_zone, "internal.example.com");
        assert_eq!(dns.upstream_dns.len(), 2);
    }

    /// 接続先サービスはエンドポイント単位の行に開く。
    /// エンドポイントを持たない種別も 1 行として残す。
    #[test]
    fn enabled_services_expand_to_one_row_per_endpoint() {
        let body = r#"{
            "Total": 1,
            "Appliances": [{
                "Class": "serviceendpointgateway", "ID": "1",
                "Settings": {"ServiceEndpointGateway": {"EnabledServices": [
                    {"Type": "ObjectStorage", "Config": {"Endpoints": [
                        "s3.isk01.sakurastorage.jp", "s3.tky01.sakurastorage.jp"]}},
                    {"Type": "AppRunDedicatedControlPlane", "Config": {"Mode": "Managed"}}
                ]}}
            }]
        }"#;
        let (items, _) = parse_segs(body).unwrap();
        let services = &items[0].services;
        assert_eq!(services.len(), 3);

        assert_eq!(services[0].kind_label(), "オブジェクトストレージ");
        assert_eq!(services[0].endpoint, "s3.isk01.sakurastorage.jp");
        assert!(!services[0].managed);
        assert_eq!(services[0].mode_label(), "手動指定");
        assert_eq!(services[1].endpoint, "s3.tky01.sakurastorage.jp");

        // エンドポイントを指定できない種別も消えない。
        assert_eq!(services[2].kind_label(), "AppRun専有型コントロールプレーン");
        assert_eq!(services[2].endpoint, "");
        assert!(services[2].managed);
        assert_eq!(services[2].mode_label(), "自動設定");
    }

    /// `Enabled` は boolean ではなく文字列 `'True'` / `'False'` で返る。
    #[test]
    fn enabled_is_a_string_not_a_boolean() {
        assert!(parse_enabled("True"));
        assert!(parse_enabled("true"));
        assert!(!parse_enabled("False"));
        assert!(!parse_enabled(""));

        let body = r#"{
            "Total": 1,
            "Appliances": [{
                "Class": "serviceendpointgateway", "ID": "1",
                "Settings": {"ServiceEndpointGateway": {
                    "EnabledServices": [],
                    "MonitoringSuite": {"Enabled": "False"},
                    "DNSForwarding": {"Enabled": "False", "PrivateHostedZone": "",
                                      "UpstreamDNS1": "", "UpstreamDNS2": ""}
                }}
            }]
        }"#;
        let (items, _) = parse_segs(body).unwrap();
        assert!(!items[0].monitoring_suite_enabled);
        let dns = items[0].dns_forwarding.as_ref().unwrap();
        assert!(!dns.enabled);
        // 空のフォワード先は落とす。
        assert!(dns.upstream_dns.is_empty());
    }

    /// nullable の項目が軒並み null でも落ちないこと。
    #[test]
    fn tolerates_nulls_across_nullable_fields() {
        let body = r#"{
            "Total": 1,
            "Appliances": [{
                "Class": "serviceendpointgateway", "ID": "1", "Name": "seg",
                "Tags": null,
                "Settings": null,
                "SettingsHash": null,
                "Instance": {"Status": null, "StatusChangedAt": null},
                "Disk": {"EncryptionKey": null, "DedicatedStorageContract": null},
                "Icon": null,
                "Switch": {"ID": "2", "Name": "sw", "Internet": null, "Scope": "user",
                           "Zone": null},
                "Remark": {"Network": null, "Servers": null},
                "Interfaces": [null, {"IPAddress": null, "UserIPAddress": null,
                                      "HostName": null}]
            }]
        }"#;
        let (items, _) = parse_segs(body).unwrap();
        let seg = &items[0];
        assert!(seg.tags.is_empty());
        assert!(seg.services.is_empty());
        assert!(seg.dns_forwarding.is_none());
        assert!(!seg.monitoring_suite_enabled);
        assert!(seg.ip_addresses.is_empty());
        assert!(seg.zone.is_empty());
        assert_eq!(seg.status_label(), "");
        assert_eq!(seg.ip_label(), "");
    }

    /// API 側のフィルターが効かず別クラスが混ざっても取り除くこと。
    #[test]
    fn drops_appliances_of_other_classes() {
        let body = r#"{
            "Total": 2,
            "Appliances": [
                {"Class": "serviceendpointgateway", "ID": "1", "Name": "keep"},
                {"Class": "loadbalancer", "ID": "2", "Name": "drop"}
            ]
        }"#;
        let (items, _) = parse_segs(body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "keep");
    }

    /// 可用性と稼働状態がずれていたら両方見せる。
    #[test]
    fn status_label_keeps_both_axes_when_they_disagree() {
        let cleaning = Seg {
            availability: "migrating".to_string(),
            status: "cleaning".to_string(),
            ..Seg::default()
        };
        assert_eq!(cleaning.status_label(), "移行中 / クリーニング中");

        let stopped = Seg {
            availability: "unavailable".to_string(),
            status: "down".to_string(),
            ..Seg::default()
        };
        // どちらも「停止」なので重ねて出さない。
        assert_eq!(stopped.status_label(), "停止");
    }

    /// IP は自ゲートウェイ側を優先し、無ければユーザー側で代替する。
    #[test]
    fn ip_label_falls_back_to_the_user_address() {
        let global = Seg {
            ip_addresses: vec!["203.0.113.100".to_string()],
            user_ip_addresses: vec!["192.0.2.1".to_string()],
            ..Seg::default()
        };
        assert_eq!(global.ip_label(), "203.0.113.100");

        let user_only = Seg {
            user_ip_addresses: vec!["192.0.2.1".to_string()],
            ..Seg::default()
        };
        assert_eq!(user_only.ip_label(), "192.0.2.1");
    }

    /// 空レスポンスでも空一覧を返すこと。
    #[test]
    fn empty_response_yields_an_empty_list() {
        let (items, total) = parse_segs("{}").unwrap();
        assert!(items.is_empty());
        assert_eq!(total, 0);
    }
}
