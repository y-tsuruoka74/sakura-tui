//! パケットフィルタ。
//!
//! ルールはフィルタ本体の配列として持つので、1本足すだけでも配列ごと送り直す。
//! その際、読んだときの `ExpressionHash` を一緒に送ると、間に他の人が変更して
//! いた場合に API 側が弾いてくれる。上書きで消してしまわないよう必ず送る。

use anyhow::{Context, Result};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{ResourceId, SacloudClient, null_as_default};

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;

/// ルールで選べるプロトコル。
pub const PROTOCOLS: [&str; 7] = ["tcp", "udp", "icmp", "fragment", "ip", "http", "https"];
/// ポートを指定できるプロトコル。
const PORTED_PROTOCOLS: [&str; 2] = ["tcp", "udp"];
/// ルールの動作。
pub const ACTIONS: [&str; 2] = ["allow", "deny"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketFilter {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    /// 更新のときにそのまま送り返す。読んだ内容が今も最新かの目印。
    pub expression_hash: String,
    pub rules: Vec<PacketFilterRule>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PacketFilterRule {
    pub protocol: String,
    /// 送信元の IP か CIDR。空なら全部。
    pub source_network: String,
    /// 送信元ポート。空なら全部。`80` や `80-89` の形。
    pub source_port: String,
    pub destination_port: String,
    /// `allow` か `deny`。
    pub action: String,
    pub description: String,
}

impl PacketFilterRule {
    pub fn is_allow(&self) -> bool {
        // 空のときは API 側の既定（拒否）に合わせる。
        self.action == "allow"
    }

    /// このプロトコルでポートを指定できるか。
    pub fn takes_port(protocol: &str) -> bool {
        PORTED_PROTOCOLS.contains(&protocol)
    }

    /// 一覧に出す「どこから」の表記。
    pub fn source(&self) -> String {
        let network = if self.source_network.is_empty() {
            "すべて"
        } else {
            &self.source_network
        };
        if self.source_port.is_empty() {
            network.to_string()
        } else {
            format!("{network}:{}", self.source_port)
        }
    }

    /// 一覧に出す「どこへ」の表記。
    pub fn destination(&self) -> String {
        if self.destination_port.is_empty() {
            "すべて".to_string()
        } else {
            self.destination_port.clone()
        }
    }

    /// API に送る形。ポートを取らないプロトコルでは port を落とす。
    fn to_json(&self) -> serde_json::Value {
        let mut value = json!({
            "Protocol": self.protocol,
            "SourceNetwork": self.source_network,
            "Action": self.action,
            "Description": self.description,
        });
        if Self::takes_port(&self.protocol) {
            value["SourcePort"] = json!(self.source_port);
            value["DestinationPort"] = json!(self.destination_port);
        }
        value
    }
}

impl SacloudClient {
    pub async fn list_packet_filters(&self, zone: &str) -> Result<Vec<PacketFilter>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({ "From": from, "Count": PAGE_SIZE, "Sort": ["Name"] });
            let value: serde_json::Value = self
                .request_in_zone(zone, Method::GET, "packetfilter", Some(body))
                .await?;
            let (items, total) = parse_page(&value)?;
            let received = items.len();
            out.extend(items);
            if received == 0 || from + received >= total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    /// 1件だけ読む。更新の直前に呼んで、最新の `ExpressionHash` を取る。
    pub async fn get_packet_filter(&self, zone: &str, id: ResourceId) -> Result<PacketFilter> {
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::GET, &format!("packetfilter/{id}"), None)
            .await?;
        let naked: NakedPacketFilter = value
            .get("PacketFilter")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .context("パケットフィルタの解析に失敗しました")?
            .context("パケットフィルタが応答に含まれていませんでした")?;
        Ok(naked.into())
    }

    pub async fn create_packet_filter(
        &self,
        zone: &str,
        name: &str,
        description: &str,
    ) -> Result<ResourceId> {
        let body = json!({
            "PacketFilter": { "Name": name, "Description": description }
        });
        let value: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "packetfilter", Some(body))
            .await?;
        value
            .pointer("/PacketFilter/ID")
            .and_then(|v| serde_json::from_value::<ResourceId>(v.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("パケットフィルタの作成応答にIDがありませんでした"))
    }

    /// 名前・説明・ルールをまとめて書き換える。
    ///
    /// ルールは配列ごと差し替わるので、呼び出し側は必ず全件を渡す。
    pub async fn update_packet_filter(
        &self,
        zone: &str,
        id: ResourceId,
        name: &str,
        description: &str,
        rules: &[PacketFilterRule],
        original_hash: &str,
    ) -> Result<()> {
        let expression: Vec<serde_json::Value> =
            rules.iter().map(PacketFilterRule::to_json).collect();
        let body = json!({
            "PacketFilter": {
                "Name": name,
                "Description": description,
                "Expression": expression,
            },
            "OriginalExpressionHash": original_hash,
        });
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::PUT, &format!("packetfilter/{id}"), Some(body))
            .await?;
        Ok(())
    }

    pub async fn delete_packet_filter(&self, zone: &str, id: ResourceId) -> Result<()> {
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &format!("packetfilter/{id}"), None)
            .await?;
        Ok(())
    }
}

