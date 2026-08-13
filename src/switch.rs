//! さくらのクラウドのスイッチ。
//!
//! スイッチはゾーンに属するため、サーバーと同様に選択中ゾーンの
//! API エンドポイントから取得する。

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{ResourceId, SacloudClient, flexible_number, null_as_default};

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;

/// スイッチ 1 件。
#[derive(Debug, Clone)]
pub struct Switch {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub server_count: usize,
    pub appliance_count: usize,
    pub zone: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SwitchFindResponse {
    #[serde(rename = "Switches", default, deserialize_with = "null_as_default")]
    switches: Vec<NakedSwitch>,
    #[serde(rename = "Total", default)]
    total: usize,
}

#[derive(Debug, Deserialize)]
struct NakedSwitch {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
    #[serde(rename = "ServerCount", default, deserialize_with = "flexible_number")]
    server_count: usize,
    #[serde(
        rename = "ApplianceCount",
        default,
        deserialize_with = "flexible_number"
    )]
    appliance_count: usize,
    #[serde(rename = "Zone")]
    zone: Option<NakedZone>,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NakedZone {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
}

impl From<NakedSwitch> for Switch {
    fn from(naked: NakedSwitch) -> Self {
        Switch {
            id: naked.id,
            name: naked.name,
            description: naked.description,
            tags: naked.tags,
            server_count: naked.server_count,
            appliance_count: naked.appliance_count,
            zone: naked.zone.map(|z| z.name).unwrap_or_default(),
            created_at: naked.created_at,
        }
    }
}

impl SacloudClient {
    /// 指定ゾーンのスイッチを全件取得する。
    pub async fn list_switches(&self, zone: &str) -> Result<Vec<Switch>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let res: SwitchFindResponse = self
                .request_in_zone(zone, Method::GET, "switch", Some(body))
                .await?;
            let received = res.switches.len();
            out.extend(res.switches.into_iter().map(Switch::from));
            if received == 0 || out.len() >= res.total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    /// 指定ゾーンのスイッチ件数だけを取得する。
    pub async fn count_switches(&self, zone: &str) -> Result<usize> {
        let body = json!({ "From": 0, "Count": 1 });
        let res: SwitchFindResponse = self
            .request_in_zone(zone, Method::GET, "switch", Some(body))
            .await?;
        Ok(res.total)
    }

    /// 指定ゾーンにスイッチを作成する。
    pub async fn create_switch(&self, zone: &str, name: &str, description: &str) -> Result<()> {
        let body = json!({
            "Switch": {
                "Name": name,
                "Description": description,
            }
        });
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::POST, "switch", Some(body))
            .await?;
        Ok(())
    }

    /// スイッチの名前と説明を更新する。
    pub async fn update_switch(
        &self,
        zone: &str,
        id: ResourceId,
        name: &str,
        description: &str,
    ) -> Result<()> {
        let body = json!({
            "Switch": {
                "Name": name,
                "Description": description,
            }
        });
        let path = format!("switch/{id}");
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::PUT, &path, Some(body))
            .await?;
        Ok(())
    }

    /// 接続リソースの無いスイッチを削除する。
    pub async fn delete_switch(&self, zone: &str, id: ResourceId) -> Result<()> {
        let path = format!("switch/{id}");
        let _: serde_json::Value = self
            .request_in_zone(zone, Method::DELETE, &path, None)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_switch_list() {
        let body = r#"{
            "Total": 1,
            "Switches": [{
                "ID": "113000000001",
                "Name": "private-network",
                "Description": null,
                "Tags": ["production", "internal"],
                "ServerCount": 3,
                "ApplianceCount": 1,
                "Zone": {"Name": "is1a"},
                "CreatedAt": "2026-01-02T03:04:05+09:00"
            }]
        }"#;
        let response: SwitchFindResponse = serde_json::from_str(body).unwrap();
        let switch = Switch::from(response.switches.into_iter().next().unwrap());
        assert_eq!(switch.id, ResourceId(113000000001));
        assert_eq!(switch.name, "private-network");
        assert!(switch.description.is_empty());
        assert_eq!(switch.tags, vec!["production", "internal"]);
        assert_eq!(switch.server_count, 3);
        assert_eq!(switch.appliance_count, 1);
        assert_eq!(switch.zone, "is1a");
    }

    #[test]
    fn reads_total_without_loading_every_switch() {
        let body = r#"{
            "Total": 42,
            "Switches": [{"ID": 1, "Name": "first"}]
        }"#;
        let response: SwitchFindResponse = serde_json::from_str(body).unwrap();
        assert_eq!(response.total, 42);
        assert_eq!(response.switches.len(), 1);
    }

    #[test]
    fn accepts_empty_switch_list() {
        let response: SwitchFindResponse =
            serde_json::from_str(r#"{"Total":0,"Switches":null}"#).unwrap();
        assert_eq!(response.total, 0);
        assert!(response.switches.is_empty());
    }

    #[test]
    fn accepts_string_and_null_counts() {
        let response: SwitchFindResponse = serde_json::from_str(
            r#"{"Total":1,"Switches":[{
                "ID":"1","Name":"switch","ServerCount":"2","ApplianceCount":null
            }]}"#,
        )
        .unwrap();
        let switch = Switch::from(response.switches.into_iter().next().unwrap());
        assert_eq!(switch.server_count, 2);
        assert_eq!(switch.appliance_count, 0);
    }
}
