//! `commonserviceitem` を共用する DNS とシンプル監視（閲覧のみ）。
//!
//! コンテナレジストリと同じエンドポイントで、`Provider.Class` だけが違う。
//! そのため `SacloudClient` をそのまま使える。

use anyhow::{Context, Result};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{ResourceId, SacloudClient, null_as_default};

const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が実態と違う総件数を返しても止まるようにする。
const MAX_PAGES: usize = 100;
const DNS_CLASS: &str = "dns";
const SIMPLE_MONITOR_CLASS: &str = "simplemon";

/// DNS ゾーン 1 件。
#[derive(Debug, Clone)]
pub struct DnsZone {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// 委任先のネームサーバー。
    pub name_servers: Vec<String>,
    pub records: Vec<DnsRecord>,
    pub created_at: Option<String>,
}

/// DNS レコード 1 件。
#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub record_type: String,
    pub data: String,
    pub ttl: u32,
}

impl DnsRecord {
    /// ゾーン名を足した FQDN。`@` はゾーン頂点を指す。
    pub fn fqdn(&self, zone: &str) -> String {
        if self.name == "@" || self.name.is_empty() {
            zone.to_string()
        } else {
            format!("{}.{}", self.name, zone)
        }
    }
}

/// シンプル監視 1 件。
#[derive(Debug, Clone)]
pub struct SimpleMonitor {
    pub id: ResourceId,
    /// 監視対象（ホスト名 or IP）。
    pub target: String,
    pub description: String,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub protocol: String,
    pub port: String,
    pub path: String,
    /// HTTP/HTTPS 監視で期待するステータスコード。
    pub expected_status: String,
    /// 監視間隔（秒）。
    pub delay_loop: u32,
    pub timeout: u32,
    pub notify_email: bool,
    pub notify_slack: bool,
    pub created_at: Option<String>,
}

impl SimpleMonitor {
    /// 「https://example.jp:443/health」のような要約。
    pub fn summary(&self) -> String {
        let mut out = self.protocol.clone();
        if !out.is_empty() {
            out.push_str("://");
        }
        out.push_str(&self.target);
        if !self.port.is_empty() {
            out.push(':');
            out.push_str(&self.port);
        }
        if !self.path.is_empty() {
            out.push_str(&self.path);
        }
        out
    }
}

// --- API のレスポンス形状 ---

#[derive(Debug, Deserialize)]
struct FindResponse {
    #[serde(
        rename = "CommonServiceItems",
        default,
        deserialize_with = "null_as_default"
    )]
    items: Vec<serde_json::Value>,
    #[serde(rename = "Total", default)]
    total: usize,
}

#[derive(Debug, Deserialize)]
struct NakedDns {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "Settings")]
    settings: Option<NakedDnsSettings>,
    #[serde(rename = "Status")]
    status: Option<NakedDnsStatus>,
}

#[derive(Debug, Deserialize)]
struct NakedDnsSettings {
    #[serde(rename = "DNS")]
    dns: Option<NakedDnsSetting>,
}

#[derive(Debug, Deserialize)]
struct NakedDnsSetting {
    #[serde(
        rename = "ResourceRecordSets",
        default,
        deserialize_with = "null_as_default"
    )]
    records: Vec<NakedDnsRecord>,
}

#[derive(Debug, Deserialize)]
struct NakedDnsRecord {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Type", default, deserialize_with = "null_as_default")]
    record_type: String,
    #[serde(rename = "RData", default, deserialize_with = "null_as_default")]
    data: String,
    #[serde(rename = "TTL", default)]
    ttl: u32,
}

#[derive(Debug, Deserialize)]
struct NakedDnsStatus {
    #[serde(rename = "NS", default, deserialize_with = "null_as_default")]
    name_servers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NakedSimpleMonitor {
    #[serde(rename = "ID")]
    id: ResourceId,
    /// シンプル監視は Name に監視対象が入る。
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "Settings")]
    settings: Option<NakedMonitorSettings>,
    #[serde(rename = "Status")]
    status: Option<NakedMonitorStatus>,
}

#[derive(Debug, Deserialize)]
struct NakedMonitorSettings {
    #[serde(rename = "SimpleMonitor")]
    simple_monitor: Option<NakedMonitorSetting>,
}

#[derive(Debug, Deserialize)]
struct NakedMonitorSetting {
    #[serde(rename = "DelayLoop", default)]
    delay_loop: u32,
    #[serde(rename = "Timeout", default)]
    timeout: u32,
    /// API は文字列の "True"/"False" で返す。
    #[serde(rename = "Enabled", default, deserialize_with = "null_as_default")]
    enabled: StringFlag,
    #[serde(rename = "HealthCheck")]
    health_check: Option<NakedHealthCheck>,
    #[serde(rename = "NotifyEmail")]
    notify_email: Option<NakedNotify>,
    #[serde(rename = "NotifySlack")]
    notify_slack: Option<NakedNotify>,
}

#[derive(Debug, Deserialize)]
struct NakedNotify {
    #[serde(rename = "Enabled", default, deserialize_with = "null_as_default")]
    enabled: StringFlag,
}

