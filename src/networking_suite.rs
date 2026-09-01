//! さくらのクラウド ネットワークスイート (CR) の読み取り専用クライアント。
//!
//! エンドポイントは IaaS と同じ `{root}/{zone}/api/cloud/1.1/` の下の
//! `networking-suite/...`。認証もAPIキーのBasic認証で [`SacloudClient`] を使うが、
//! 検索条件は IaaS のような JSON ではなく普通のクエリ文字列。
//!
//! リソースは SRN（`srnv1:{ロケーション}:{リソース種}:{ID}`）で参照し合う。
//! サブネットグループはリージョンスコープ、サブネットとアドレスはゾーンスコープ。
//!
//! 仕様の説明には「ゾーン部分を差し替えて使う」とあるが、本番で実際に叩くと
//! **is1c 以外は 500 を返す**（is1a / is1b / tk1a / tk1b で確認）。
//! 仕様の `servers` も is1c の1件だけなので、受付ゾーンは is1c に固定する。
//!
//! 一覧APIには親の SRN が必須なので、横断的な取得はできない。
//! サブネットはサブネットグループごと、アドレスはサブネットごとにしか引けない。
//!
//! NIC接続（インターフェースコネクション）は GET が仕様に無く、このAPIだけでは
//! 一覧できない。IaaS の `/server/{id}` から拾って SRN を組み立てる必要があるため
//! 対応していない。

use anyhow::Result;
use serde::Deserialize;

use crate::sacloud::{SacloudClient, flexible_number, null_as_default};

const SUFFIX: &str = "api/cloud/1.1";

/// 本番での受付ゾーン。他ゾーンは 500 を返すため決め打ちにする。
const PRODUCTION_ZONE: &str = "is1c";

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// サブネットグループ。リージョン単位。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubnetGroup {
    pub srn: String,
    pub name: String,
    pub description: String,
    /// IPv4アドレス範囲（CIDR）。
    pub cidr: String,
    /// リージョンコード（`is1` など）。
    pub region: String,
}

impl SubnetGroup {
    pub fn id(&self) -> String {
        srn_id(&self.srn)
    }
}

/// サブネット。ゾーン単位で、サブネットグループに属する。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Subnet {
    pub srn: String,
    pub name: String,
    pub description: String,
    pub cidr: String,
    /// ゾーンコード（`is1c` など）。
    pub zone: String,
    /// 親サブネットグループの SRN。
    pub subnet_group_srn: String,
}

impl Subnet {
    pub fn id(&self) -> String {
        srn_id(&self.srn)
    }
}

/// サブネットから払い出されたアドレス。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubnetAddress {
    pub srn: String,
    /// `EPHEMERAL_ADDRESS`。仕様上は enum ではなく自由文字列。
    pub address_type: String,
    /// `IPv4`。同じく自由文字列。
    pub ip_version: String,
    pub ip_address: String,
    /// 所属サブネットの SRN。
    pub subnet_srn: String,
}

impl SubnetAddress {
    pub fn id(&self) -> String {
        srn_id(&self.srn)
    }

    pub fn address_type_label(&self) -> String {
        match self.address_type.as_str() {
            "EPHEMERAL_ADDRESS" => "エフェメラル".to_string(),
            other => other.to_string(),
        }
    }
}

/// SRN（`srnv1:{ロケーション}:{リソース種}:{ID}`）から末尾の ID を取り出す。
///
/// 各リソースは数値IDのフィールドを持たず、SRN から切り出す運用になっている。
pub fn srn_id(srn: &str) -> String {
    srn.rsplit(':').next().unwrap_or_default().to_string()
}

// ---------------------------------------------------------------------------
// デシリアライズ用の内部型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SubnetGroupPage {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Total")]
    total: usize,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "SubnetGroups")]
    subnet_groups: Vec<Option<RawSubnetGroup>>,
}

#[derive(Debug, Deserialize)]
struct RawSubnetGroup {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "SRN")]
    srn: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Description")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "IPv4AddressRangeCIDR")]
    cidr: String,
    #[serde(rename = "Region")]
    region: Option<RawCode>,
}

#[derive(Debug, Deserialize)]
struct SubnetPage {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Total")]
    total: usize,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Subnets")]
    subnets: Vec<Option<RawSubnet>>,
}

#[derive(Debug, Deserialize)]
struct RawSubnet {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "SRN")]
    srn: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Description")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "IPv4AddressRangeCIDR")]
    cidr: String,
    #[serde(rename = "Zone")]
    zone: Option<RawCode>,
    #[serde(rename = "SubnetGroup")]
    subnet_group: Option<RawSrnRef>,
}

#[derive(Debug, Deserialize)]
struct AddressPage {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Total")]
    total: usize,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Addresses")]
    addresses: Vec<Option<RawAddress>>,
}