fn parse_page(value: &serde_json::Value) -> Result<(Vec<PacketFilter>, usize)> {
    let items: Vec<NakedPacketFilter> = value
        .get("PacketFilters")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .context("パケットフィルタ一覧の解析に失敗しました")?
        .unwrap_or_default();
    let total = value
        .get("Total")
        .and_then(|v| {
            v.as_u64()
                .map(|n| n as usize)
                .or_else(|| v.as_str()?.parse().ok())
        })
        .unwrap_or(items.len());
    Ok((items.into_iter().map(Into::into).collect(), total))
}

#[derive(Debug, Deserialize)]
struct NakedPacketFilter {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(
        rename = "ExpressionHash",
        default,
        deserialize_with = "null_as_default"
    )]
    expression_hash: String,
    #[serde(rename = "Expression", default, deserialize_with = "null_as_default")]
    expression: Vec<NakedExpression>,
    #[serde(rename = "CreatedAt", default, deserialize_with = "null_as_default")]
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct NakedExpression {
    #[serde(rename = "Protocol", default, deserialize_with = "null_as_default")]
    protocol: String,
    #[serde(
        rename = "SourceNetwork",
        default,
        deserialize_with = "null_as_default"
    )]
    source_network: String,
    #[serde(rename = "SourcePort", default, deserialize_with = "null_as_default")]
    source_port: String,
    #[serde(
        rename = "DestinationPort",
        default,
        deserialize_with = "null_as_default"
    )]
    destination_port: String,
    #[serde(rename = "Action", default, deserialize_with = "null_as_default")]
    action: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
}

impl From<NakedPacketFilter> for PacketFilter {
    fn from(naked: NakedPacketFilter) -> Self {
        Self {
            id: naked.id,
            name: naked.name,
            description: naked.description,
            expression_hash: naked.expression_hash,
            rules: naked
                .expression
                .into_iter()
                .map(|e| PacketFilterRule {
                    protocol: e.protocol,
                    source_network: e.source_network,
                    source_port: e.source_port,
                    destination_port: e.destination_port,
                    action: e.action,
                    description: e.description,
                })
                .collect(),
            created_at: (!naked.created_at.is_empty()).then_some(naked.created_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filters_with_their_rules() {
        let value = json!({
            "Total": "1",
            "PacketFilters": [{
                "ID": "1", "Name": "web", "Description": "外向け",
                "ExpressionHash": "abc123",
                "Expression": [
                    {"Protocol": "tcp", "SourceNetwork": "203.0.113.0/24",
                     "SourcePort": "", "DestinationPort": "443",
                     "Action": "allow", "Description": "HTTPS"},
                    {"Protocol": "ip", "SourceNetwork": "", "Action": "deny"}
                ],
                "CreatedAt": "2026-01-01T00:00:00+09:00"
            }]
        });
        let (filters, total) = parse_page(&value).unwrap();
        assert_eq!(total, 1);
        let filter = &filters[0];
        assert_eq!(filter.expression_hash, "abc123");
        assert_eq!(filter.rules.len(), 2);
        assert_eq!(filter.rules[0].source(), "203.0.113.0/24");
        assert_eq!(filter.rules[0].destination(), "443");
        assert!(filter.rules[0].is_allow());
        // 空欄は「すべて」として読ませる。
        assert_eq!(filter.rules[1].source(), "すべて");
        assert_eq!(filter.rules[1].destination(), "すべて");
        assert!(!filter.rules[1].is_allow());
    }

    /// ポートを取らないプロトコルでは、ポートを送らないこと。
    /// 送ると API に弾かれる。
    #[test]
    fn ports_are_dropped_for_protocols_that_have_none() {
        let tcp = PacketFilterRule {
            protocol: "tcp".to_string(),
            destination_port: "22".to_string(),
            action: "allow".to_string(),
            ..PacketFilterRule::default()
        };
        assert_eq!(tcp.to_json()["DestinationPort"], json!("22"));

        let icmp = PacketFilterRule {
            protocol: "icmp".to_string(),
            // 前にTCPで入れた値が残っていても送らない。
            destination_port: "22".to_string(),
            action: "allow".to_string(),
            ..PacketFilterRule::default()
        };
        assert!(icmp.to_json().get("DestinationPort").is_none());
        assert!(!PacketFilterRule::takes_port("icmp"));
        assert!(PacketFilterRule::takes_port("udp"));
    }

    /// 送信元はネットワークとポートを1つにまとめて出す。
    #[test]
    fn the_source_shows_the_network_and_port_together() {
        let rule = PacketFilterRule {
            source_network: "192.0.2.1".to_string(),
            source_port: "1024-65535".to_string(),
            ..PacketFilterRule::default()
        };
        assert_eq!(rule.source(), "192.0.2.1:1024-65535");
    }
}
