//! さくらのクラウドの請求情報（閲覧のみ）。
//!
//! IaaS とは別の接尾辞（`api/system/1.0`）にぶら下がっている。
//! 請求の取得にはアカウントIDと会員コードが要るので、まず `auth-status` から引く。

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use reqwest::Method;
use serde::Deserialize;

use crate::sacloud::{BILLING_SUFFIX, SacloudClient, flexible_number, null_as_default};

/// 請求を引くのに必要な識別子。
#[derive(Debug, Clone)]
pub struct BillingIdentity {
    pub account_id: String,
    pub member_code: String,
    /// 表示用のアカウント名。
    pub account_name: String,
}

/// 請求 1 件（月ごと）。
#[derive(Debug, Clone)]
pub struct Bill {
    pub id: String,
    /// 金額。単位は API の返す通り（円）。
    pub amount: i64,
    pub date: Option<String>,
    pub paid: bool,
    pub pay_limit: Option<String>,
}

/// 請求の明細 1 行（おおむね 1 リソース）。
#[derive(Debug, Clone)]
pub struct BillDetail {
    pub description: String,
    pub amount: i64,
    pub zone: String,
    /// サービス種別（`cloud/server/plan/...` のようなパス）。
    pub service_class_path: String,
    /// 整形済みの利用量。
    pub usage: String,
    pub contract_end_at: Option<String>,
}

impl BillDetail {
    /// サービス種別を読みやすい名前にする。
    ///
    /// `cloud/server/plan/1core-1gb` のようなパスの、意味のある部分を拾う。
    pub fn service_label(&self) -> String {
        let parts: Vec<&str> = self
            .service_class_path
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        match parts.as_slice() {
            [] => "(不明)".to_string(),
            // 先頭の `cloud` は全部に付くので落とす。
            ["cloud", rest @ ..] if !rest.is_empty() => rest.join("/"),
            other => other.join("/"),
        }
    }

    /// 集計に使う大分類（`server` / `disk` など）。
    pub fn category(&self) -> String {
        self.service_label()
            .split('/')
            .next()
            .unwrap_or("(不明)")
            .to_string()
    }
}

/// 明細を指定のキーで集計する。金額の大きい順に返す。
pub fn summarize<F>(details: &[BillDetail], key: F) -> Vec<(String, i64, usize)>
where
    F: Fn(&BillDetail) -> String,
{
    let mut totals: BTreeMap<String, (i64, usize)> = BTreeMap::new();
    for detail in details {
        let entry = totals.entry(key(detail)).or_default();
        entry.0 += detail.amount;
        entry.1 += 1;
    }
    let mut out: Vec<(String, i64, usize)> = totals
        .into_iter()
        .map(|(name, (amount, count))| (name, amount, count))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

// --- API のレスポンス形状 ---

/// `auth-status` は封筒に包まず、そのまま返ってくる。
#[derive(Debug, Deserialize)]
struct AuthStatusResponse {
    #[serde(rename = "Account")]
    account: Option<NakedAccount>,
    #[serde(rename = "Member")]
    member: Option<NakedMember>,
}

#[derive(Debug, Deserialize)]
struct NakedAccount {
    #[serde(rename = "ID", default, deserialize_with = "null_as_default")]
    id: serde_json::Value,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct NakedMember {
    #[serde(rename = "Code", default, deserialize_with = "null_as_default")]
    code: String,
}

#[derive(Debug, Deserialize)]
struct BillsResponse {
    #[serde(rename = "Bills", default, deserialize_with = "null_as_default")]
    bills: Vec<NakedBill>,
}

#[derive(Debug, Deserialize)]
struct NakedBill {
    #[serde(rename = "BillID", default, deserialize_with = "null_as_default")]
    bill_id: serde_json::Value,
    #[serde(rename = "Amount", default, deserialize_with = "flexible_number")]
    amount: i64,
    #[serde(rename = "Date")]
    date: Option<String>,
    #[serde(rename = "Paid", default)]
    paid: bool,
    #[serde(rename = "PayLimit")]
    pay_limit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BillDetailsResponse {
    #[serde(rename = "BillDetails", default, deserialize_with = "null_as_default")]
    details: Vec<NakedBillDetail>,
}

#[derive(Debug, Deserialize)]
struct NakedBillDetail {
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Amount", default, deserialize_with = "flexible_number")]
    amount: i64,
    #[serde(rename = "Zone", default, deserialize_with = "null_as_default")]
    zone: String,
    #[serde(
        rename = "ServiceClassPath",
        default,
        deserialize_with = "null_as_default"
    )]
    service_class_path: String,
    #[serde(
        rename = "FormattedUsage",
        default,
        deserialize_with = "null_as_default"
    )]
    formatted_usage: String,
    #[serde(rename = "ContractEndAt")]
    contract_end_at: Option<String>,
}

