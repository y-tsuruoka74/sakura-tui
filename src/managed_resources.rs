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
const WEBACCEL_API_ZONE: &str = "is1a";
const WEBACCEL_SUFFIX: &str = "api/webaccel/1.0";
const WORKFLOWS_SUFFIX: &str = "api/workflow/1.0";
const KMS_API_ZONE: &str = "is1a";
const KMS_SUFFIX: &str = "api/cloud/1.1/kms";
const IAM_SUFFIX: &str = "api/iam/1.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManagedResourceKind {
    AiEngine,
    ObjectStorage,
    SimpleMq,
    EventBus,
    Workflows,
    WebAccel,
    EnhancedLoadBalancer,
    LocalRouter,
    Gslb,
    Kms,
    Iam,
    SimpleNotification,
    AutoScale,
    EnhancedDb,
}

impl ManagedResourceKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::AiEngine => "AI Engineモデル",
            Self::ObjectStorage => "オブジェクトストレージ",
            Self::SimpleMq => "シンプルMQ",
            Self::EventBus => "イベントバス",
            Self::Workflows => "ワークフロー",
            Self::WebAccel => "ウェブアクセラレータ",
            Self::EnhancedLoadBalancer => "エンハンスドロードバランサ",
            Self::LocalRouter => "ローカルルータ",
            Self::Gslb => "GSLB",
            Self::Kms => "KMS",
            Self::Iam => "IAM",
            Self::SimpleNotification => "シンプル通知",
            Self::AutoScale => "オートスケール",
            Self::EnhancedDb => "エンハンスドデータベース",
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
        let details = self
            .details
            .iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "{} {} {} {} {} {} {} {}",
            self.name,
            self.description,
            self.tags.join(" "),
            self.resource_type,
            self.status,
            self.plan,
            self.id,
            details,
        )
    }
}