#[derive(Debug, Deserialize)]
struct RawAddress {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "SRN")]
    srn: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "AddressType")]
    address_type: String,
    /// 仕様の綴りは `IPVersion`（V が大文字）。
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "IPVersion")]
    ip_version: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "IPAddress")]
    ip_address: String,
    #[serde(rename = "Subnet")]
    subnet: Option<RawSrnRef>,
}

#[derive(Debug, Deserialize)]
struct RawCode {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Code")]
    code: String,
}

#[derive(Debug, Deserialize)]
struct RawSrnRef {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "SRN")]
    srn: String,
}

// ---------------------------------------------------------------------------
// パース
// ---------------------------------------------------------------------------

fn parse_subnet_groups(body: &str) -> Result<(Vec<SubnetGroup>, usize)> {
    let page: SubnetGroupPage = parse_json(body)?;
    Ok((
        page.subnet_groups
            .into_iter()
            .flatten()
            .map(|raw| SubnetGroup {
                srn: raw.srn,
                name: raw.name,
                description: raw.description,
                cidr: raw.cidr,
                region: raw.region.map(|r| r.code).unwrap_or_default(),
            })
            .collect(),
        page.total,
    ))
}

fn parse_subnets(body: &str) -> Result<(Vec<Subnet>, usize)> {
    let page: SubnetPage = parse_json(body)?;
    Ok((
        page.subnets
            .into_iter()
            .flatten()
            .map(|raw| Subnet {
                srn: raw.srn,
                name: raw.name,
                description: raw.description,
                cidr: raw.cidr,
                zone: raw.zone.map(|z| z.code).unwrap_or_default(),
                subnet_group_srn: raw.subnet_group.map(|g| g.srn).unwrap_or_default(),
            })
            .collect(),
        page.total,
    ))
}

fn parse_addresses(body: &str) -> Result<(Vec<SubnetAddress>, usize)> {
    let page: AddressPage = parse_json(body)?;
    Ok((
        page.addresses
            .into_iter()
            .flatten()
            .map(|raw| SubnetAddress {
                srn: raw.srn,
                address_type: raw.address_type,
                ip_version: raw.ip_version,
                ip_address: raw.ip_address,
                subnet_srn: raw.subnet.map(|s| s.srn).unwrap_or_default(),
            })
            .collect(),
        page.total,
    ))
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    use anyhow::Context;
    let body = if body.trim().is_empty() { "{}" } else { body };
    serde_json::from_str(body).with_context(|| {
        let head: String = body.chars().take(200).collect();
        format!("ネットワークスイートAPIレスポンスの解析に失敗しました: {head}")
    })
}

// ---------------------------------------------------------------------------
// API 呼び出し
// ---------------------------------------------------------------------------

/// 接続先に応じた受付ゾーン。
///
/// 本番は is1c 固定。社内テスト環境（cloud-test）に is1c は存在しないので、
/// 決め打ちにせず既定ゾーンへ回す。
fn networking_suite_zone<'a>(api_root: &str, default_zone: &'a str) -> &'a str {
    if api_root == crate::config::TEST_API_ROOT {
        default_zone
    } else {
        PRODUCTION_ZONE
    }
}

impl SacloudClient {
    /// ネットワークスイートの問い合わせ先ゾーン。画面にも表示する。
    pub fn networking_suite_zone(&self) -> &str {
        networking_suite_zone(self.api_root(), self.default_zone())
    }

    /// サブネットグループ一覧。
    ///
    /// 仕様にページングのリクエストパラメータが無いため1回だけ引く。
    pub async fn list_subnet_groups(&self) -> Result<Vec<SubnetGroup>> {
        let zone = self.networking_suite_zone().to_string();
        let value: serde_json::Value = self
            .request_zoned_service(&zone, SUFFIX, "networking-suite/subnet-groups", &[])
            .await?;
        Ok(parse_subnet_groups(&value.to_string())?.0)
    }

    /// サブネット一覧。親のサブネットグループ SRN が必須。
    pub async fn list_subnets(&self, subnet_group_srn: &str) -> Result<Vec<Subnet>> {
        let zone = self.networking_suite_zone().to_string();
        let value: serde_json::Value = self
            .request_zoned_service(
                &zone,
                SUFFIX,
                "networking-suite/subnets",
                &[("subnetGroupSRN", subnet_group_srn.to_string())],
            )
            .await?;
        Ok(parse_subnets(&value.to_string())?.0)
    }

