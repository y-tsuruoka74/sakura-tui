//! IaaSの読み取り専用リソースブラウザ。

use anyhow::{Context, Result};
use reqwest::Method;
use serde_json::{Value, json};

use crate::sacloud::{ResourceId, SacloudClient};

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloudResourceKind {
    Disk,
    Internet,
    LoadBalancer,
    VpcRouter,
    Database,
    Nfs,
}

impl CloudResourceKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Disk => "ディスク",
            Self::Internet => "ルータ＋スイッチ",
            Self::LoadBalancer => "ロードバランサ",
            Self::VpcRouter => "VPCルータ",
            Self::Database => "データベース",
            Self::Nfs => "NFS",
        }
    }

    fn endpoint(self) -> &'static str {
        match self {
            Self::Disk => "disk",
            Self::Internet => "internet",
            _ => "appliance",
        }
    }

    fn appliance_class(self) -> Option<&'static str> {
        match self {
            Self::LoadBalancer => Some("loadbalancer"),
            Self::VpcRouter => Some("vpcrouter"),
            Self::Database => Some("database"),
            Self::Nfs => Some("nfs"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CloudResource {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub availability: String,
    pub class: String,
    pub status: String,
    pub plan: String,
    pub connection: String,
    pub created_at: String,
    pub details: Vec<(String, String)>,
}

impl CloudResource {
    pub fn searchable(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.name,
            self.description,
            self.tags.join(" "),
            self.status,
            self.plan
        )
    }
}

impl SacloudClient {
    pub async fn list_cloud_resources(
        &self,
        zone: &str,
        kind: CloudResourceKind,
    ) -> Result<Vec<CloudResource>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let mut body = json!({"From": from, "Count": PAGE_SIZE, "Sort": ["Name"]});
            if let Some(class) = kind.appliance_class() {
                body["Filter"] = json!({"Class": class});
            }
            let value: Value = self
                .request_in_zone(zone, Method::GET, kind.endpoint(), Some(body))
                .await?;
            let total = value.get("Total").and_then(value_usize).unwrap_or(0);
            let items = find_items(&value, kind.endpoint());
            let received = items.len();
            for item in items {
                if kind
                    .appliance_class()
                    .is_some_and(|class| string_at(item, "/Class") != class)
                {
                    continue;
                }
                out.push(parse_resource(item, kind)?);
            }
            if received == 0 || from + received >= total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    pub async fn count_cloud_resources(
        &self,
        zone: &str,
        kind: CloudResourceKind,
    ) -> Result<usize> {
        let mut body = json!({"From": 0, "Count": 1});
        if let Some(class) = kind.appliance_class() {
            body["Filter"] = json!({"Class": class});
        }
        let value: Value = self
            .request_in_zone(zone, Method::GET, kind.endpoint(), Some(body))
            .await?;
        Ok(value
            .get("Total")
            .and_then(value_usize)
            .unwrap_or_else(|| find_items(&value, kind.endpoint()).len()))
    }
}

fn find_items<'a>(value: &'a Value, endpoint: &str) -> Vec<&'a Value> {
    let preferred = match endpoint {
        "disk" => &["Disks", "Disk"][..],
        "internet" => &["Internet", "Internets"][..],
        _ => &["Appliances", "Appliance"][..],
    };
    preferred
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn parse_resource(value: &Value, kind: CloudResourceKind) -> Result<CloudResource> {
    let id_value = value.get("ID").context("リソースIDがありません")?.clone();
    let id: ResourceId = serde_json::from_value(id_value).context("リソースIDを解析できません")?;
    let tags: Vec<String> = value
        .get("Tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let status = first_non_empty(
        value,
        &["/Instance/Status", "/Status/Status", "/Availability"],
    );
    let plan = first_non_empty(value, &["/Plan/Name", "/Remark/Plan/Name", "/ServiceClass"]);
    let connection = match kind {
        CloudResourceKind::Disk => first_non_empty(value, &["/Server/Name", "/Connection"]),
        CloudResourceKind::Internet => first_non_empty(value, &["/Switch/Name", "/BandWidthMbps"]),
        _ => first_non_empty(value, &["/Switch/Name", "/Interfaces/0/Switch/Name"]),
    };
    let mut details = Vec::new();
    add_detail(&mut details, "ID", id.to_string());
    add_detail(&mut details, "種別", kind.title().to_string());
    add_detail(&mut details, "状態", status.clone());
    add_detail(&mut details, "プラン", plan.clone());
    add_detail(&mut details, "接続先", connection.clone());
    for (label, pointers) in detail_fields(kind) {
        add_detail(&mut details, label, first_non_empty(value, pointers));
    }
    add_detail(&mut details, "タグ", tags.join(", "));
    add_detail(&mut details, "作成日時", string_at(value, "/CreatedAt"));
    Ok(CloudResource {
        id,
        name: string_at(value, "/Name"),
        description: string_at(value, "/Description"),
        tags,
        availability: string_at(value, "/Availability"),
        class: string_at(value, "/Class"),
        status,
        plan,
        connection,
        created_at: string_at(value, "/CreatedAt"),
        details,
    })
}