#[derive(Debug, Deserialize)]
struct NakedHealthCheck {
    #[serde(rename = "Protocol", default, deserialize_with = "null_as_default")]
    protocol: String,
    #[serde(rename = "Port", default, deserialize_with = "null_as_default")]
    port: StringNumber,
    #[serde(rename = "Path", default, deserialize_with = "null_as_default")]
    path: String,
    #[serde(rename = "Status", default, deserialize_with = "null_as_default")]
    status: StringNumber,
    #[serde(rename = "Host", default, deserialize_with = "null_as_default")]
    host: String,
}

#[derive(Debug, Deserialize)]
struct NakedMonitorStatus {
    #[serde(rename = "Target", default, deserialize_with = "null_as_default")]
    target: String,
}

/// `"True"` / `"False"` / 真偽値のいずれでも受け取るフラグ。
#[derive(Debug, Default, Clone, Copy)]
struct StringFlag(bool);

impl<'de> Deserialize<'de> for StringFlag {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(match serde_json::Value::deserialize(de)? {
            serde_json::Value::Bool(b) => StringFlag(b),
            serde_json::Value::String(s) => StringFlag(s.eq_ignore_ascii_case("true")),
            _ => StringFlag(false),
        })
    }
}

/// 数値でも文字列でも返ってくる項目（ポート番号など）。
#[derive(Debug, Default, Clone)]
struct StringNumber(String);

impl<'de> Deserialize<'de> for StringNumber {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(match serde_json::Value::deserialize(de)? {
            serde_json::Value::String(s) => StringNumber(s),
            serde_json::Value::Number(n) => StringNumber(n.to_string()),
            _ => StringNumber(String::new()),
        })
    }
}

impl From<NakedDns> for DnsZone {
    fn from(naked: NakedDns) -> Self {
        DnsZone {
            id: naked.id,
            name: naked.name,
            description: naked.description,
            tags: naked.tags,
            name_servers: naked.status.map(|s| s.name_servers).unwrap_or_default(),
            records: naked
                .settings
                .and_then(|s| s.dns)
                .map(|d| d.records)
                .unwrap_or_default()
                .into_iter()
                .map(|r| DnsRecord {
                    name: r.name,
                    record_type: r.record_type,
                    data: r.data,
                    ttl: r.ttl,
                })
                .collect(),
            created_at: naked.created_at,
        }
    }
}

impl From<NakedSimpleMonitor> for SimpleMonitor {
    fn from(naked: NakedSimpleMonitor) -> Self {
        let setting = naked.settings.and_then(|s| s.simple_monitor);
        let health = setting.as_ref().and_then(|s| s.health_check.as_ref());
        // Status.Target が空なら Name（監視対象が入っている）で代用する。
        let target = naked
            .status
            .map(|s| s.target)
            .filter(|t| !t.is_empty())
            .or_else(|| health.map(|h| h.host.clone()).filter(|h| !h.is_empty()))
            .unwrap_or(naked.name);

        SimpleMonitor {
            id: naked.id,
            target,
            description: naked.description,
            tags: naked.tags,
            enabled: setting.as_ref().is_some_and(|s| s.enabled.0),
            protocol: health.map(|h| h.protocol.clone()).unwrap_or_default(),
            port: health.map(|h| h.port.0.clone()).unwrap_or_default(),
            path: health.map(|h| h.path.clone()).unwrap_or_default(),
            expected_status: health.map(|h| h.status.0.clone()).unwrap_or_default(),
            delay_loop: setting.as_ref().map_or(0, |s| s.delay_loop),
            timeout: setting.as_ref().map_or(0, |s| s.timeout),
            notify_email: setting
                .as_ref()
                .and_then(|s| s.notify_email.as_ref())
                .is_some_and(|n| n.enabled.0),
            notify_slack: setting
                .as_ref()
                .and_then(|s| s.notify_slack.as_ref())
                .is_some_and(|n| n.enabled.0),
            created_at: naked.created_at,
        }
    }
}

/// `Provider.Class` が指定の種別か。`Settings` の形でも判定する。
fn has_class(item: &serde_json::Value, class: &str, settings_key: &str) -> bool {
    let found = item
        .get("Provider")
        .and_then(|p| p.get("Class"))
        .and_then(serde_json::Value::as_str);
    if let Some(found) = found {
        return found == class;
    }
    item.get("Settings")
        .and_then(|s| s.get(settings_key))
        .is_some_and(|v| !v.is_null())
}

