//! アプリケーション連携・マネージドサービスの読み取り専用ブラウザ。

use anyhow::{Context, Result};
use reqwest::Method;
use serde_json::{Value, json};

use crate::sacloud::SacloudClient;

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;
/// オブジェクトストレージ管理APIの受付ゾーン。
///
/// バケット自体の配置先サイトとは別物で、公式SDKも `is1a` を固定で使用する。
const OBJECT_STORAGE_API_ZONE: &str = "is1a";
const OBJECT_STORAGE_SUFFIX: &str = "api/objectstorage/1.0";
const WORKFLOWS_SUFFIX: &str = "api/workflow/1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManagedResourceKind {
    ObjectStorage,
    SimpleMq,
    EventBus,
    Workflows,
}

impl ManagedResourceKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::ObjectStorage => "オブジェクトストレージ",
            Self::SimpleMq => "シンプルMQ",
            Self::EventBus => "イベントバス",
            Self::Workflows => "ワークフロー",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManagedResource {
    /// 数値IDだけでなく、UUIDやバケット名も扱う。
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub resource_type: String,
    pub status: String,
    pub plan: String,
    pub created_at: String,
    pub details: Vec<(String, String)>,
}

impl ManagedResource {
    pub fn searchable(&self) -> String {
        format!(
            "{} {} {} {} {} {}",
            self.name,
            self.description,
            self.tags.join(" "),
            self.resource_type,
            self.status,
            self.plan
        )
    }
}

impl SacloudClient {
    pub async fn list_managed_resources(
        &self,
        kind: ManagedResourceKind,
    ) -> Result<Vec<ManagedResource>> {
        match kind {
            ManagedResourceKind::ObjectStorage => self.list_object_storage_buckets().await,
            ManagedResourceKind::SimpleMq => self.list_common_service("simplemq", kind).await,
            ManagedResourceKind::EventBus => self.list_eventbus_resources().await,
            ManagedResourceKind::Workflows => self.list_workflows().await,
        }
    }