fn detail_fields(kind: CloudResourceKind) -> &'static [(&'static str, &'static [&'static str])] {
    match kind {
        CloudResourceKind::Disk => &[
            ("サイズ", &["/SizeMB"]),
            ("接続方式", &["/Connection"]),
            ("サーバー", &["/Server/Name", "/Server/ID"]),
            ("暗号化", &["/EncryptionAlgorithm"]),
        ],
        CloudResourceKind::Internet => &[
            ("帯域(Mbps)", &["/BandWidthMbps"]),
            ("スイッチ", &["/Switch/Name", "/Switch/ID"]),
            ("ネットワーク", &["/Switch/Subnets/0/NetworkAddress"]),
            ("マスク長", &["/Switch/Subnets/0/NetworkMaskLen"]),
            ("ゲートウェイ", &["/Switch/Subnets/0/DefaultRoute"]),
        ],
        _ => &[
            ("サービスクラス", &["/ServiceClass"]),
            ("スイッチ", &["/Switch/Name", "/Switch/ID"]),
            (
                "IPアドレス",
                &["/Interfaces/0/IPAddress", "/Remark/Servers/0/IPAddress"],
            ),
            ("デフォルトルート", &["/Remark/Network/DefaultRoute"]),
            ("マスク長", &["/Remark/Network/NetworkMaskLen"]),
        ],
    }
}

fn add_detail(details: &mut Vec<(String, String)>, label: &str, value: String) {
    if !value.is_empty() {
        details.push((label.to_string(), value));
    }
}

fn first_non_empty(value: &Value, pointers: &[&str]) -> String {
    pointers
        .iter()
        .map(|p| string_at(value, p))
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

fn string_at(value: &Value, pointer: &str) -> String {
    value.pointer(pointer).map(value_string).unwrap_or_default()
}

fn value_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}

fn value_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_disk_and_appliance_details() {
        let disk = json!({"ID":"1","Name":"data","SizeMB":"102400","Plan":{"Name":"SSD"},"Server":{"Name":"web"},"Availability":"available"});
        let parsed = parse_resource(&disk, CloudResourceKind::Disk).unwrap();
        assert_eq!(parsed.id, ResourceId(1));
        assert_eq!(parsed.plan, "SSD");
        assert_eq!(parsed.connection, "web");
        let appliance = json!({"ID":2,"Name":"lb","Class":"loadbalancer","Instance":{"Status":"up"},"ServiceClass":"cloud/appliance/loadbalancer/1"});
        assert_eq!(
            parse_resource(&appliance, CloudResourceKind::LoadBalancer)
                .unwrap()
                .status,
            "up"
        );
    }

    #[test]
    fn finds_singular_internet_array() {
        let value = json!({"Total":"1","Internet":[{"ID":"1"}]});
        assert_eq!(find_items(&value, "internet").len(), 1);
        assert_eq!(value_usize(&value["Total"]), Some(1));
    }
}
