//! さくらのクラウド クラウドHSM の読み取り専用クライアント。
//!
//! エンドポイントは IaaS と同じ `{root}/{zone}/api/cloud/1.1/` の下の
//! `cloudhsm/...`。認証もAPIキーのBasic認証で、[`SacloudClient`] をそのまま使う。
//!
//! 仕様（KMS / SecretManager / CloudHSM API v1.1.0）には癖が多い。
//! 一覧の配列キーが `CloudHSMs` / `Clients` / `Licenses` / `CloudHSMDocuments`
//! と不揃いで、ページングは `Count` / `From` のオフセット方式。個別取得は
//! ラッパーキーが違うだけで中身は一覧と同じスキーマなので呼ばない。
//!
//! ピア（`/cloudhsm/cloudhsms/{id}/peers`）は仕様側のスキーマが未整備で、
//! GET のレスポンスがピア一覧ではなく HSM 本体を返す定義になっている。
//! 何が返るか仕様から判断できないため実装しない。

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{SacloudClient, flexible_number, null_as_default};

/// 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。
const MAX_PAGES: usize = 100;

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// HSM パーティション 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudHsm {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// `precreate` / `available` / `discontinued`。
    pub availability: String,
    pub service_class: String,
    pub ipv4_address: String,
    pub ipv4_network_address: String,
    pub ipv4_prefix_length: u32,
    pub local_router: String,
    pub created_at: String,
    pub modified_at: String,
}

impl CloudHsm {
    /// ネットワークの表示。アドレスとプレフィックス長をまとめる。
    pub fn network_label(&self) -> String {
        if self.ipv4_network_address.is_empty() {
            return String::new();
        }
        if self.ipv4_prefix_length == 0 {
            return self.ipv4_network_address.clone();
        }
        format!("{}/{}", self.ipv4_network_address, self.ipv4_prefix_length)
    }
}

/// HSM に登録されたクライアント。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudHsmClient {
    pub id: String,
    pub name: String,
    pub availability: String,
    /// クライアント証明書（PEM）。公開鍵側なので表示してよい。
    pub certificate: String,
    pub created_at: String,
    pub modified_at: String,
}

impl CloudHsmClient {
    /// 証明書の有無だけを一覧に出す。本文は詳細で見る。
    pub fn certificate_label(&self) -> &'static str {
        if self.certificate.is_empty() {
            ""
        } else {
            "あり"
        }
    }
}

/// ソフトウェアライセンス。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudHsmLicense {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub service_class: String,
    pub created_at: String,
    pub modified_at: String,
}

/// ライセンスに紐づくドキュメント。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudHsmDocument {
    pub id: String,
    pub name: String,
    pub license_id: String,
    pub created_at: String,
    pub modified_at: String,
}

/// `precreate` / `available` / `discontinued` の日本語表示。
pub fn availability_label(raw: &str) -> String {
    match raw {
        "available" => "利用可能".to_string(),
        "precreate" => "準備中".to_string(),
        "discontinued" => "廃止".to_string(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// デシリアライズ用の内部型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawCloudHsm {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
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
    #[serde(rename = "IPv4Address")]
    ipv4_address: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "IPv4NetworkAddress")]
    ipv4_network_address: String,
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "IPv4PrefixLength")]
    ipv4_prefix_length: u32,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "LocalRouter")]
    local_router: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "CreatedAt")]
    created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ModifiedAt")]
    modified_at: String,
}

#[derive(Debug, Deserialize)]
struct RawClient {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Availability")]
    availability: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Certificate")]
    certificate: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "CreatedAt")]
    created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ModifiedAt")]
    modified_at: String,
}

#[derive(Debug, Deserialize)]
struct RawLicense {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
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
    #[serde(rename = "ServiceClass")]
    service_class: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "CreatedAt")]
    created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ModifiedAt")]
    modified_at: String,
}

#[derive(Debug, Deserialize)]
struct RawDocument {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "CreatedAt")]
    created_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ModifiedAt")]
    modified_at: String,
}

// ---------------------------------------------------------------------------
// パース
// ---------------------------------------------------------------------------

fn parse_hsms(body: &str) -> Result<(Vec<CloudHsm>, usize)> {
    let (raws, total) = parse_page::<RawCloudHsm>(body, "CloudHSMs")?;
    Ok((
        raws.into_iter()
            .map(|raw| CloudHsm {
                id: raw.id,
                name: raw.name,
                description: raw.description,
                tags: raw.tags,
                availability: raw.availability,
                service_class: raw.service_class,
                ipv4_address: raw.ipv4_address,
                ipv4_network_address: raw.ipv4_network_address,
                ipv4_prefix_length: raw.ipv4_prefix_length,
                local_router: raw.local_router,
                created_at: raw.created_at,
                modified_at: raw.modified_at,
            })
            .collect(),
        total,
    ))
}