    /// アドレス一覧。親のサブネット SRN が必須。
    pub async fn list_subnet_addresses(&self, subnet_srn: &str) -> Result<Vec<SubnetAddress>> {
        let zone = self.networking_suite_zone().to_string();
        let value: serde_json::Value = self
            .request_zoned_service(
                &zone,
                SUFFIX,
                "networking-suite/addresses",
                &[("subnetSRN", subnet_srn.to_string())],
            )
            .await?;
        Ok(parse_addresses(&value.to_string())?.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仕様の example をそのまま流す。キーは全て PascalCase。
    #[test]
    fn parses_subnet_groups_from_the_specification_example() {
        let body = r#"{
            "Total": 1, "From": 0, "Count": 1,
            "SubnetGroups": [{
                "SRN": "srnv1:sakura-is1:sakura.networking-suite.subnet-group:1234567890",
                "Name": "サブネットグループ名",
                "Description": "サブネットグループの説明",
                "IPv4AddressRangeCIDR": "10.0.0.0/20",
                "Region": {"Code": "is1"}
            }]
        }"#;
        let (items, total) = parse_subnet_groups(body).unwrap();
        assert_eq!(total, 1);
        let group = &items[0];
        assert_eq!(group.name, "サブネットグループ名");
        assert_eq!(group.cidr, "10.0.0.0/20");
        assert_eq!(group.region, "is1");
        // 数値IDのフィールドは無いので SRN から切り出す。
        assert_eq!(group.id(), "1234567890");
    }

    #[test]
    fn parses_subnets_with_the_parent_reference() {
        let body = r#"{
            "Total": 1, "From": 0, "Count": 1,
            "Subnets": [{
                "SRN": "srnv1:sakura-is1c:sakura.networking-suite.subnet:2345678901",
                "Name": "サブネット名",
                "Description": "サブネットの説明",
                "IPv4AddressRangeCIDR": "10.0.0.0/26",
                "Zone": {"Code": "is1c"},
                "SubnetGroup": {
                    "SRN": "srnv1:sakura-is1:sakura.networking-suite.subnet-group:1234567890"
                }
            }]
        }"#;
        let (items, _) = parse_subnets(body).unwrap();
        let subnet = &items[0];
        assert_eq!(subnet.zone, "is1c");
        assert_eq!(subnet.cidr, "10.0.0.0/26");
        assert_eq!(subnet.id(), "2345678901");
        assert_eq!(
            subnet.subnet_group_srn,
            "srnv1:sakura-is1:sakura.networking-suite.subnet-group:1234567890"
        );
    }

    /// アドレスの綴りは `IPVersion`（V が大文字）。取り違えると黙って空になる。
    #[test]
    fn parses_addresses_with_capital_v_in_ip_version() {
        let body = r#"{
            "Total": 1, "From": 0, "Count": 1,
            "Addresses": [{
                "SRN": "srnv1:sakura-is1c:sakura.networking-suite.address:5678901234",
                "AddressType": "EPHEMERAL_ADDRESS",
                "IPVersion": "IPv4",
                "IPAddress": "10.0.0.10",
                "Subnet": {
                    "SRN": "srnv1:sakura-is1c:sakura.networking-suite.subnet:2345678901"
                }
            }]
        }"#;
        let (items, _) = parse_addresses(body).unwrap();
        let address = &items[0];
        assert_eq!(address.ip_version, "IPv4");
        assert_eq!(address.ip_address, "10.0.0.10");
        assert_eq!(address.address_type_label(), "エフェメラル");
        assert_eq!(address.id(), "5678901234");
    }

    /// SRN から ID を切り出す。リソース種にドットが入るので、
    /// 末尾のコロン区切りで切る。
    #[test]
    fn extracts_the_id_from_the_srn() {
        let srn = "srnv1:sakura-is1c:sakura.networking-suite.subnet:2345678901";
        assert_eq!(srn_id(srn), "2345678901");

        // 壊れた入力でも落ちない。
        assert_eq!(srn_id(""), "");
        assert_eq!(srn_id("abc"), "abc");
    }

    /// 接続先ごとにゾーンを解決する。
    /// 本番は is1c 固定（他ゾーンは 500 を返すことを実地で確認）。
    #[test]
    fn zone_follows_the_environment() {
        assert_eq!(
            networking_suite_zone("https://secure.sakura.ad.jp/cloud/zone", "is1b"),
            "is1c"
        );
        assert_eq!(
            networking_suite_zone(crate::config::TEST_API_ROOT, "is1x"),
            "is1x"
        );
    }

    /// 未知の種別はそのまま出す。仕様上 enum ではなく自由文字列。
    #[test]
    fn unknown_address_type_is_shown_as_is() {
        let address = SubnetAddress {
            address_type: "STATIC_ADDRESS".to_string(),
            ..SubnetAddress::default()
        };
        assert_eq!(address.address_type_label(), "STATIC_ADDRESS");
    }

    /// null や欠けた項目、空レスポンスでも落ちないこと。
    #[test]
    fn tolerates_nulls_and_empty_responses() {
        let body = r#"{"Total": null, "Subnets": [null, {"SRN": null, "Zone": null,
                       "SubnetGroup": null}]}"#;
        let (items, total) = parse_subnets(body).unwrap();
        assert_eq!(total, 0);
        assert_eq!(items.len(), 1);
        assert!(items[0].zone.is_empty());
        assert!(items[0].subnet_group_srn.is_empty());

        assert!(parse_subnet_groups("{}").unwrap().0.is_empty());
        assert!(parse_addresses("{}").unwrap().0.is_empty());
    }
}