impl SacloudClient {
    pub async fn list_managed_resources(
        &self,
        kind: ManagedResourceKind,
    ) -> Result<Vec<ManagedResource>> {
        match kind {
            ManagedResourceKind::AiEngine => {
                anyhow::bail!("AI Engineには専用のアカウントトークンが必要です")
            }
            ManagedResourceKind::ObjectStorage => self.list_object_storage_buckets().await,
            ManagedResourceKind::SimpleMq => self.list_common_service("simplemq", kind).await,
            ManagedResourceKind::EventBus => self.list_eventbus_resources().await,
            ManagedResourceKind::Workflows => self.list_workflows().await,
            ManagedResourceKind::WebAccel => self.list_webaccel_sites().await,
            ManagedResourceKind::EnhancedLoadBalancer => {
                self.list_common_service("proxylb", kind).await
            }
            ManagedResourceKind::LocalRouter => self.list_common_service("localrouter", kind).await,
            ManagedResourceKind::Gslb => self.list_common_service("gslb", kind).await,
            ManagedResourceKind::Kms => self.list_kms_keys().await,
            ManagedResourceKind::Iam => self.list_iam_resources().await,
            ManagedResourceKind::SimpleNotification => {
                self.list_simple_notification_resources().await
            }
            ManagedResourceKind::AutoScale => self.list_common_service("autoscale", kind).await,
            ManagedResourceKind::EnhancedDb => self.list_common_service("enhanceddb", kind).await,
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

    async fn list_kms_keys(&self) -> Result<Vec<ManagedResource>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let value: Value = self
                .request_with_suffix(
                    KMS_API_ZONE,
                    KMS_SUFFIX,
                    Method::GET,
                    "keys",
                    Some(json!({"From": from, "Count": PAGE_SIZE, "Sort": ["Name"]})),
                )
                .await?;
            let total = value.get("Total").and_then(value_usize).unwrap_or(0);
            let items = first_array(&value, &["/Keys", "/keys"]);
            let received = items.len();
            out.extend(
                items
                    .iter()
                    .map(parse_kms_key)
                    .collect::<Result<Vec<_>>>()?,
            );
            if received == 0 || from + received >= total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    async fn list_simple_notification_resources(&self) -> Result<Vec<ManagedResource>> {
        let mut out = Vec::new();
        for class in ["saknoticedestination", "saknoticegroup", "saknoticerouting"] {
            out.extend(
                self.list_common_service(class, ManagedResourceKind::SimpleNotification)
                    .await?,
            );
        }
        out.sort_by(|a, b| {
            a.resource_type
                .cmp(&b.resource_type)
                .then(a.name.cmp(&b.name))
        });
        Ok(out)
    }

    async fn list_iam_resources(&self) -> Result<Vec<ManagedResource>> {
        let mut out = Vec::new();
        for (path, resource_type) in [
            ("compat/users", "ユーザー"),
            ("groups", "グループ"),
            ("projects", "プロジェクト"),
            ("iam-roles", "ロール"),
            ("service-principals", "サービスプリンシパル"),
        ] {
            out.extend(self.list_iam_resource_type(path, resource_type).await?);
        }
        out.sort_by(|a, b| {
            a.resource_type
                .cmp(&b.resource_type)
                .then(a.name.cmp(&b.name))
        });
        Ok(out)
    }

    async fn list_iam_resource_type(
        &self,
        path: &str,
        resource_type: &str,
    ) -> Result<Vec<ManagedResource>> {
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            let value: Value = self
                .request_global_with_query(
                    IAM_SUFFIX,
                    path,
                    &[
                        ("page", page.to_string()),
                        ("per_page", PAGE_SIZE.to_string()),
                    ],
                )
                .await
                .with_context(|| format!("IAMの{resource_type}一覧を取得できませんでした"))?;
            let total = value.get("count").and_then(value_usize).unwrap_or(0);
            let items = first_array(&value, &["/items", "/Items"]);
            let received = items.len();
            out.extend(
                items
                    .iter()
                    .map(|item| parse_iam_resource(item, resource_type))
                    .collect::<Result<Vec<_>>>()?,
            );
            if received == 0 || out.len() >= total {
                break;
            }
        }
        Ok(out)
    }

    pub async fn create_iam_user(
        &self,
        name: &str,
        code: &str,
        password: &str,
        description: &str,
        email: &str,
    ) -> Result<()> {
        let mut body = json!({
            "name": name, "code": code, "password": password, "description": description
        });
        if !email.is_empty() {
            body["email"] = json!(email);
        }
        self.iam_write(Method::POST, "compat/users", Some(body))
            .await
    }

    pub async fn update_iam_user(
        &self,
        id: &str,
        name: &str,
        password: Option<&str>,
        description: &str,
    ) -> Result<()> {
        let mut body = json!({"name": name, "description": description});
        if let Some(password) = password.filter(|value| !value.is_empty()) {
            body["password"] = json!(password);
        }
        self.iam_write(Method::PUT, &format!("compat/users/{id}"), Some(body))
            .await
    }

    pub async fn create_iam_project(
        &self,
        name: &str,
        code: &str,
        description: &str,
        parent_folder_id: Option<i64>,
    ) -> Result<()> {
        let mut body = json!({"name": name, "code": code, "description": description});
        if let Some(parent_folder_id) = parent_folder_id {
            body["parent_folder_id"] = json!(parent_folder_id);
        }
        self.iam_write(Method::POST, "projects", Some(body)).await
    }

    pub async fn create_iam_group(&self, name: &str, description: &str) -> Result<()> {
        self.iam_write(
            Method::POST,
            "groups",
            Some(json!({"name": name, "description": description})),
        )
        .await
    }

    pub async fn update_iam_group(&self, id: &str, name: &str, description: &str) -> Result<()> {
        self.iam_write(
            Method::PUT,
            &format!("groups/{id}"),
            Some(json!({"name": name, "description": description})),
        )
        .await
    }

    pub async fn update_iam_project(&self, id: &str, name: &str, description: &str) -> Result<()> {
        self.iam_write(
            Method::PUT,
            &format!("projects/{id}"),
            Some(json!({"name": name, "description": description})),
        )
        .await
    }

    pub async fn create_iam_service_principal(
        &self,
        project_id: i64,
        name: &str,
        description: &str,
    ) -> Result<()> {
        self.iam_write(
            Method::POST,
            "service-principals",
            Some(json!({
                "project_id": project_id, "name": name, "description": description
            })),
        )
        .await
    }

    pub async fn update_iam_service_principal(
        &self,
        id: &str,
        name: &str,
        description: &str,
    ) -> Result<()> {
        self.iam_write(
            Method::PUT,
            &format!("service-principals/{id}"),
            Some(json!({"name": name, "description": description})),
        )
        .await
    }

    pub async fn delete_iam_resource(&self, resource_type: &str, id: &str) -> Result<()> {
        let path = match resource_type {
            "ユーザー" => format!("compat/users/{id}"),
            "グループ" => format!("groups/{id}"),
            "プロジェクト" => format!("projects/{id}"),
            "サービスプリンシパル" => format!("service-principals/{id}"),
            _ => anyhow::bail!("{resource_type}は削除できません"),
        };
        self.iam_write(Method::DELETE, &path, None).await
    }

    pub async fn change_project_iam_role(
        &self,
        project_id: i64,
        role_id: &str,
        principal_type: &str,
        principal_id: i64,
        grant: bool,
    ) -> Result<()> {
        anyhow::ensure!(
            matches!(principal_type, "user" | "group" | "service-principal"),
            "プリンシパル種別が不正です"
        );
        let path = format!("projects/{project_id}/iam-policy");
        let mut policy: Value = self
            .request_global(Method::GET, IAM_SUFFIX, &path, &[], None)
            .await?;
        change_policy_binding(&mut policy, role_id, principal_type, principal_id, grant)?;
        self.iam_write(Method::PUT, &path, Some(policy)).await
    }

    async fn iam_write(&self, method: Method, path: &str, body: Option<Value>) -> Result<()> {
        let _: Value = self
            .request_global(method, IAM_SUFFIX, path, &[], body)
            .await?;
        Ok(())
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

    async fn list_webaccel_sites(&self) -> Result<Vec<ManagedResource>> {
        let value: Value = self
            .request_with_suffix(
                WEBACCEL_API_ZONE,
                WEBACCEL_SUFFIX,
                Method::GET,
                "site",
                None,
            )
            .await?;
        value
            .get("Sites")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(parse_webaccel_site)
            .collect()
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
        "autoscale" => "スケール設定",
        "enhanceddb" => "データベース",
        "proxylb" => "ロードバランサ",
        "localrouter" => "ルータ",
        "gslb" => "GSLB",
        "saknoticedestination" => "通知先",
        "saknoticegroup" => "通知先グループ",
        "saknoticerouting" => "ルーティング",
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
    match kind {
        ManagedResourceKind::SimpleMq => {
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
        }
        ManagedResourceKind::AutoScale => {
            add_detail(
                &mut details,
                "トリガー",
                string_at(value, "/Settings/TriggerType"),
            );
            add_detail(
                &mut details,
                "対象ゾーン",
                string_array_at(value, "/Settings/SakuraCloudZones").join(", "),
            );
            add_detail(
                &mut details,
                "停止中",
                string_at(value, "/Settings/Disabled"),
            );
            add_detail(
                &mut details,
                "登録元",
                string_at(value, "/Status/RegisteredBy"),
            );
        }
        ManagedResourceKind::EnhancedDb => {
            add_detail(
                &mut details,
                "データベース名",
                string_at(value, "/Status/database_name"),
            );
            add_detail(
                &mut details,
                "データベース種別",
                string_at(value, "/Status/database_type"),
            );
            add_detail(
                &mut details,
                "リージョン",
                string_at(value, "/Status/region"),
            );
            add_detail(&mut details, "ホスト", string_at(value, "/Status/hostname"));
            add_detail(&mut details, "ポート", string_at(value, "/Status/port"));
        }
        ManagedResourceKind::EnhancedLoadBalancer => {
            add_detail(
                &mut details,
                "プラン",
                string_at(value, "/Settings/ProxyLB/Plan"),
            );
            add_detail(&mut details, "FQDN", string_at(value, "/Status/FQDN"));
            add_detail(
                &mut details,
                "仮想IPアドレス",
                string_at(value, "/Status/VirtualIPAddress"),
            );
            add_detail(
                &mut details,
                "リージョン",
                string_at(value, "/Status/Region"),
            );
            add_array_count(
                &mut details,
                "待受ポート数",
                value,
                "/Settings/ProxyLB/BindPorts",
            );
            add_array_count(
                &mut details,
                "実サーバ数",
                value,
                "/Settings/ProxyLB/Servers",
            );
            add_array_count(&mut details, "ルール数", value, "/Settings/ProxyLB/Rules");
        }
        ManagedResourceKind::LocalRouter => {
            add_detail(
                &mut details,
                "接続スイッチ",
                first_non_empty(
                    value,
                    &[
                        "/Settings/LocalRouter/Switch/Name",
                        "/Settings/LocalRouter/Switch/Code",
                    ],
                ),
            );
            add_detail(
                &mut details,
                "仮想IPアドレス",
                string_at(value, "/Settings/LocalRouter/Interface/VirtualIPAddress"),
            );
            add_detail(
                &mut details,
                "ネットワーク",
                string_at(value, "/Settings/LocalRouter/Interface/NetworkMaskLen"),
            );
            add_array_count(&mut details, "ピア数", value, "/Settings/LocalRouter/Peers");
            add_array_count(
                &mut details,
                "スタティックルート数",
                value,
                "/Settings/LocalRouter/StaticRoutes",
            );
        }
        ManagedResourceKind::Gslb => {
            add_detail(
                &mut details,
                "FQDN",
                first_non_empty(value, &["/Status/FQDN", "/Status/Hostname"]),
            );
            add_detail(
                &mut details,
                "監視方法",
                first_non_empty(
                    value,
                    &[
                        "/Settings/GSLB/HealthCheck/Protocol",
                        "/Settings/HealthCheck/Protocol",
                    ],
                ),
            );
            add_detail(
                &mut details,
                "ポート",
                first_non_empty(
                    value,
                    &[
                        "/Settings/GSLB/HealthCheck/Port",
                        "/Settings/HealthCheck/Port",
                    ],
                ),
            );
            add_first_array_count(
                &mut details,
                "実サーバ数",
                value,
                &["/Settings/GSLB/Servers", "/Settings/Servers"],
            );
        }
        ManagedResourceKind::SimpleNotification => match class.as_str() {
            "saknoticedestination" => {
                add_detail(&mut details, "通知方法", string_at(value, "/Settings/Type"));
                add_detail(&mut details, "通知先", string_at(value, "/Settings/Value"));
                add_detail(&mut details, "無効", string_at(value, "/Settings/Disabled"));
                add_detail(
                    &mut details,
                    "確認状態",
                    first_non_empty(value, &["/Status/Verified", "/Status/Status"]),
                );
            }
            "saknoticegroup" => {
                add_array_count(&mut details, "通知先数", value, "/Settings/Destinations");
                add_detail(&mut details, "無効", string_at(value, "/Settings/Disabled"));
            }
            "saknoticerouting" => {
                add_detail(
                    &mut details,
                    "通知元ID",
                    string_at(value, "/Settings/SourceID"),
                );
                add_detail(
                    &mut details,
                    "通知先グループID",
                    string_at(value, "/Settings/TargetGroupID"),
                );
                add_detail(
                    &mut details,
                    "優先順位",
                    string_at(value, "/Settings/PriorityRank"),
                );
                add_array_count(&mut details, "ラベル条件数", value, "/Settings/MatchLabels");
            }
            _ => {}
        },
        _ => {
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

fn parse_kms_key(value: &Value) -> Result<ManagedResource> {
    let id = first_non_empty(
        value,
        &["/resource_id", "/resourceId", "/ResourceID", "/id", "/ID"],
    );
    anyhow::ensure!(!id.is_empty(), "KMSキーのリソースIDがありません");
    let name = first_non_empty(value, &["/name", "/Name"]);
    let description = first_non_empty(value, &["/description", "/Description"]);
    let status = first_non_empty(value, &["/status", "/Status"]);
    let service_class = first_non_empty(value, &["/ServiceClass", "/service_class"]);
    let key_origin = first_non_empty(value, &["/KeyOrigin", "/key_origin"]);
    let created_at = first_non_empty(value, &["/created_at", "/createdAt", "/CreatedAt"]);
    let tags = string_array_at(value, "/Tags");
    let mut details = Vec::new();
    add_detail(&mut details, "リソースID", id.clone());
    add_detail(&mut details, "状態", status.clone());
    add_detail(&mut details, "サービスクラス", service_class.clone());
    add_detail(&mut details, "鍵の由来", key_origin.clone());
    add_detail(
        &mut details,
        "最新バージョン",
        first_non_empty(value, &["/LatestVersion", "/latest_version"]),
    );
    add_detail(
        &mut details,
        "削除予定日時",
        first_non_empty(
            value,
            &["/destruction_scheduled_at", "/destructionScheduledAt"],
        ),
    );
    add_detail(&mut details, "作成日時", created_at.clone());
    add_detail(
        &mut details,
        "更新日時",
        first_non_empty(value, &["/ModifiedAt", "/modified_at"]),
    );
    add_detail(&mut details, "タグ", tags.join(", "));
    Ok(ManagedResource {
        id,
        name,
        description,
        tags,
        resource_type: key_origin,
        status,
        plan: service_class,
        created_at,
        details,
    })
}

fn parse_iam_resource(value: &Value, resource_type: &str) -> Result<ManagedResource> {
    let id = first_non_empty(value, &["/id", "/ID"]);
    anyhow::ensure!(!id.is_empty(), "IAMの{resource_type} IDがありません");
    let name = first_non_empty(value, &["/name", "/code", "/id"]);
    let description = string_at(value, "/description");
    let status = string_at(value, "/status");
    let plan = match resource_type {
        "ユーザー" | "プロジェクト" => string_at(value, "/code"),
        "ロール" => string_at(value, "/category"),
        "サービスプリンシパル" => string_at(value, "/project_id"),
        _ => String::new(),
    };
    let created_at = string_at(value, "/created_at");
    let mut details = Vec::new();
    add_detail(&mut details, "ID", id.clone());
    add_detail(&mut details, "種別", resource_type.to_string());
    add_detail(&mut details, "状態", status.clone());
    match resource_type {
        "ユーザー" => {
            add_detail(&mut details, "ユーザーコード", string_at(value, "/code"));
            add_detail(&mut details, "メールアドレス", string_at(value, "/email"));
            add_detail(&mut details, "会員ID", string_at(value, "/member/code"));
            add_detail(&mut details, "OTP", string_at(value, "/otp/status"));
            add_detail(
                &mut details,
                "セキュリティキー登録済み",
                string_at(value, "/is_security_key_registered"),
            );
            add_detail(
                &mut details,
                "パスワードレス",
                string_at(value, "/is_passwordless"),
            );
        }
        "プロジェクト" => {
            add_detail(
                &mut details,
                "プロジェクトコード",
                string_at(value, "/code"),
            );
            add_detail(
                &mut details,
                "親フォルダID",
                string_at(value, "/parent_folder_id"),
            );
        }
        "ロール" => {
            add_detail(&mut details, "カテゴリ", string_at(value, "/category"));
            add_detail(
                &mut details,
                "付与可能な最低階層",
                string_at(value, "/lowest_grantable_resource"),
            );
        }
        "サービスプリンシパル" => {
            add_detail(
                &mut details,
                "プロジェクトID",
                string_at(value, "/project_id"),
            );
        }
        _ => {}
    }
    add_detail(&mut details, "作成日時", created_at.clone());
    add_detail(&mut details, "更新日時", string_at(value, "/updated_at"));
    Ok(ManagedResource {
        id,
        name,
        description,
        tags: Vec::new(),
        resource_type: resource_type.to_string(),
        status,
        plan,
        created_at,
        details,
    })
}

fn change_policy_binding(
    policy: &mut Value,
    role_id: &str,
    principal_type: &str,
    principal_id: i64,
    grant: bool,
) -> Result<()> {
    let bindings = policy
        .get_mut("bindings")
        .and_then(Value::as_array_mut)
        .context("IAMポリシーにbindingsがありません")?;
    let target = json!({"type": principal_type, "id": principal_id});
    if grant {
        if let Some(binding) = bindings
            .iter_mut()
            .find(|binding| string_at(binding, "/role/id") == role_id)
        {
            let principals = binding
                .get_mut("principals")
                .and_then(Value::as_array_mut)
                .context("IAMポリシーのprincipalsが不正です")?;
            if !principals.iter().any(|principal| principal == &target) {
                principals.push(target);
            }
        } else {
            bindings.push(json!({
                "role": {"type": "preset", "id": role_id},
                "principals": [target]
            }));
        }
    } else {
        for binding in bindings.iter_mut() {
            if string_at(binding, "/role/id") == role_id
                && let Some(principals) =
                    binding.get_mut("principals").and_then(Value::as_array_mut)
            {
                principals.retain(|principal| principal != &target);
            }
        }
        bindings.retain(|binding| {
            binding
                .get("principals")
                .and_then(Value::as_array)
                .is_some_and(|principals| !principals.is_empty())
        });
    }
    Ok(())
}

fn parse_webaccel_site(value: &Value) -> Result<ManagedResource> {
    let id = first_non_empty(value, &["/ID", "/Name"]);
    anyhow::ensure!(!id.is_empty(), "ウェブアクセラレータのサイトIDがありません");
    let status = string_at(value, "/Status");
    let origin_type = match string_at(value, "/OriginType").as_str() {
        "0" => "ウェブサーバー",
        "1" => "オブジェクトストレージ",
        _ => "不明",
    }
    .to_string();
    let domain = first_non_empty(value, &["/Domain", "/Subdomain"]);
    let mut details = Vec::new();
    add_detail(&mut details, "ID", id.clone());
    add_detail(&mut details, "状態", status.clone());
    add_detail(&mut details, "ドメイン", domain);
    add_detail(&mut details, "オリジン種別", origin_type.clone());
    add_detail(&mut details, "オリジン", string_at(value, "/Origin"));
    add_detail(
        &mut details,
        "オリジンプロトコル",
        string_at(value, "/OriginProtocol"),
    );
    add_detail(
        &mut details,
        "キャッシュTTL(秒)",
        string_at(value, "/DefaultCacheTTL"),
    );
    add_detail(
        &mut details,
        "証明書",
        if value
            .get("HasCertificate")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "設定済み"
        } else {
            "未設定"
        }
        .to_string(),
    );
    add_detail(&mut details, "作成日時", string_at(value, "/CreatedAt"));
    Ok(ManagedResource {
        id,
        name: string_at(value, "/Name"),
        description: String::new(),
        tags: Vec::new(),
        resource_type: origin_type,
        status,
        plan: string_at(value, "/RequestProtocol"),
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

fn add_array_count(details: &mut Vec<(String, String)>, label: &str, value: &Value, pointer: &str) {
    if let Some(count) = value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::len)
    {
        add_detail(details, label, count.to_string());
    }
}

fn add_first_array_count(
    details: &mut Vec<(String, String)>,
    label: &str,
    value: &Value,
    pointers: &[&str],
) {
    if let Some(count) = pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer)?.as_array().map(Vec::len))
    {
        add_detail(details, label, count.to_string());
    }
}

fn first_array(value: &Value, pointers: &[&str]) -> Vec<Value> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer)?.as_array().cloned())
        .unwrap_or_default()
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

    #[test]
    fn parses_webaccel_site_details() {
        let value = json!({
            "ID": "000000000001", "Name": "docs", "Status": "enabled",
            "Domain": "docs.example.com", "OriginType": "0",
            "Origin": "origin.example.com", "OriginProtocol": "https",
            "DefaultCacheTTL": 3600, "HasCertificate": true
        });
        let item = parse_webaccel_site(&value).unwrap();
        assert_eq!(item.resource_type, "ウェブサーバー");
        assert_eq!(item.status, "enabled");
        assert!(
            item.details
                .iter()
                .any(|(label, value)| { label == "証明書" && value == "設定済み" })
        );
    }

    #[test]
    fn parses_autoscale_and_enhanced_db_details() {
        let autoscale = json!({
            "ID": "1", "Name": "scale-web", "Availability": "available",
            "Provider": {"Class": "autoscale"},
            "Settings": {"TriggerType": "cpu", "SakuraCloudZones": ["is1b"], "Disabled": false}
        });
        let item = parse_common_service(&autoscale, ManagedResourceKind::AutoScale).unwrap();
        assert!(
            item.details
                .iter()
                .any(|(label, value)| { label == "対象ゾーン" && value == "is1b" })
        );

        let database = json!({
            "ID": "2", "Name": "app-db", "Availability": "available",
            "Provider": {"Class": "enhanceddb"},
            "Status": {"database_name": "app", "database_type": "mariadb", "hostname": "db.example", "port": 3306}
        });
        let item = parse_common_service(&database, ManagedResourceKind::EnhancedDb).unwrap();
        assert!(
            item.details
                .iter()
                .any(|(label, value)| { label == "ホスト" && value == "db.example" })
        );
    }

    #[test]
    fn parses_enhanced_load_balancer_details() {
        let value = json!({
            "ID": "10", "Name": "public-lb", "Availability": "available",
            "Provider": {"Class": "proxylb", "ServiceClass": "cloud/proxylb/100"},
            "Settings": {"ProxyLB": {
                "Plan": 100,
                "BindPorts": [{"ProxyMode": "http"}, {"ProxyMode": "https"}],
                "Servers": [{"IPAddress": "192.0.2.10"}],
                "Rules": [{"Action": "forward"}]
            }},
            "Status": {"FQDN": "example.sakura.ne.jp", "VirtualIPAddress": "198.51.100.10", "Region": "is1"}
        });
        let item = parse_common_service(&value, ManagedResourceKind::EnhancedLoadBalancer).unwrap();
        assert_eq!(item.resource_type, "ロードバランサ");
        assert!(
            item.details
                .iter()
                .any(|v| v == &("実サーバ数".into(), "1".into()))
        );
        assert!(item.searchable().contains("198.51.100.10"));
    }

    #[test]
    fn parses_local_router_and_nested_gslb_details() {
        let router = json!({
            "ID": "20", "Name": "inter-zone", "Provider": {"Class": "localrouter"},
            "Settings": {"LocalRouter": {
                "Switch": {"Name": "private"},
                "Interface": {"VirtualIPAddress": "192.0.2.1", "NetworkMaskLen": 24},
                "Peers": [{"SecretKey": "hidden"}],
                "StaticRoutes": [{"Prefix": "10.0.0.0/8"}]
            }}
        });
        let item = parse_common_service(&router, ManagedResourceKind::LocalRouter).unwrap();
        assert!(
            item.details
                .iter()
                .any(|v| v == &("接続スイッチ".into(), "private".into()))
        );
        assert!(
            item.details
                .iter()
                .any(|v| v == &("ピア数".into(), "1".into()))
        );

        let gslb = json!({
            "ID": "30", "Name": "global", "Provider": {"Class": "gslb"},
            "Status": {"FQDN": "global.gslb.example"},
            "Settings": {"GSLB": {
                "HealthCheck": {"Protocol": "https", "Port": 443},
                "Servers": [{"IPAddress": "192.0.2.10"}, {"IPAddress": "192.0.2.11"}]
            }}
        });
        let item = parse_common_service(&gslb, ManagedResourceKind::Gslb).unwrap();
        assert!(
            item.details
                .iter()
                .any(|v| v == &("監視方法".into(), "https".into()))
        );
        assert!(
            item.details
                .iter()
                .any(|v| v == &("実サーバ数".into(), "2".into()))
        );
    }

    #[test]
    fn parses_simple_notification_resources() {
        let destination = json!({
            "ID": "dest-1", "Name": "operations", "Provider": {"Class": "saknoticedestination"},
            "Settings": {"Type": "email", "Value": "ops@example.com", "Disabled": false},
            "Status": {"Verified": true}
        });
        let item =
            parse_common_service(&destination, ManagedResourceKind::SimpleNotification).unwrap();
        assert_eq!(item.resource_type, "通知先");
        assert!(item.searchable().contains("ops@example.com"));

        let routing = json!({
            "ID": "route-1", "Name": "critical", "Provider": {"Class": "saknoticerouting"},
            "Settings": {"SourceID": "source-1", "TargetGroupID": "group-1", "PriorityRank": 10,
                "MatchLabels": [{"Key": "severity", "Value": "critical"}]}
        });
        let item = parse_common_service(&routing, ManagedResourceKind::SimpleNotification).unwrap();
        assert_eq!(item.resource_type, "ルーティング");
        assert!(item.searchable().contains("group-1"));
    }

    #[test]
    fn parses_kms_key_details() {
        let value = json!({
            "ID": "key-1", "Name": "database", "Description": "database encryption",
            "ServiceClass": "cloud/kms/standard", "KeyOrigin": "sakura",
            "LatestVersion": 3, "Status": "available", "Tags": ["prod"],
            "CreatedAt": "2026-08-01T00:00:00Z", "ModifiedAt": "2026-08-02T00:00:00Z"
        });
        let item = parse_kms_key(&value).unwrap();
        assert_eq!(item.id, "key-1");
        assert_eq!(item.plan, "cloud/kms/standard");
        assert_eq!(item.status, "available");
        assert!(
            item.details
                .iter()
                .any(|v| v == &("最新バージョン".into(), "3".into()))
        );
        assert!(item.searchable().contains("prod"));
    }

    #[test]
    fn parses_iam_users_and_projects_without_secret_fields() {
        let user = json!({
            "id": 101, "name": "Alice", "code": "alice", "status": "available",
            "description": "operator", "email": "alice@example.com",
            "member": {"id": 1, "code": "abc12345"},
            "otp": {"status": "activated", "has_recovery_code": true},
            "is_security_key_registered": true, "is_passwordless": false,
            "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-02T00:00:00Z",
            "password": "must-not-be-shown"
        });
        let item = parse_iam_resource(&user, "ユーザー").unwrap();
        assert_eq!(item.id, "101");
        assert_eq!(item.plan, "alice");
        assert!(item.searchable().contains("alice@example.com"));
        assert!(!item.searchable().contains("must-not-be-shown"));

        let project = json!({
            "id": 201, "code": "production", "name": "Production",
            "description": "main project", "status": "available",
            "parent_folder_id": 12, "created_at": "2026-08-01T00:00:00Z"
        });
        let item = parse_iam_resource(&project, "プロジェクト").unwrap();
        assert_eq!(item.name, "Production");
        assert!(
            item.details
                .iter()
                .any(|v| v == &("親フォルダID".into(), "12".into()))
        );
    }

    #[test]
    fn parses_iam_roles_and_service_principals() {
        let group = json!({
            "id": 250, "name": "developers", "description": "development team",
            "created_at": "2026-08-01T00:00:00Z"
        });
        let item = parse_iam_resource(&group, "グループ").unwrap();
        assert_eq!(item.id, "250");
        assert!(item.searchable().contains("development team"));

        let role = json!({
            "id": "sakura-cloud-viewer", "name": "リソース閲覧",
            "description": "リソースを閲覧できます", "category": "sakura-cloud",
            "lowest_grantable_resource": "project"
        });
        let item = parse_iam_resource(&role, "ロール").unwrap();
        assert_eq!(item.id, "sakura-cloud-viewer");
        assert_eq!(item.plan, "sakura-cloud");
        assert!(item.searchable().contains("project"));

        let principal = json!({
            "id": 301, "project_id": 201, "name": "backup-worker",
            "description": "nightly backup", "created_at": "2026-08-01T00:00:00Z"
        });
        let item = parse_iam_resource(&principal, "サービスプリンシパル").unwrap();
        assert_eq!(item.plan, "201");
        assert!(item.searchable().contains("backup-worker"));
    }

    #[test]
    fn iam_policy_grant_is_idempotent_and_preserves_existing_bindings() {
        let mut policy = json!({"bindings": [
            {"role":{"type":"preset","id":"viewer"}, "principals":[{"type":"user","id":1}]},
            {"role":{"type":"preset","id":"admin"}, "principals":[{"type":"group","id":2}]}
        ]});
        change_policy_binding(&mut policy, "viewer", "service-principal", 3, true).unwrap();
        change_policy_binding(&mut policy, "viewer", "service-principal", 3, true).unwrap();
        let bindings = policy["bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0]["principals"].as_array().unwrap().len(), 2);
        assert_eq!(bindings[1]["role"]["id"], "admin");
    }

    #[test]
    fn iam_policy_revoke_removes_only_the_target_principal() {
        let mut policy = json!({"bindings": [
            {"role":{"type":"preset","id":"viewer"}, "principals":[
                {"type":"user","id":1}, {"type":"user","id":2}
            ]},
            {"role":{"type":"preset","id":"admin"}, "principals":[{"type":"user","id":1}]}
        ]});
        change_policy_binding(&mut policy, "viewer", "user", 1, false).unwrap();
        let bindings = policy["bindings"].as_array().unwrap();
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0]["principals"], json!([{"type":"user","id":2}]));
        assert_eq!(bindings[1]["principals"], json!([{"type":"user","id":1}]));
    }
}