fn parse_clients(body: &str) -> Result<(Vec<CloudHsmClient>, usize)> {
    let (raws, total) = parse_page::<RawClient>(body, "Clients")?;
    Ok((
        raws.into_iter()
            .map(|raw| CloudHsmClient {
                id: raw.id,
                name: raw.name,
                availability: raw.availability,
                certificate: raw.certificate,
                created_at: raw.created_at,
                modified_at: raw.modified_at,
            })
            .collect(),
        total,
    ))
}

fn parse_licenses(body: &str) -> Result<(Vec<CloudHsmLicense>, usize)> {
    let (raws, total) = parse_page::<RawLicense>(body, "Licenses")?;
    Ok((
        raws.into_iter()
            .map(|raw| CloudHsmLicense {
                id: raw.id,
                name: raw.name,
                description: raw.description,
                tags: raw.tags,
                service_class: raw.service_class,
                created_at: raw.created_at,
                modified_at: raw.modified_at,
            })
            .collect(),
        total,
    ))
}

fn parse_documents(body: &str, license_id: &str) -> Result<(Vec<CloudHsmDocument>, usize)> {
    let (raws, total) = parse_page::<RawDocument>(body, "CloudHSMDocuments")?;
    Ok((
        raws.into_iter()
            .map(|raw| CloudHsmDocument {
                id: raw.id,
                name: raw.name,
                license_id: license_id.to_string(),
                created_at: raw.created_at,
                modified_at: raw.modified_at,
            })
            .collect(),
        total,
    ))
}

/// オフセット方式のページング封筒を、配列キーを名指しして解く。
///
/// 一覧の配列キーは `CloudHSMs` / `Clients` / `Licenses` / `CloudHSMDocuments`
/// と不揃いなので、どれでも拾う作りにすると別リソースを取り違える。
fn parse_page<T: serde::de::DeserializeOwned>(body: &str, key: &str) -> Result<(Vec<T>, usize)> {
    use anyhow::Context;
    let value: serde_json::Value = parse_json(body)?;
    let total = value
        .get("Total")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;
    let mut items = Vec::new();
    for raw in value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        if raw.is_null() {
            continue;
        }
        items.push(
            serde_json::from_value(raw)
                .with_context(|| format!("クラウドHSMの{key}の解析に失敗しました"))?,
        );
    }
    Ok((items, total))
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    use anyhow::Context;
    let body = if body.trim().is_empty() { "{}" } else { body };
    serde_json::from_str(body).with_context(|| {
        let head: String = body.chars().take(200).collect();
        format!("クラウドHSM APIレスポンスの解析に失敗しました: {head}")
    })
}

// ---------------------------------------------------------------------------
// API 呼び出し
// ---------------------------------------------------------------------------

impl SacloudClient {
    pub async fn list_cloudhsms(&self, zone: &str) -> Result<Vec<CloudHsm>> {
        self.collect_hsm_pages(zone, "cloudhsm/cloudhsms", parse_hsms)
            .await
    }

    pub async fn list_cloudhsm_clients(
        &self,
        zone: &str,
        hsm_id: &str,
    ) -> Result<Vec<CloudHsmClient>> {
        let path = format!("cloudhsm/cloudhsms/{hsm_id}/clients");
        self.collect_hsm_pages(zone, &path, parse_clients).await
    }

    pub async fn list_cloudhsm_licenses(&self, zone: &str) -> Result<Vec<CloudHsmLicense>> {
        self.collect_hsm_pages(zone, "cloudhsm/licenses", parse_licenses)
            .await
    }

    pub async fn list_cloudhsm_documents(
        &self,
        zone: &str,
        license_id: &str,
    ) -> Result<Vec<CloudHsmDocument>> {
        let path = format!("cloudhsm/licenses/{license_id}/documents");
        let owned = license_id.to_string();
        self.collect_hsm_pages(zone, &path, move |body| parse_documents(body, &owned))
            .await
    }

    /// `Count` / `From` のオフセット方式で全件集める。
    async fn collect_hsm_pages<T, F>(&self, zone: &str, path: &str, parse: F) -> Result<Vec<T>>
    where
        F: Fn(&str) -> Result<(Vec<T>, usize)>,
    {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({ "Count": PAGE_SIZE, "From": from });
            let value: serde_json::Value = self
                .request_in_zone(zone, Method::GET, path, Some(body))
                .await?;
            let (items, total) = parse(&value.to_string())?;
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

    /// 一覧の配列キーは `CloudHSMs`。ID は数値に見えるが文字列。
    #[test]
    fn parses_hsm_list() {
        let body = r#"{
            "Count": 1, "From": 0, "Total": 1,
            "CloudHSMs": [{
                "ID": "110000000000",
                "CreatedAt": "2025-02-05T12:19:22.551827+09:00",
                "ModifiedAt": "2025-02-05T12:19:22.551827+09:00",
                "ServiceClass": "cloud/cloudhsm/partition",
                "Availability": "available",
                "Name": "名前",
                "Description": "説明",
                "Tags": ["tag1"],
                "IPv4NetworkAddress": "192.168.100.0",
                "IPv4PrefixLength": 24,
                "IPv4Address": "192.168.100.11",
                "LocalRouter": "lr-1",
                "InitialData": "x"
            }]
        }"#;
        let (items, total) = parse_hsms(body).unwrap();
        assert_eq!(total, 1);
        let hsm = &items[0];
        assert_eq!(hsm.id, "110000000000");
        assert_eq!(hsm.ipv4_address, "192.168.100.11");
        assert_eq!(hsm.network_label(), "192.168.100.0/24");
        assert_eq!(availability_label(&hsm.availability), "利用可能");
        assert_eq!(hsm.tags, vec!["tag1"]);
    }

