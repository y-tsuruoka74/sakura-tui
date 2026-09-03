//! `commonserviceitem` を共用する DNS とシンプル監視。
//!
//! コンテナレジストリと同じエンドポイントで、`Provider.Class` だけが違う。
//! そのため `SacloudClient` をそのまま使える。
//! DNSとシンプル監視の作成・更新・削除に対応する。

use anyhow::{Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
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
    /// 更新競合を検出するため、次回PUTで送り返すハッシュ。
    pub settings_hash: String,
    pub created_at: Option<String>,
}

/// DNS レコード 1 件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DnsRecord {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Type")]
    pub record_type: String,
    #[serde(rename = "RData")]
    pub data: String,
    #[serde(rename = "TTL")]
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
    pub notify_slack_url: String,
    /// 更新時に未知の設定を維持するための `Settings.SimpleMonitor` 全体。
    pub raw_settings: serde_json::Value,
    pub settings_hash: String,
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

/// シンプル監視の作成・更新で変更する基本設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleMonitorInput {
    pub target: String,
    pub description: String,
    pub protocol: String,
    pub port: Option<u16>,
    pub path: String,
    pub expected_status: Option<u16>,
    pub delay_loop: u32,
    pub timeout: u32,
    pub enabled: bool,
    pub notify_email: bool,
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
    #[serde(rename = "SettingsHash", default, deserialize_with = "null_as_default")]
    settings_hash: String,
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
    settings: Option<serde_json::Value>,
    #[serde(rename = "Status")]
    status: Option<NakedMonitorStatus>,
    #[serde(rename = "SettingsHash", default, deserialize_with = "null_as_default")]
    settings_hash: String,
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
    #[serde(
        rename = "IncomingWebhooksURL",
        default,
        deserialize_with = "null_as_default"
    )]
    incoming_webhooks_url: String,
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
            settings_hash: naked.settings_hash,
            created_at: naked.created_at,
        }
    }
}