/// ID は文字列でも数値でも返るので、表示用の文字列に寄せる。
fn id_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

impl SacloudClient {
    /// 請求を引くのに必要なアカウントIDと会員コードを取る。
    pub async fn billing_identity(&self) -> Result<BillingIdentity> {
        // auth-status は IaaS 側の接尾辞にある。
        let res: AuthStatusResponse = self
            .request_common(Method::GET, "auth-status", None)
            .await?;
        let account = res.account.context("アカウント情報が含まれていません")?;
        let member = res.member.context("会員情報が含まれていません")?;
        Ok(BillingIdentity {
            account_id: id_to_string(&account.id),
            member_code: member.code,
            account_name: account.name,
        })
    }

    /// 指定した年の請求を新しい順に返す。
    ///
    /// 年を付けずに呼ぶと直近しか返らないため、年を明示して引く。
    pub async fn list_bills(&self, account_id: &str, year: i32) -> Result<Vec<Bill>> {
        let path = format!("bill/by-contract/{account_id}/{year}");
        let res: BillsResponse = self
            .request_with_suffix(
                self.default_zone(),
                BILLING_SUFFIX,
                Method::GET,
                &path,
                None,
            )
            .await?;
        let mut bills: Vec<Bill> = res
            .bills
            .into_iter()
            .map(|b| Bill {
                id: id_to_string(&b.bill_id),
                amount: b.amount,
                date: b.date,
                paid: b.paid,
                pay_limit: b.pay_limit,
            })
            .collect();
        // 請求日の新しい順（日付が無いものは後ろ）。
        bills.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(bills)
    }

    /// 請求の明細。
    pub async fn bill_details(&self, member_code: &str, bill_id: &str) -> Result<Vec<BillDetail>> {
        let path = format!("billdetail/{member_code}/{bill_id}");
        let res: BillDetailsResponse = self
            .request_with_suffix(
                self.default_zone(),
                BILLING_SUFFIX,
                Method::GET,
                &path,
                None,
            )
            .await?;
        let mut details: Vec<BillDetail> = res
            .details
            .into_iter()
            .map(|d| BillDetail {
                description: d.description,
                amount: d.amount,
                zone: d.zone,
                service_class_path: d.service_class_path,
                usage: d.formatted_usage,
                contract_end_at: d.contract_end_at,
            })
            .collect();
        details.sort_by_key(|d| std::cmp::Reverse(d.amount));
        Ok(details)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実際のレスポンスは封筒に包まれず、余分なキーも多い。
    #[test]
    fn parses_auth_status() {
        let body = r#"{
            "Account": {"ID": "113600000000", "Name": "さくら太郎", "Code": "acc",
                        "Class": "account", "Tags": [], "UsedServers": 3},
            "Member": {"Class": "member", "Code": "ixt15226", "Errors": []},
            "AuthClass": "account", "IsAPIKey": true, "is_ok": true
        }"#;
        let res: AuthStatusResponse = serde_json::from_str(body).unwrap();
        assert_eq!(id_to_string(&res.account.unwrap().id), "113600000000");
        assert_eq!(res.member.unwrap().code, "ixt15226");
    }