    /// クライアントの配列キーは `Clients`。ID は ULID で数値ではない。
    #[test]
    fn parses_client_list_with_ulid_ids() {
        let body = r#"{
            "Count": 1, "From": 0, "Total": 1,
            "Clients": [{
                "ID": "01JP9500000000000000000000",
                "CreatedAt": "2025-02-05T12:19:22.551827+09:00",
                "ModifiedAt": "2025-02-05T12:19:22.551827+09:00",
                "Availability": "precreate",
                "Name": "client-1",
                "Certificate": "-----BEGIN CERTIFICATE-----\nMII\n-----END CERTIFICATE-----"
            }]
        }"#;
        let (items, _) = parse_clients(body).unwrap();
        let client = &items[0];
        assert_eq!(client.id, "01JP9500000000000000000000");
        assert_eq!(availability_label(&client.availability), "準備中");
        assert_eq!(client.certificate_label(), "あり");

        let empty = CloudHsmClient::default();
        assert_eq!(empty.certificate_label(), "");
    }

    /// ライセンスの配列キーは `Licenses`。
    #[test]
    fn parses_license_list() {
        let body = r#"{
            "Total": 1,
            "Licenses": [{
                "ID": "110000000000",
                "ServiceClass": "cloud/cloudhsm/license/l7",
                "Name": "ライセンス",
                "Description": "説明",
                "Tags": ["a"],
                "CreatedAt": "2025-02-05T12:19:22.551827+09:00"
            }]
        }"#;
        let (items, _) = parse_licenses(body).unwrap();
        assert_eq!(items[0].service_class, "cloud/cloudhsm/license/l7");
        assert_eq!(items[0].name, "ライセンス");
    }

    /// ドキュメントだけ配列キーにプレフィックスが付く（`CloudHSMDocuments`）。
    /// 親のライセンスIDはレスポンスに無いので、呼び出し側で補う。
    #[test]
    fn parses_document_list_and_keeps_the_parent_license() {
        let body = r#"{
            "Total": 1,
            "CloudHSMDocuments": [{
                "ID": "110000000001",
                "Name": "ドキュメント名",
                "CreatedAt": "2025-02-05T12:19:22.551827+09:00",
                "ModifiedAt": "2025-02-05T12:19:22.551827+09:00"
            }]
        }"#;
        let (items, total) = parse_documents(body, "lic-9").unwrap();
        assert_eq!(total, 1);
        assert_eq!(items[0].id, "110000000001");
        assert_eq!(items[0].name, "ドキュメント名");
        assert_eq!(items[0].license_id, "lic-9");
    }

    /// 4種類ある配列キーを取り違えないこと。
    /// 別リソースのキーしか無い本文からは何も拾わない。
    #[test]
    fn does_not_confuse_the_four_list_keys() {
        let (items, _) = parse_hsms(r#"{"Total": 1, "Licenses": [{"ID": "1"}]}"#).unwrap();
        assert!(items.is_empty());

        let (items, _) = parse_licenses(r#"{"Total": 1, "Clients": [{"ID": "1"}]}"#).unwrap();
        assert!(items.is_empty());
    }

    /// null や欠けた項目でも落ちないこと。
    #[test]
    fn tolerates_nulls_and_missing_fields() {
        let body = r#"{"Total": 2, "CloudHSMs": [null, {"ID": "1", "Tags": null,
                       "Name": null, "IPv4PrefixLength": null}]}"#;
        let (items, _) = parse_hsms(body).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].tags.is_empty());
        assert_eq!(items[0].ipv4_prefix_length, 0);
        assert_eq!(items[0].network_label(), "");

        let (items, total) = parse_hsms("{}").unwrap();
        assert!(items.is_empty());
        assert_eq!(total, 0);
    }

    /// プレフィックス長が取れないときはアドレスだけ出す。
    #[test]
    fn network_label_without_prefix_shows_the_address_alone() {
        let hsm = CloudHsm {
            ipv4_network_address: "192.168.100.0".to_string(),
            ..CloudHsm::default()
        };
        assert_eq!(hsm.network_label(), "192.168.100.0");
    }
}