impl From<NakedSimpleMonitor> for SimpleMonitor {
    fn from(naked: NakedSimpleMonitor) -> Self {
        let raw_settings = naked
            .settings
            .as_ref()
            .and_then(|s| s.get("SimpleMonitor"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        let setting: Option<NakedMonitorSetting> =
            serde_json::from_value(raw_settings.clone()).ok();
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
            notify_slack_url: setting
                .as_ref()
                .and_then(|s| s.notify_slack.as_ref())
                .map(|n| n.incoming_webhooks_url.clone())
                .unwrap_or_default(),
            raw_settings,
            settings_hash: naked.settings_hash,
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

/// 自動バックアップの `Provider.Class`。
const AUTO_BACKUP_CLASS: &str = "autobackup";

/// 曜日の指定に使える値。API はこの綴りしか受け付けない。
pub const BACKUP_WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

/// 自動バックアップの設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBackupInput {
    pub name: String,
    pub description: String,
    /// 対象のディスク。作成後は変えられない。
    pub disk_id: ResourceId,
    /// 取得する曜日。空にはできない。
    pub weekdays: Vec<String>,
    /// 残す世代数。
    pub generations: u32,
}

fn auto_backup_settings(input: &AutoBackupInput) -> serde_json::Value {
    // 公式SDKは種別を送らない（曜日指定しか無いため）。合わせておく。
    json!({
        "BackupSpanWeekdays": input.weekdays,
        "MaximumNumberOfArchives": input.generations,
    })
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

    /// DNSゾーンを作成する。ゾーン名は作成後に変更できない。
    pub async fn create_dns_zone(&self, name: &str, description: &str) -> Result<()> {
        let body = json!({
            "CommonServiceItem": {
                "Name": name,
                "Description": description,
                "Status": { "Zone": name },
                "Settings": { "DNS": { "ResourceRecordSets": [] } },
                "Provider": { "Class": DNS_CLASS },
                "Tags": [],
                "Icon": {},
            }
        });
        let _: serde_json::Value = self
            .request_common(Method::POST, "commonserviceitem", Some(body))
            .await?;
        Ok(())
    }

    /// DNSゾーンの説明を更新する。レコード集合も同時に送り、設定を維持する。
    pub async fn update_dns_zone(&self, zone: &DnsZone, description: &str) -> Result<()> {
        let mut body = dns_update_body(&zone.records, &zone.settings_hash);
        body["CommonServiceItem"]["Name"] = json!(zone.name);
        body["CommonServiceItem"]["Description"] = json!(description);
        let path = format!("commonserviceitem/{}", zone.id);
        let _: serde_json::Value = self.request_common(Method::PUT, &path, Some(body)).await?;
        Ok(())
    }

    /// DNSゾーンと、その全レコードを削除する。
    pub async fn delete_dns_zone(&self, id: ResourceId) -> Result<()> {
        let path = format!("commonserviceitem/{id}");
        let _: serde_json::Value = self.request_common(Method::DELETE, &path, None).await?;
        Ok(())
    }

    /// DNSゾーン内のレコード集合を更新する。
    ///
    /// APIはレコード単位の更新ではなく全件置換なので、呼び出し側で追加・編集・削除後の
    /// 完全な一覧を渡す。`OriginalSettingsHash` により同時更新は409で拒否される。
    pub async fn update_dns_records(
        &self,
        id: ResourceId,
        records: &[DnsRecord],
        original_settings_hash: &str,
    ) -> Result<()> {
        let body = dns_update_body(records, original_settings_hash);
        let path = format!("commonserviceitem/{id}");
        let _: serde_json::Value = self.request_common(Method::PUT, &path, Some(body)).await?;
        Ok(())
    }

    /// 自動バックアップを作る。
    ///
    /// 対象ディスクのあるゾーンで呼ぶ。`commonserviceitem` は DNS などと
    /// 同じ入れ物だが、こちらは対象ディスクがゾーンに属する。
    pub async fn create_auto_backup(
        &self,
        zone: &str,
        input: &AutoBackupInput,
    ) -> Result<ResourceId> {
        let body = json!({
            "CommonServiceItem": {
                "Name": input.name,
                "Description": input.description,
                // API のキーは DiskId（小文字の d）。DiskID だと 400 になる。
                "Status": { "DiskId": input.disk_id.0 },
                "Settings": { "Autobackup": auto_backup_settings(input) },
                "Provider": { "Class": AUTO_BACKUP_CLASS },
            }
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "commonserviceitem", Some(body))
            .await?;
        value
            .pointer("/CommonServiceItem/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("自動バックアップの作成応答にIDがありませんでした"))
    }

    /// 曜日と世代数を変える。対象ディスクは変えられない。
    pub async fn update_auto_backup(
        &self,
        zone: &str,
        id: ResourceId,
        input: &AutoBackupInput,
    ) -> Result<()> {
        let body = json!({
            "CommonServiceItem": {
                "Name": input.name,
                "Description": input.description,
                "Settings": { "Autobackup": auto_backup_settings(input) },
            }
        });
        let _: serde_json::Value = self
            .request_in_zone(
                zone,
                Method::PUT,
                &format!("commonserviceitem/{id}"),
                Some(body),
            )
            .await?;
        Ok(())
    }

    pub async fn delete_auto_backup(&self, zone: &str, id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(
                zone,
                Method::DELETE,
                &format!("commonserviceitem/{id}"),
                None,
            )
            .await?;
        Ok(())
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

    pub async fn create_simple_monitor(&self, input: &SimpleMonitorInput) -> Result<()> {
        let settings = simple_monitor_settings(input, None);
        let body = json!({
            "CommonServiceItem": {
                "Name": input.target,
                "Description": input.description,
                "Status": { "Target": input.target },
                "Settings": { "SimpleMonitor": settings },
                "Provider": { "Class": SIMPLE_MONITOR_CLASS },
                "Tags": [],
                "Icon": {},
            }
        });
        let _: serde_json::Value = self
            .request_common(Method::POST, "commonserviceitem", Some(body))
            .await?;
        Ok(())
    }

    pub async fn update_simple_monitor(
        &self,
        monitor: &SimpleMonitor,
        input: &SimpleMonitorInput,
    ) -> Result<()> {
        let settings = simple_monitor_settings(input, Some(monitor.raw_settings.clone()));
        let mut body = json!({
            "CommonServiceItem": {
                "Name": monitor.target,
                "Description": input.description,
                "Settings": { "SimpleMonitor": settings },
            }
        });
        if !monitor.settings_hash.is_empty() {
            body["OriginalSettingsHash"] = json!(monitor.settings_hash);
        }
        let path = format!("commonserviceitem/{}", monitor.id);
        let _: serde_json::Value = self.request_common(Method::PUT, &path, Some(body)).await?;
        Ok(())
    }

    /// 有効状態だけを変更し、その他の既知・未知設定をそのまま維持する。
    pub async fn set_simple_monitor_enabled(
        &self,
        monitor: &SimpleMonitor,
        enabled: bool,
    ) -> Result<()> {
        let mut settings = monitor.raw_settings.clone();
        if !settings.is_object() {
            settings = json!({});
        }
        settings["Enabled"] = json!(if enabled { "True" } else { "False" });
        let mut body = json!({
            "CommonServiceItem": {
                "Settings": { "SimpleMonitor": settings },
            }
        });
        if !monitor.settings_hash.is_empty() {
            body["OriginalSettingsHash"] = json!(monitor.settings_hash);
        }
        let path = format!("commonserviceitem/{}", monitor.id);
        let _: serde_json::Value = self.request_common(Method::PUT, &path, Some(body)).await?;
        Ok(())
    }

    pub async fn delete_simple_monitor(&self, id: ResourceId) -> Result<()> {
        let path = format!("commonserviceitem/{id}");
        let _: serde_json::Value = self.request_common(Method::DELETE, &path, None).await?;
        Ok(())
    }
}

fn simple_monitor_settings(
    input: &SimpleMonitorInput,
    base: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut settings = base
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| json!({}));
    settings["DelayLoop"] = json!(input.delay_loop);
    settings["Timeout"] = json!(input.timeout);
    settings["Enabled"] = json!(if input.enabled { "True" } else { "False" });
    settings["NotifyEmail"]["Enabled"] = json!(if input.notify_email { "True" } else { "False" });

    let existing_health = settings
        .get("HealthCheck")
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    // 同じ方式の編集なら未知の方式固有設定も維持する。方式を変えた場合は、旧方式の
    // パラメータ（SNIなど）が新方式で不正になりうるため引き継がない。
    let mut health = if existing_health
        .get("Protocol")
        .and_then(serde_json::Value::as_str)
        == Some(input.protocol.as_str())
    {
        existing_health
    } else {
        json!({})
    };
    health["Protocol"] = json!(input.protocol);
    for key in ["Port", "Path", "Status"] {
        if let Some(object) = health.as_object_mut() {
            object.remove(key);
        }
    }
    if let Some(port) = input.port {
        health["Port"] = json!(port.to_string());
    }
    if matches!(input.protocol.as_str(), "http" | "https") {
        health["Path"] = json!(input.path);
        if let Some(status) = input.expected_status {
            health["Status"] = json!(status.to_string());
        }
    }
    settings["HealthCheck"] = health;
    settings
}

fn dns_update_body(records: &[DnsRecord], original_settings_hash: &str) -> serde_json::Value {
    let mut body = json!({
        "CommonServiceItem": {
            "Settings": {
                "DNS": {
                    "ResourceRecordSets": records,
                }
            }
        }
    });
    if !original_settings_hash.is_empty() {
        body["OriginalSettingsHash"] = json!(original_settings_hash);
    }
    body
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
            "SettingsHash": "hash-1",
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
        assert_eq!(zone.settings_hash, "hash-1");
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

    #[test]
    fn builds_dns_update_with_settings_hash() {
        let records = vec![DnsRecord {
            name: "www".to_string(),
            record_type: "A".to_string(),
            data: "192.0.2.1".to_string(),
            ttl: 300,
        }];
        let body = dns_update_body(&records, "hash-1");
        assert_eq!(body["OriginalSettingsHash"], "hash-1");
        assert_eq!(
            body["CommonServiceItem"]["Settings"]["DNS"]["ResourceRecordSets"][0],
            json!({"Name":"www", "Type":"A", "RData":"192.0.2.1", "TTL":300})
        );
    }

    #[test]
    fn simple_monitor_update_preserves_unknown_settings() {
        let input = SimpleMonitorInput {
            target: "example.jp".to_string(),
            description: String::new(),
            protocol: "https".to_string(),
            port: Some(443),
            path: "/health".to_string(),
            expected_status: Some(204),
            delay_loop: 120,
            timeout: 10,
            enabled: true,
            notify_email: false,
        };
        let base = json!({
            "RetryCount": 3,
            "NotifySlack": {"Enabled":"True", "IncomingWebhooksURL":"https://example.invalid"},
            "HealthCheck": {"Protocol":"https", "Port":"80", "SNI": true}
        });
        let settings = simple_monitor_settings(&input, Some(base));
        assert_eq!(settings["RetryCount"], 3);
        assert_eq!(
            settings["NotifySlack"]["IncomingWebhooksURL"],
            "https://example.invalid"
        );
        assert_eq!(settings["HealthCheck"]["SNI"], true);
        assert_eq!(settings["HealthCheck"]["Protocol"], "https");
        assert_eq!(settings["HealthCheck"]["Port"], "443");
        assert_eq!(settings["HealthCheck"]["Path"], "/health");
        assert_eq!(settings["HealthCheck"]["Status"], "204");
        assert_eq!(settings["NotifyEmail"]["Enabled"], "False");
    }
}