    /// ID が数値で返ってきても扱えること。
    #[test]
    fn accepts_numeric_ids() {
        let body = r#"{"Account": {"ID": 113600000000}, "Member": {"Code": "m"}}"#;
        let res: AuthStatusResponse = serde_json::from_str(body).unwrap();
        assert_eq!(id_to_string(&res.account.unwrap().id), "113600000000");
    }

    #[test]
    fn parses_bills_newest_first() {
        let body = r#"{"Total": 2, "Bills": [
            {"BillID": 1, "Amount": 129800, "Date": "2026-06-30T00:00:00+09:00", "Paid": true},
            {"BillID": 2, "Amount": 128400, "Date": "2026-08-31T00:00:00+09:00", "Paid": false}
        ]}"#;
        let res: BillsResponse = serde_json::from_str(body).unwrap();
        let mut bills: Vec<Bill> = res
            .bills
            .into_iter()
            .map(|b| Bill {
                id: id_to_string(&b.bill_id),
                amount: b.amount,
                date: b.date,
                paid: b.paid,
                pay_limit: b.pay_limit,
            })
            .collect();
        bills.sort_by(|a, b| b.date.cmp(&a.date));
        assert_eq!(bills[0].id, "2", "新しい請求が先頭");
        assert_eq!(bills[0].amount, 128_400);
        assert!(!bills[0].paid);
    }

    #[test]
    fn parses_bill_details() {
        let body = r#"{"Total": 1, "BillDetails": [
            {"ContractID": 1, "Amount": 5280, "Description": "web-01",
             "ServiceClassPath": "cloud/server/plan/2core-4gb", "Zone": "is1b",
             "Usage": 2678400, "FormattedUsage": "744時間", "ContractEndAt": null}
        ]}"#;
        let res: BillDetailsResponse = serde_json::from_str(body).unwrap();
        let raw = res.details.into_iter().next().unwrap();
        let detail = BillDetail {
            description: raw.description,
            amount: raw.amount,
            zone: raw.zone,
            service_class_path: raw.service_class_path,
            usage: raw.formatted_usage,
            contract_end_at: raw.contract_end_at,
        };
        assert_eq!(detail.description, "web-01");
        // 先頭の cloud は落として読みやすくする。
        assert_eq!(detail.service_label(), "server/plan/2core-4gb");
        assert_eq!(detail.category(), "server");
        assert_eq!(detail.usage, "744時間");
    }

    #[test]
    fn service_label_handles_odd_paths() {
        let make = |path: &str| BillDetail {
            description: String::new(),
            amount: 0,
            zone: String::new(),
            service_class_path: path.to_string(),
            usage: String::new(),
            contract_end_at: None,
        };
        assert_eq!(make("").service_label(), "(不明)");
        assert_eq!(make("cloud").service_label(), "cloud");
        assert_eq!(make("cloud/disk/ssd/100gb").category(), "disk");
        assert_eq!(make("other/thing").service_label(), "other/thing");
    }

    /// ゾーン別・種別別の集計が金額の大きい順になること。
    #[test]
    fn summarizes_by_key() {
        let make = |amount: i64, zone: &str, path: &str| BillDetail {
            description: String::new(),
            amount,
            zone: zone.to_string(),
            service_class_path: path.to_string(),
            usage: String::new(),
            contract_end_at: None,
        };
        let details = vec![
            make(100, "is1b", "cloud/server/plan/a"),
            make(300, "tk1a", "cloud/disk/ssd"),
            make(200, "is1b", "cloud/server/plan/b"),
        ];

        let by_zone = summarize(&details, |d| d.zone.clone());
        assert_eq!(by_zone[0], ("is1b".to_string(), 300, 2));
        assert_eq!(by_zone[1], ("tk1a".to_string(), 300, 1));

        let by_category = summarize(&details, |d| d.category());
        assert_eq!(by_category[0], ("disk".to_string(), 300, 1));
        assert_eq!(by_category[1], ("server".to_string(), 300, 2));
    }

    #[test]
    fn empty_details_summarize_to_nothing() {
        assert!(summarize(&[], |d| d.zone.clone()).is_empty());
    }
}