    async fn list_common_service(
        &self,
        provider_class: &str,
        kind: ManagedResourceKind,
    ) -> Result<Vec<ManagedResource>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({
                "Filter": { "Provider.Class": provider_class },
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let value: Value = self
                .request_common(Method::GET, "commonserviceitem", Some(body))
                .await?;
            let total = value.get("Total").and_then(value_usize).unwrap_or(0);
            let items = value
                .get("CommonServiceItems")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let received = items.len();
            for item in items {
                // API側のフィルターを信用しきらず、別サービスの混入を防ぐ。
                if string_at(&item, "/Provider/Class") == provider_class {
                    out.push(parse_common_service(&item, kind)?);
                }
            }
            if received == 0 || from + received >= total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    async fn list_eventbus_resources(&self) -> Result<Vec<ManagedResource>> {
        let mut out = Vec::new();
        for class in [
            "eventbusschedule",
            "eventbustrigger",
            "eventbusprocessconfiguration",
        ] {
            out.extend(
                self.list_common_service(class, ManagedResourceKind::EventBus)
                    .await?,
            );
        }
        out.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.resource_type.cmp(&b.resource_type))
        });
        Ok(out)
    }

    async fn list_object_storage_buckets(&self) -> Result<Vec<ManagedResource>> {
        let sites: Value = self
            .request_with_suffix(
                OBJECT_STORAGE_API_ZONE,
                OBJECT_STORAGE_SUFFIX,
                Method::GET,
                "fed/v1/clusters",
                None,
            )
            .await?;
        let sites = sites
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        for site in sites {
            let site_id = string_at(&site, "/id");
            if site_id.is_empty() {
                continue;
            }
            // clusters は利用可能な全サイトを返す。サイトアカウント未作成の
            // サイトへ /buckets を要求すると認証エラー扱いになるため、契約済み
            // サイトだけを先に判定する。
            let account: Result<Value> = self
                .request_with_suffix(
                    OBJECT_STORAGE_API_ZONE,
                    OBJECT_STORAGE_SUFFIX,
                    Method::GET,
                    &format!("{site_id}/v2/account"),
                    None,
                )
                .await;
            match account {
                Ok(_) => {}
                Err(err) if is_api_status(&err, 404) => continue,
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("オブジェクトストレージ {site_id} のアカウント確認に失敗しました")
                    });
                }
            }
            let response: Value = self
                .request_with_suffix(
                    OBJECT_STORAGE_API_ZONE,
                    OBJECT_STORAGE_SUFFIX,
                    Method::GET,
                    &format!("{site_id}/v2/buckets"),
                    None,
                )
                .await
                .with_context(|| {
                    format!("オブジェクトストレージ {site_id} の取得に失敗しました")
                })?;
            let buckets = response
                .get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for bucket in buckets {
                let name = string_at(&bucket, "/name");
                let id = first_non_empty(&bucket, &["/resource_id", "/name"]);
                let plan = string_at(&bucket, "/plan/type");
                let mut details = Vec::new();
                add_detail(&mut details, "バケット名", name.clone());
                add_detail(&mut details, "サイト", site_id.clone());
                add_detail(
                    &mut details,
                    "サイト表示名",
                    first_non_empty(&site, &["/display_name", "/display_name_ja"]),
                );
                add_detail(&mut details, "リージョン", string_at(&site, "/region"));
                add_detail(
                    &mut details,
                    "S3エンドポイント",
                    string_at(&site, "/s3_endpoint"),
                );
                add_detail(
                    &mut details,
                    "サービスクラス",
                    string_at(&bucket, "/plan/service_class_path"),
                );
                out.push(ManagedResource {
                    id,
                    name,
                    description: String::new(),
                    tags: Vec::new(),
                    resource_type: site_id.clone(),
                    status: "available".to_string(),
                    plan,
                    created_at: String::new(),
                    details,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn list_workflows(&self) -> Result<Vec<ManagedResource>> {
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            let value: Value = self
                .request_with_suffix(
                    "tk1b",
                    WORKFLOWS_SUFFIX,
                    Method::GET,
                    &format!("workflows?Page={page}&PageLimit=500&SortBy=createdAt&Order=asc"),
                    None,
                )
                .await?;
            let total = value.get("Total").and_then(value_usize).unwrap_or(0);
            let items = value
                .get("Workflows")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let received = items.len();
            for item in items {
                out.push(parse_workflow(&item)?);
            }
            if received == 0 || out.len() >= total {
                break;
            }
        }
        Ok(out)
    }
}

fn parse_common_service(value: &Value, kind: ManagedResourceKind) -> Result<ManagedResource> {
    let id = first_non_empty(value, &["/ID", "/Name"]);
    anyhow::ensure!(!id.is_empty(), "{}のIDがありません", kind.title());
    let class = string_at(value, "/Provider/Class");
    let resource_type = match class.as_str() {
        "simplemq" => "キュー",
        "eventbusschedule" => "スケジュール",
        "eventbustrigger" => "トリガー",
        "eventbusprocessconfiguration" => "処理設定",
        _ => class.as_str(),
    }
    .to_string();
    let tags = string_array_at(value, "/Tags");
    let status = first_non_empty(value, &["/Availability", "/Status/Status"]);
    let plan = first_non_empty(value, &["/ServiceClass", "/Provider/ServiceClass"]);
    let mut details = Vec::new();
    add_detail(&mut details, "ID", id.clone());
    add_detail(&mut details, "種別", resource_type.clone());
    add_detail(&mut details, "状態", status.clone());
    add_detail(&mut details, "サービスクラス", plan.clone());
    if kind == ManagedResourceKind::SimpleMq {
        add_detail(
            &mut details,
            "キュー名",
            first_non_empty(value, &["/Status/QueueName", "/Name"]),
        );
        add_detail(
            &mut details,
            "可視性タイムアウト(秒)",
            string_at(value, "/Settings/VisibilityTimeoutSeconds"),
        );
        add_detail(
            &mut details,
            "保存期間(秒)",
            string_at(value, "/Settings/ExpireSeconds"),
        );
    } else {
        add_detail(
            &mut details,
            "プロバイダー",
            string_at(value, "/Provider/Name"),
        );
        match class.as_str() {
            "eventbusschedule" => {
                add_detail(
                    &mut details,
                    "処理設定ID",
                    string_at(value, "/Settings/ProcessConfigurationID"),
                );
                add_detail(
                    &mut details,
                    "開始時刻",
                    string_at(value, "/Settings/StartsAt"),
                );
                add_detail(
                    &mut details,
                    "crontab",
                    string_at(value, "/Settings/Crontab"),
                );
                let interval = [
                    string_at(value, "/Settings/RecurringStep"),
                    string_at(value, "/Settings/RecurringUnit"),
                ]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
                add_detail(&mut details, "実行間隔", interval);
            }
            "eventbustrigger" => {
                add_detail(
                    &mut details,
                    "イベントソース",
                    string_at(value, "/Settings/Source"),
                );
                add_detail(
                    &mut details,
                    "イベント種別",
                    string_array_at(value, "/Settings/Types").join(", "),
                );
                add_detail(
                    &mut details,
                    "処理設定ID",
                    string_at(value, "/Settings/ProcessConfigurationID"),
                );
            }
            "eventbusprocessconfiguration" => {
                add_detail(
                    &mut details,
                    "宛先",
                    string_at(value, "/Settings/Destination"),
                );
                add_detail(
                    &mut details,
                    "パラメータ",
                    string_at(value, "/Settings/Parameters"),
                );
            }
            _ => {}
        }
        add_detail(
            &mut details,
            "最終結果",
            string_at(value, "/Status/Message"),
        );
    }
    add_detail(&mut details, "タグ", tags.join(", "));
    add_detail(&mut details, "作成日時", string_at(value, "/CreatedAt"));
    add_detail(&mut details, "更新日時", string_at(value, "/ModifiedAt"));
    Ok(ManagedResource {
        id,
        name: string_at(value, "/Name"),
        description: string_at(value, "/Description"),
        tags,
        resource_type,
        status,
        plan,
        created_at: string_at(value, "/CreatedAt"),
        details,
    })
}

fn parse_workflow(value: &Value) -> Result<ManagedResource> {
    let id = first_non_empty(value, &["/Id", "/Name"]);
    anyhow::ensure!(!id.is_empty(), "ワークフローのIDがありません");
    let published = value
        .get("Publish")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let logging = value
        .get("Logging")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tags: Vec<String> = value
        .get("Tags")
        .and_then(Value::as_array)
        .map(|tags| tags.iter().map(|tag| string_at(tag, "/Name")).collect())
        .unwrap_or_default();
    let status = if published { "公開" } else { "下書き" }.to_string();
    let plan = string_at(value, "/ConcurrencyMode");
    let mut details = Vec::new();
    add_detail(&mut details, "ID", id.clone());
    add_detail(&mut details, "公開状態", status.clone());
    add_detail(
        &mut details,
        "ログ",
        if logging { "有効" } else { "無効" }.to_string(),
    );
    add_detail(&mut details, "同時実行モード", plan.clone());
    add_detail(
        &mut details,
        "サービスプリンシパルID",
        string_at(value, "/ServicePrincipalId"),
    );
    add_detail(&mut details, "タグ", tags.join(", "));
    add_detail(&mut details, "作成日時", string_at(value, "/CreatedAt"));
    add_detail(&mut details, "更新日時", string_at(value, "/UpdatedAt"));
    Ok(ManagedResource {
        id,
        name: string_at(value, "/Name"),
        description: string_at(value, "/Description"),
        tags,
        resource_type: "ワークフロー".to_string(),
        status,
        plan,
        created_at: string_at(value, "/CreatedAt"),
        details,
    })
}

fn add_detail(details: &mut Vec<(String, String)>, label: &str, value: String) {
    if !value.is_empty() {
        details.push((label.to_string(), value));
    }
}

fn first_non_empty(value: &Value, pointers: &[&str]) -> String {
    pointers
        .iter()
        .map(|pointer| string_at(value, pointer))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn string_array_at(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(value_string)
                .filter(|v| !v.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn string_at(value: &Value, pointer: &str) -> String {
    value.pointer(pointer).map(value_string).unwrap_or_default()
}

fn value_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => String::new(),
    }
}

fn value_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn is_api_status(error: &anyhow::Error, status: u16) -> bool {
    let marker = format!("API エラー ({status} ");
    error
        .chain()
        .any(|cause| cause.to_string().contains(&marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_mq_details() {
        let value = json!({
            "ID": "123", "Name": "orders", "Availability": "available",
            "Provider": {"Class": "simplemq", "ServiceClass": "cloud/simplemq"},
            "Settings": {"VisibilityTimeoutSeconds": 30, "ExpireSeconds": 3600},
            "Status": {"QueueName": "orders"}, "Tags": ["prod"]
        });
        let item = parse_common_service(&value, ManagedResourceKind::SimpleMq).unwrap();
        assert_eq!(item.id, "123");
        assert_eq!(item.resource_type, "キュー");
        assert!(item.searchable().contains("prod"));
    }

    #[test]
    fn parses_workflow_status_and_uuid() {
        let value = json!({
            "Id": "550e8400-e29b-41d4-a716-446655440000", "Name": "backup",
            "Publish": true, "Logging": false, "ConcurrencyMode": "lock",
            "Tags": [{"Name": "nightly"}]
        });
        let item = parse_workflow(&value).unwrap();
        assert_eq!(item.status, "公開");
        assert_eq!(item.plan, "lock");
    }

    #[test]
    fn object_storage_management_api_uses_the_official_fixed_zone() {
        // IaaSの既定ゾーンへ追従させると、is1a以外のプロファイルで401になる。
        assert_eq!(OBJECT_STORAGE_API_ZONE, "is1a");
    }

    #[test]
    fn recognizes_missing_site_account_without_hiding_authentication_errors() {
        let missing = anyhow::anyhow!("API エラー (404 Not Found): does not exist");
        let unauthorized = anyhow::anyhow!("API エラー (401 Unauthorized): Authentication failed");
        assert!(is_api_status(&missing, 404));
        assert!(!is_api_status(&unauthorized, 404));
    }
}