impl SacloudClient {
    /// `commonserviceitem` から指定種別のものだけを全件集める。
    async fn find_common_items(
        &self,
        class: &str,
        settings_key: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        let mut fetched = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({
                "Filter": { "Provider.Class": class },
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let res: FindResponse = self
                .request_common(Method::GET, "commonserviceitem", Some(body))
                .await?;
            let received = res.items.len();
            out.extend(
                res.items
                    .into_iter()
                    .filter(|item| has_class(item, class, settings_key)),
            );
            fetched += received;
            if received == 0 || fetched >= res.total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    pub async fn list_dns_zones(&self) -> Result<Vec<DnsZone>> {
        let items = self.find_common_items(DNS_CLASS, "DNS").await?;
        items
            .into_iter()
            .map(|item| {
                let naked: NakedDns =
                    serde_json::from_value(item).context("DNSゾーンの解析に失敗しました")?;
                Ok(DnsZone::from(naked))
            })
            .collect()
    }

    pub async fn list_simple_monitors(&self) -> Result<Vec<SimpleMonitor>> {
        let items = self
            .find_common_items(SIMPLE_MONITOR_CLASS, "SimpleMonitor")
            .await?;
        items
            .into_iter()
            .map(|item| {
                let naked: NakedSimpleMonitor =
                    serde_json::from_value(item).context("シンプル監視の解析に失敗しました")?;
                Ok(SimpleMonitor::from(naked))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dns_zone_with_records() {
        let body = r#"{
            "ID": "113701924283",
            "Name": "example.jp",
            "Description": null,
            "Tags": [],
            "Settings": {"DNS": {"ResourceRecordSets": [
                {"Name": "app", "Type": "A", "RData": "203.0.113.10", "TTL": 300},
                {"Name": "@", "Type": "MX", "RData": "10 mail.example.jp.", "TTL": 3600}
            ]}},
            "Status": {"Zone": "example.jp", "NS": ["ns1.gslb1.sakura.ne.jp"]}
        }"#;
        let naked: NakedDns = serde_json::from_str(body).unwrap();
        let zone = DnsZone::from(naked);
        assert_eq!(zone.name, "example.jp");
        assert_eq!(zone.records.len(), 2);
        assert_eq!(zone.records[0].fqdn("example.jp"), "app.example.jp");
        // @ はゾーン頂点。
        assert_eq!(zone.records[1].fqdn("example.jp"), "example.jp");
        assert_eq!(zone.name_servers, vec!["ns1.gslb1.sakura.ne.jp"]);
    }

    /// レコードが 1 件も無いゾーンでも落ちないこと。
    #[test]
    fn parses_dns_zone_without_records() {
        let body = r#"{"ID": 1, "Name": "example.jp",
            "Settings": {"DNS": {"ResourceRecordSets": null}}, "Status": null}"#;
        let naked: NakedDns = serde_json::from_str(body).unwrap();
        let zone = DnsZone::from(naked);
        assert!(zone.records.is_empty());
        assert!(zone.name_servers.is_empty());
    }

    #[test]
    fn parses_simple_monitor() {
        // Enabled や Port は文字列で返ってくる。
        let body = r#"{
            "ID": "113000000001",
            "Name": "www.example.jp",
            "Description": "本番",
            "Tags": ["prod"],
            "Settings": {"SimpleMonitor": {
                "DelayLoop": 60, "Timeout": 10, "Enabled": "True",
                "HealthCheck": {"Protocol": "https", "Port": "443", "Path": "/health",
                                "Status": "200", "Host": "www.example.jp"},
                "NotifyEmail": {"Enabled": "True"},
                "NotifySlack": {"Enabled": "False"}
            }},
            "Status": {"Target": "www.example.jp"}
        }"#;
        let naked: NakedSimpleMonitor = serde_json::from_str(body).unwrap();
        let monitor = SimpleMonitor::from(naked);
        assert_eq!(monitor.target, "www.example.jp");
        assert!(monitor.enabled);
        assert_eq!(monitor.summary(), "https://www.example.jp:443/health");
        assert_eq!(monitor.expected_status, "200");
        assert_eq!(monitor.delay_loop, 60);
        assert!(monitor.notify_email);
        assert!(!monitor.notify_slack);
    }

    /// Port が数値で返ってきても受けられること。
    #[test]
    fn accepts_numeric_port() {
        let body = r#"{"ID": 1, "Name": "x",
            "Settings": {"SimpleMonitor": {"Enabled": true,
                "HealthCheck": {"Protocol": "tcp", "Port": 22}}}}"#;
        let naked: NakedSimpleMonitor = serde_json::from_str(body).unwrap();
        let monitor = SimpleMonitor::from(naked);
        assert_eq!(monitor.port, "22");
        assert!(monitor.enabled);
    }

    /// Status.Target が無ければ HealthCheck.Host、それも無ければ Name を使う。
    #[test]
    fn falls_back_for_target() {
        let body = r#"{"ID": 1, "Name": "fallback.example.jp",
            "Settings": {"SimpleMonitor": {"HealthCheck": {"Protocol": "ping"}}}}"#;
        let naked: NakedSimpleMonitor = serde_json::from_str(body).unwrap();
        assert_eq!(SimpleMonitor::from(naked).target, "fallback.example.jp");
    }

    #[test]
    fn class_detection_uses_settings_as_fallback() {
        let dns = serde_json::json!({"Settings": {"DNS": {"ResourceRecordSets": []}}});
        assert!(has_class(&dns, DNS_CLASS, "DNS"));
        assert!(!has_class(&dns, SIMPLE_MONITOR_CLASS, "SimpleMonitor"));

        let explicit = serde_json::json!({"Provider": {"Class": "simplemon"}});
        assert!(has_class(&explicit, SIMPLE_MONITOR_CLASS, "SimpleMonitor"));
    }
}
