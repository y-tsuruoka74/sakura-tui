//! さくらのクラウド モニタリングスイート API。
//!
//! IaaS とはパスの接尾辞が違う（`api/monitoring/1.0`）ため、専用のクライアントを持つ。
//! アラートプロジェクトを頂点に、ルールと発報履歴がぶら下がる。

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::config::ApiCredentials;
use crate::sacloud::{flexible_number, null_as_default};

const API_SUFFIX: &str = "api/monitoring/1.0";
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が実態と違う総件数を返しても止まるようにする。
const MAX_PAGES: usize = 100;

/// アラートプロジェクト 1 件。
#[derive(Debug, Clone)]
pub struct AlertProject {
    /// 一覧表示用の ID。
    pub id: i64,
    /// パス（`/alerts/projects/{project_resource_id}/...`）に使う ID。
    pub resource_id: i64,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub created_at: Option<String>,
}

/// アラートルール 1 件。
#[derive(Debug, Clone)]
pub struct AlertRule {
    pub uid: String,
    pub metrics_storage_id: i64,
    pub name: String,
    pub query: String,
    /// 発報中かどうか。
    pub open: bool,
    pub warning_enabled: bool,
    pub critical_enabled: bool,
    pub threshold_warning: String,
    pub threshold_critical: String,
    pub duration_warning: i64,
    pub duration_critical: i64,
}

#[derive(Debug, Clone)]
pub struct LogMeasureRule {
    pub uid: String,
    pub name: String,
    pub description: String,
    pub log_storage_id: i64,
    pub metrics_storage_id: i64,
    pub rule: serde_json::Value,
}

/// クラウドリソースからログストレージへの転送設定。
#[derive(Debug, Clone)]
pub struct LogRouting {
    pub uid: String,
    pub publisher_code: String,
    pub publisher_description: String,
    pub variant: String,
    pub resource_id: Option<i64>,
    pub log_storage_id: i64,
}

/// アラートの発報履歴 1 件。
#[derive(Debug, Clone)]
pub struct AlertHistory {
    pub rule_uid: String,
    pub severity: String,
    pub starts_at: String,
    /// まだ復旧していないか。
    pub open: bool,
    pub value: Option<f64>,
    pub threshold: String,
    pub labels: String,
}

#[derive(Debug, Clone)]
pub struct NotificationTarget {
    pub uid: String,
    pub service_type: String,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct NotificationRouting {
    pub uid: String,
    pub target_uid: String,
    pub target_service_type: String,
    pub target_description: String,
    pub match_labels: Vec<(String, String)>,
    pub resend_interval_minutes: Option<i64>,
    pub order: i64,
}

/// ログ・メトリクス・トレースの保管先。
#[derive(Debug, Clone)]
pub struct Storage {
    pub kind: StorageKind,
    pub id: i64,
    pub resource_id: i64,
    pub name: String,
    pub description: String,
    /// 共用 / 専有。
    pub classification: String,
    /// 保持日数（メトリクスには無い）。
    pub retention_days: Option<i64>,
    pub is_system: bool,
}

impl Storage {
    pub fn supports_access_keys(&self) -> bool {
        !self.is_system
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageKind {
    Logs,
    Metrics,
    Traces,
}

/// ストレージへの書き込みに使うアクセスキー。シークレットは一覧では保持しない。
#[derive(Debug, Clone)]
pub struct StorageAccessKey {
    pub uid: String,
    pub id: i64,
    pub token: String,
    pub description: String,
}

/// 作成直後または明示取得時だけ扱うアクセスキーの秘密情報。
#[derive(Clone)]
pub struct StorageAccessKeySecret {
    pub uid: String,
    pub token: String,
    pub secret: String,
}

impl std::fmt::Debug for StorageAccessKeySecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageAccessKeySecret")
            .field("uid", &self.uid)
            .field("token", &"<redacted>")
            .field("secret", &"<redacted>")
            .finish()
    }
}

impl StorageKind {
    pub fn label(self) -> &'static str {
        match self {
            StorageKind::Logs => "ログ",
            StorageKind::Metrics => "メトリクス",
            StorageKind::Traces => "トレース",
        }
    }

    fn path(self) -> &'static str {
        match self {
            StorageKind::Logs => "logs/storages/",
            StorageKind::Metrics => "metrics/storages/",
            StorageKind::Traces => "traces/storages/",
        }
    }
}

// --- API のレスポンス形状 ---

/// この API はどの一覧も `results` に入れて返す。
#[derive(Debug, Deserialize)]
struct Paginated<T> {
    results: Option<Vec<T>>,
    #[serde(default, deserialize_with = "flexible_number")]
    total: usize,
}

impl<T> Paginated<T> {
    fn items(self) -> Vec<T> {
        self.results.unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
struct RawProject {
    #[serde(default, deserialize_with = "flexible_number")]
    id: i64,
    /// パスに使う ID。無ければ `id` で代用する。
    #[serde(default, deserialize_with = "flexible_number")]
    resource_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRule {
    #[serde(default, deserialize_with = "null_as_default")]
    uid: String,
    #[serde(default, deserialize_with = "flexible_number")]
    metrics_storage_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    query: String,
    #[serde(default)]
    open: bool,
    #[serde(default)]
    enabled_warning: bool,
    #[serde(default)]
    enabled_critical: bool,
    #[serde(default, deserialize_with = "null_as_default")]
    threshold_warning: String,
    #[serde(default, deserialize_with = "null_as_default")]
    threshold_critical: String,
    #[serde(default, deserialize_with = "flexible_number")]
    threshold_duration_warning: i64,
    #[serde(default, deserialize_with = "flexible_number")]
    threshold_duration_critical: i64,
}

#[derive(Debug, Deserialize)]
struct RawStorageRef {
    #[serde(default, deserialize_with = "flexible_number")]
    resource_id: i64,
    #[serde(default, deserialize_with = "flexible_number")]
    id: i64,
}

#[derive(Debug, Deserialize)]
struct RawLogMeasureRule {
    #[serde(default, deserialize_with = "null_as_default")]
    uid: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(default)]
    log_storage: Option<RawStorageRef>,
    #[serde(default)]
    metrics_storage: Option<RawStorageRef>,
    #[serde(default)]
    rule: serde_json::Value,
}

#[derive(Debug, Default, Deserialize)]
struct RawPublisher {
    #[serde(default, deserialize_with = "null_as_default")]
    code: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
}

#[derive(Debug, Deserialize)]
struct RawLogRouting {
    #[serde(default, deserialize_with = "null_as_default")]
    uid: String,
    #[serde(default)]
    publisher: RawPublisher,
    #[serde(default, deserialize_with = "null_as_default")]
    variant: String,
    #[serde(default)]
    resource_id: serde_json::Value,
    #[serde(default)]
    log_storage: Option<RawStorageRef>,
}

#[derive(Debug, Deserialize)]
struct RawHistory {
    #[serde(default, deserialize_with = "null_as_default")]
    rule_uid: String,
    #[serde(default, deserialize_with = "null_as_default")]
    severity: String,
    #[serde(rename = "startsAt", default, deserialize_with = "null_as_default")]
    starts_at: String,
    #[serde(default)]
    open: bool,
    #[serde(default, deserialize_with = "flexible_float")]
    value: Option<f64>,
    #[serde(default, deserialize_with = "null_as_default")]
    threshold: String,
    #[serde(default, deserialize_with = "null_as_default")]
    labels: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawNotificationTarget {
    #[serde(default, deserialize_with = "null_as_default")]
    uid: String,
    #[serde(default, deserialize_with = "null_as_default")]
    service_type: String,
    #[serde(default, deserialize_with = "null_as_default")]
    url: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
}

#[derive(Debug, Deserialize)]
struct RawMatchLabel {
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    value: String,
}

#[derive(Debug, Deserialize)]
struct RawNotificationRouting {
    #[serde(default, deserialize_with = "null_as_default")]
    uid: String,
    #[serde(default)]
    notification_target: RawNotificationTarget,
    #[serde(default, deserialize_with = "null_as_default")]
    match_labels: Vec<RawMatchLabel>,
    #[serde(default, deserialize_with = "flexible_number")]
    resend_interval_minutes: i64,
    #[serde(default, deserialize_with = "flexible_number")]
    order: i64,
}

#[derive(Debug, Deserialize)]
struct RawStorage {
    #[serde(default, deserialize_with = "flexible_number")]
    id: i64,
    #[serde(default, deserialize_with = "flexible_number")]
    resource_id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    classification: String,
    /// ログは `expire_day`、トレースは `retention_period_days`。
    #[serde(default, deserialize_with = "flexible_number")]
    expire_day: i64,
    #[serde(default, deserialize_with = "flexible_number")]
    retention_period_days: i64,
    #[serde(default)]
    is_system: bool,
}

#[derive(Debug, Deserialize)]
struct RawStorageAccessKey {
    #[serde(default, deserialize_with = "flexible_number")]
    id: i64,
    #[serde(default, deserialize_with = "null_as_default")]
    uid: String,
    #[serde(default, deserialize_with = "null_as_default")]
    token: String,
    #[serde(default, deserialize_with = "null_as_default")]
    secret: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
}

#[derive(Debug, Default, Deserialize)]
struct ApiError {
    #[serde(default, deserialize_with = "null_as_default")]
    detail: String,
    #[serde(default, deserialize_with = "null_as_default")]
    message: String,
    #[serde(default, deserialize_with = "null_as_default")]
    error_msg: String,
}

#[derive(Debug)]
pub struct MonitoringClient {
    http: reqwest::Client,
    token: String,
    secret: String,
    api_root: String,
}

impl MonitoringClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        let http = crate::http::client()?;
        Ok(Self {
            http,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
            api_root: creds.api_root().to_string(),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        zone: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let url = format!("{}/{zone}/{API_SUFFIX}/{path}", self.api_root);
        let res = crate::http::send_with_retry(&self.http, || {
            Ok(self
                .http
                .request(Method::GET, &url)
                .basic_auth(&self.token, Some(&self.secret))
                .query(query)
                .build()?)
        })
        .await
        .context("モニタリングAPIへのリクエストに失敗しました")?;
        let status = res.status();
        let text = res
            .text()
            .await
            .context("モニタリングAPIのレスポンス読み取りに失敗しました")?;

        if !status.is_success() {
            bail!("{}", format_api_error(status, &text));
        }
        let text = if text.trim().is_empty() { "{}" } else { &text };
        serde_json::from_str(text).with_context(|| {
            let head: String = text.chars().take(200).collect();
            format!("モニタリングAPIのレスポンス解析に失敗しました: {head}")
        })
    }

    async fn send<T: DeserializeOwned>(
        &self,
        zone: &str,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}/{zone}/{API_SUFFIX}/{path}", self.api_root);
        let res = crate::http::send_with_retry(&self.http, || {
            let mut request = self
                .http
                .request(method.clone(), &url)
                .basic_auth(&self.token, Some(&self.secret));
            if let Some(body) = &body {
                request = request.json(body);
            }
            Ok(request.build()?)
        })
        .await
        .context("モニタリングAPIへのリクエストに失敗しました")?;
        let status = res.status();
        let text = res
            .text()
            .await
            .context("モニタリングAPIのレスポンス読み取りに失敗しました")?;
        if !status.is_success() {
            bail!("{}", format_api_error(status, &text));
        }
        let text = if text.trim().is_empty() { "{}" } else { &text };
        serde_json::from_str(text).context("モニタリングAPIのレスポンス解析に失敗しました")
    }

    /// `from` / `count` によるページングを辿る。
    async fn collect<T, F, Fut>(&self, mut fetch: F) -> Result<Vec<T>>
    where
        F: FnMut(usize) -> Fut,
        Fut: Future<Output = Result<(Vec<T>, usize)>>,
    {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let (items, total) = fetch(from).await?;
            let received = items.len();
            out.extend(items);
            if received == 0 || out.len() >= total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    fn page_query(from: usize) -> Vec<(&'static str, String)> {
        vec![("from", from.to_string()), ("count", PAGE_SIZE.to_string())]
    }

    pub async fn list_projects(&self, zone: &str) -> Result<Vec<AlertProject>> {
        self.collect(|from| async move {
            let res: Paginated<RawProject> = self
                .get(zone, "alerts/projects/", &Self::page_query(from))
                .await?;
            let total = res.total;
            Ok((
                res.items()
                    .into_iter()
                    .map(|p| AlertProject {
                        // resource_id が空なら id を使う。
                        resource_id: if p.resource_id > 0 {
                            p.resource_id
                        } else {
                            p.id
                        },
                        id: p.id,
                        name: p.name,
                        description: p.description,
                        tags: p.tags,
                        created_at: p.created_at,
                    })
                    .collect(),
                total,
            ))
        })
        .await
    }

    /// 指定ゾーンのアラートプロジェクト件数だけを数える。
    pub async fn count_projects(&self, zone: &str) -> Result<usize> {
        let query = [("from", "0".to_string()), ("count", "1".to_string())];
        let res: Paginated<RawProject> = self.get(zone, "alerts/projects/", &query).await?;
        Ok(res.total)
    }

    pub async fn create_project(&self, zone: &str, name: &str, description: &str) -> Result<()> {
        let _: serde_json::Value = self
            .send(
                zone,
                Method::POST,
                "alerts/projects/",
                Some(json!({ "name": name, "description": description })),
            )
            .await?;
        Ok(())
    }

    pub async fn update_project(
        &self,
        zone: &str,
        resource_id: i64,
        name: &str,
        description: &str,
    ) -> Result<()> {
        let path = format!("alerts/projects/{resource_id}/");
        let _: serde_json::Value = self
            .send(
                zone,
                Method::PATCH,
                &path,
                Some(json!({ "name": name, "description": description })),
            )
            .await?;
        Ok(())
    }

    pub async fn delete_project(&self, zone: &str, resource_id: i64) -> Result<()> {
        let path = format!("alerts/projects/{resource_id}/");
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn list_rules(&self, zone: &str, project: i64) -> Result<Vec<AlertRule>> {
        let path = format!("alerts/projects/{project}/rules/");
        self.collect(|from| {
            let path = path.clone();
            async move {
                let res: Paginated<RawRule> =
                    self.get(zone, &path, &Self::page_query(from)).await?;
                let total = res.total;
                Ok((
                    res.items()
                        .into_iter()
                        .map(|r| AlertRule {
                            uid: r.uid,
                            metrics_storage_id: r.metrics_storage_id,
                            name: r.name,
                            query: r.query,
                            open: r.open,
                            warning_enabled: r.enabled_warning,
                            critical_enabled: r.enabled_critical,
                            threshold_warning: r.threshold_warning,
                            threshold_critical: r.threshold_critical,
                            duration_warning: r.threshold_duration_warning,
                            duration_critical: r.threshold_duration_critical,
                        })
                        .collect(),
                    total,
                ))
            }
        })
        .await
    }

    pub async fn create_rule(
        &self,
        zone: &str,
        project: i64,
        input: &AlertRuleInput,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/rules/");
        let _: serde_json::Value = self
            .send(zone, Method::POST, &path, Some(input.payload()))
            .await?;
        Ok(())
    }

    pub async fn update_rule(
        &self,
        zone: &str,
        project: i64,
        uid: &str,
        input: &AlertRuleInput,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/rules/{uid}/");
        let _: serde_json::Value = self
            .send(zone, Method::PATCH, &path, Some(input.payload()))
            .await?;
        Ok(())
    }

    pub async fn delete_rule(&self, zone: &str, project: i64, uid: &str) -> Result<()> {
        let path = format!("alerts/projects/{project}/rules/{uid}/");
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn list_log_measure_rules(
        &self,
        zone: &str,
        project: i64,
    ) -> Result<Vec<LogMeasureRule>> {
        let path = format!("alerts/projects/{project}/log-measure-rules/");
        self.collect(|from| {
            let path = path.clone();
            async move {
                let res: Paginated<RawLogMeasureRule> =
                    self.get(zone, &path, &Self::page_query(from)).await?;
                let total = res.total;
                Ok((
                    res.items()
                        .into_iter()
                        .map(|rule| LogMeasureRule {
                            uid: rule.uid,
                            name: rule.name,
                            description: rule.description,
                            log_storage_id: storage_ref_id(rule.log_storage),
                            metrics_storage_id: storage_ref_id(rule.metrics_storage),
                            rule: rule.rule,
                        })
                        .collect(),
                    total,
                ))
            }
        })
        .await
    }

    pub async fn create_log_measure_rule(
        &self,
        zone: &str,
        project: i64,
        input: &LogMeasureRuleInput,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/log-measure-rules/");
        let _: serde_json::Value = self
            .send(zone, Method::POST, &path, Some(input.payload()))
            .await?;
        Ok(())
    }

    pub async fn update_log_measure_rule(
        &self,
        zone: &str,
        project: i64,
        uid: &str,
        input: &LogMeasureRuleInput,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/log-measure-rules/{uid}/");
        let _: serde_json::Value = self
            .send(zone, Method::PATCH, &path, Some(input.payload()))
            .await?;
        Ok(())
    }

    pub async fn delete_log_measure_rule(&self, zone: &str, project: i64, uid: &str) -> Result<()> {
        let path = format!("alerts/projects/{project}/log-measure-rules/{uid}/");
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn list_log_routings(&self, zone: &str) -> Result<Vec<LogRouting>> {
        self.collect(|from| async move {
            let res: Paginated<RawLogRouting> = self
                .get(zone, "logs/routings/", &Self::page_query(from))
                .await?;
            let total = res.total;
            Ok((
                res.items()
                    .into_iter()
                    .map(|routing| LogRouting {
                        uid: routing.uid,
                        publisher_code: routing.publisher.code,
                        publisher_description: routing.publisher.description,
                        variant: routing.variant,
                        resource_id: optional_i64(&routing.resource_id),
                        log_storage_id: storage_ref_id(routing.log_storage),
                    })
                    .collect(),
                total,
            ))
        })
        .await
    }

    pub async fn create_log_routing(&self, zone: &str, input: &LogRoutingInput) -> Result<()> {
        let _: serde_json::Value = self
            .send(zone, Method::POST, "logs/routings/", Some(input.payload()))
            .await?;
        Ok(())
    }

    pub async fn update_log_routing(
        &self,
        zone: &str,
        uid: &str,
        input: &LogRoutingInput,
    ) -> Result<()> {
        let path = format!("logs/routings/{uid}/");
        let _: serde_json::Value = self
            .send(zone, Method::PATCH, &path, Some(input.payload()))
            .await?;
        Ok(())
    }

    pub async fn delete_log_routing(&self, zone: &str, uid: &str) -> Result<()> {
        let path = format!("logs/routings/{uid}/");
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn list_histories(&self, zone: &str, project: i64) -> Result<Vec<AlertHistory>> {
        let path = format!("alerts/projects/{project}/histories/");
        self.collect(|from| {
            let path = path.clone();
            async move {
                let res: Paginated<RawHistory> =
                    self.get(zone, &path, &Self::page_query(from)).await?;
                let total = res.total;
                Ok((
                    res.items()
                        .into_iter()
                        .map(|h| AlertHistory {
                            rule_uid: h.rule_uid,
                            severity: h.severity,
                            starts_at: h.starts_at,
                            open: h.open,
                            value: h.value,
                            threshold: h.threshold,
                            labels: h.labels,
                        })
                        .collect(),
                    total,
                ))
            }
        })
        .await
    }

    pub async fn list_notification_targets(
        &self,
        zone: &str,
        project: i64,
    ) -> Result<Vec<NotificationTarget>> {
        let path = format!("alerts/projects/{project}/notification-targets/");
        self.collect(|from| {
            let path = path.clone();
            async move {
                let res: Paginated<RawNotificationTarget> =
                    self.get(zone, &path, &Self::page_query(from)).await?;
                let total = res.total;
                Ok((
                    res.items()
                        .into_iter()
                        .map(|target| NotificationTarget {
                            uid: target.uid,
                            service_type: target.service_type,
                            url: target.url,
                            description: target.description,
                        })
                        .collect(),
                    total,
                ))
            }
        })
        .await
    }

    pub async fn create_notification_target(
        &self,
        zone: &str,
        project: i64,
        service_type: &str,
        url: &str,
        description: &str,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/notification-targets/");
        let mut payload = json!({
            "service_type": service_type,
            "description": description,
        });
        if !url.is_empty() {
            payload["url"] = json!(url);
        }
        let _: serde_json::Value = self.send(zone, Method::POST, &path, Some(payload)).await?;
        Ok(())
    }

    pub async fn update_notification_target(
        &self,
        zone: &str,
        project: i64,
        uid: &str,
        service_type: &str,
        url: &str,
        description: &str,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/notification-targets/{uid}/");
        let mut payload = json!({
            "service_type": service_type,
            "description": description,
        });
        if !url.is_empty() {
            payload["url"] = json!(url);
        }
        let _: serde_json::Value = self.send(zone, Method::PATCH, &path, Some(payload)).await?;
        Ok(())
    }

    pub async fn delete_notification_target(
        &self,
        zone: &str,
        project: i64,
        uid: &str,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/notification-targets/{uid}/");
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn list_notification_routings(
        &self,
        zone: &str,
        project: i64,
    ) -> Result<Vec<NotificationRouting>> {
        let path = format!("alerts/projects/{project}/notification-routings/");
        self.collect(|from| {
            let path = path.clone();
            async move {
                let res: Paginated<RawNotificationRouting> =
                    self.get(zone, &path, &Self::page_query(from)).await?;
                let total = res.total;
                Ok((
                    res.items()
                        .into_iter()
                        .map(|routing| NotificationRouting {
                            uid: routing.uid,
                            target_uid: routing.notification_target.uid,
                            target_service_type: routing.notification_target.service_type,
                            target_description: routing.notification_target.description,
                            match_labels: routing
                                .match_labels
                                .into_iter()
                                .map(|label| (label.name, label.value))
                                .collect(),
                            resend_interval_minutes: (routing.resend_interval_minutes > 0)
                                .then_some(routing.resend_interval_minutes),
                            order: routing.order,
                        })
                        .collect(),
                    total,
                ))
            }
        })
        .await
    }

    pub async fn create_notification_routing(
        &self,
        zone: &str,
        project: i64,
        target_uid: &str,
        match_labels: &[(String, String)],
        resend_interval_minutes: Option<i64>,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/notification-routings/");
        let _: serde_json::Value = self
            .send(
                zone,
                Method::POST,
                &path,
                Some(notification_routing_payload(
                    target_uid,
                    match_labels,
                    resend_interval_minutes,
                )),
            )
            .await?;
        Ok(())
    }

    pub async fn update_notification_routing(
        &self,
        zone: &str,
        project: i64,
        uid: &str,
        target_uid: &str,
        match_labels: &[(String, String)],
        resend_interval_minutes: Option<i64>,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/notification-routings/{uid}/");
        let _: serde_json::Value = self
            .send(
                zone,
                Method::PATCH,
                &path,
                Some(notification_routing_payload(
                    target_uid,
                    match_labels,
                    resend_interval_minutes,
                )),
            )
            .await?;
        Ok(())
    }

    pub async fn delete_notification_routing(
        &self,
        zone: &str,
        project: i64,
        uid: &str,
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/notification-routings/{uid}/");
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn reorder_notification_routings(
        &self,
        zone: &str,
        project: i64,
        orders: &[(String, i64)],
    ) -> Result<()> {
        let path = format!("alerts/projects/{project}/notification-routings/reorder/");
        let payload = orders
            .iter()
            .map(|(uid, order)| {
                json!({
                    "notification_routing_uid": uid,
                    "order": order,
                })
            })
            .collect::<Vec<_>>();
        let _: serde_json::Value = self
            .send(zone, Method::PUT, &path, Some(json!(payload)))
            .await?;
        Ok(())
    }

    /// ログ・メトリクス・トレースの保管先をまとめて取る。
    pub async fn list_storages(&self, zone: &str) -> Result<Vec<Storage>> {
        let mut out = Vec::new();
        for kind in [StorageKind::Logs, StorageKind::Metrics, StorageKind::Traces] {
            let res: Paginated<RawStorage> = self
                .get(zone, kind.path(), &Self::page_query(0))
                .await
                .with_context(|| format!("{}の保管先取得に失敗しました", kind.label()))?;
            out.extend(res.items().into_iter().map(|s| {
                Storage {
                    kind,
                    id: s.id,
                    resource_id: if s.resource_id > 0 {
                        s.resource_id
                    } else {
                        s.id
                    },
                    name: s.name,
                    description: s.description,
                    classification: s.classification,
                    // 0 は「未設定」とみなす（メトリクスには保持期間の概念が無い）。
                    retention_days: [s.expire_day, s.retention_period_days]
                        .into_iter()
                        .find(|d| *d > 0),
                    is_system: s.is_system,
                }
            }));
        }
        Ok(out)
    }

    pub async fn create_storage(
        &self,
        zone: &str,
        kind: StorageKind,
        name: &str,
        description: &str,
        classification: &str,
        is_system: bool,
    ) -> Result<()> {
        let mut payload = json!({ "name": name, "description": description });
        match kind {
            StorageKind::Logs => {
                payload["is_system"] = json!(is_system);
                payload["classification"] = json!(classification);
            }
            StorageKind::Metrics => payload["is_system"] = json!(is_system),
            StorageKind::Traces => payload["classification"] = json!(classification),
        }
        let _: serde_json::Value = self
            .send(zone, Method::POST, kind.path(), Some(payload))
            .await?;
        Ok(())
    }

    pub async fn update_storage(
        &self,
        zone: &str,
        storage: &Storage,
        name: &str,
        description: &str,
    ) -> Result<()> {
        let path = format!("{}{}/", storage.kind.path(), storage.resource_id);
        let payload = json!({ "name": name, "description": description });
        let _: serde_json::Value = self.send(zone, Method::PATCH, &path, Some(payload)).await?;
        Ok(())
    }

    pub async fn delete_storage(&self, zone: &str, storage: &Storage) -> Result<()> {
        let path = format!("{}{}/", storage.kind.path(), storage.resource_id);
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn set_storage_retention(
        &self,
        zone: &str,
        storage: &Storage,
        days: i64,
    ) -> Result<()> {
        if storage.kind == StorageKind::Metrics {
            bail!("メトリクスストレージの保持期間は固定です");
        }
        let path = format!("{}{}/set-expire/", storage.kind.path(), storage.resource_id);
        let _: serde_json::Value = self
            .send(zone, Method::POST, &path, Some(json!({ "days": days })))
            .await?;
        Ok(())
    }

    pub async fn list_storage_access_keys(
        &self,
        zone: &str,
        storage: &Storage,
    ) -> Result<Vec<StorageAccessKey>> {
        if !storage.supports_access_keys() {
            bail!("システム領域のストレージではアクセスキーを利用できません");
        }
        let base = format!("{}{}/keys/", storage.kind.path(), storage.resource_id);
        self.collect(|from| {
            let path = base.clone();
            async move {
                let res: Paginated<RawStorageAccessKey> =
                    self.get(zone, &path, &Self::page_query(from)).await?;
                let total = res.total;
                Ok((
                    res.items()
                        .into_iter()
                        .map(|key| StorageAccessKey {
                            uid: key.uid,
                            id: key.id,
                            token: key.token,
                            description: key.description,
                        })
                        .collect(),
                    total,
                ))
            }
        })
        .await
    }

    pub async fn create_storage_access_key(
        &self,
        zone: &str,
        storage: &Storage,
        description: &str,
    ) -> Result<StorageAccessKeySecret> {
        if !storage.supports_access_keys() {
            bail!("システム領域のストレージではアクセスキーを利用できません");
        }
        let path = format!("{}{}/keys/", storage.kind.path(), storage.resource_id);
        let value: serde_json::Value = self
            .send(
                zone,
                Method::POST,
                &path,
                Some(json!({ "description": description })),
            )
            .await?;
        access_key_secret(value)
    }

    pub async fn update_storage_access_key(
        &self,
        zone: &str,
        storage: &Storage,
        uid: &str,
        description: &str,
    ) -> Result<()> {
        if !storage.supports_access_keys() {
            bail!("システム領域のストレージではアクセスキーを利用できません");
        }
        let path = format!("{}{}/keys/{uid}/", storage.kind.path(), storage.resource_id);
        let _: serde_json::Value = self
            .send(
                zone,
                Method::PUT,
                &path,
                Some(json!({ "description": description })),
            )
            .await?;
        Ok(())
    }

    pub async fn delete_storage_access_key(
        &self,
        zone: &str,
        storage: &Storage,
        uid: &str,
    ) -> Result<()> {
        if !storage.supports_access_keys() {
            bail!("システム領域のストレージではアクセスキーを利用できません");
        }
        let path = format!("{}{}/keys/{uid}/", storage.kind.path(), storage.resource_id);
        let _: serde_json::Value = self.send(zone, Method::DELETE, &path, None).await?;
        Ok(())
    }

    pub async fn read_storage_access_key_secret(
        &self,
        zone: &str,
        storage: &Storage,
        uid: &str,
    ) -> Result<StorageAccessKeySecret> {
        if !storage.supports_access_keys() {
            bail!("システム領域のストレージではアクセスキーを利用できません");
        }
        let path = format!("{}{}/keys/{uid}/", storage.kind.path(), storage.resource_id);
        let value: serde_json::Value = self.get(zone, &path, &[]).await?;
        access_key_secret(value)
    }
}

fn storage_ref_id(storage: Option<RawStorageRef>) -> i64 {
    storage.map_or(0, |storage| {
        if storage.resource_id > 0 {
            storage.resource_id
        } else {
            storage.id
        }
    })
}

fn optional_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn notification_routing_payload(
    target_uid: &str,
    match_labels: &[(String, String)],
    resend_interval_minutes: Option<i64>,
) -> serde_json::Value {
    let mut payload = json!({
        "notification_target_uid": target_uid,
        "match_labels": match_labels
            .iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect::<Vec<_>>(),
    });
    if let Some(minutes) = resend_interval_minutes {
        payload["resend_interval_minutes"] = json!(minutes);
    }
    payload
}

fn access_key_secret(mut value: serde_json::Value) -> Result<StorageAccessKeySecret> {
    if let Some(result) = value.get_mut("result") {
        value = result.take();
    }
    let raw: RawStorageAccessKey =
        serde_json::from_value(value).context("アクセスキーのレスポンス解析に失敗しました")?;
    if raw.uid.is_empty() || raw.secret.is_empty() {
        bail!("アクセスキーの秘密情報がレスポンスに含まれていません");
    }
    Ok(StorageAccessKeySecret {
        uid: raw.uid,
        token: raw.token,
        secret: raw.secret,
    })
}

#[derive(Debug, Clone)]
pub struct AlertRuleInput {
    pub metrics_storage_id: i64,
    pub name: String,
    pub query: String,
    pub warning_enabled: bool,
    pub critical_enabled: bool,
    pub threshold_warning: String,
    pub threshold_critical: String,
    pub duration_warning: i64,
    pub duration_critical: i64,
}

#[derive(Debug, Clone)]
pub struct LogMeasureRuleInput {
    pub log_storage_id: i64,
    pub metrics_storage_id: i64,
    pub name: String,
    pub description: String,
    pub rule: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct LogRoutingInput {
    pub publisher_code: String,
    pub resource_id: Option<i64>,
    pub variant: String,
    pub log_storage_id: i64,
}

impl LogRoutingInput {
    fn payload(&self) -> serde_json::Value {
        json!({
            "publisher_code": self.publisher_code,
            "resource_id": self.resource_id,
            "variant": self.variant,
            "log_storage_id": self.log_storage_id,
        })
    }
}

impl LogMeasureRuleInput {
    fn payload(&self) -> serde_json::Value {
        json!({
            "log_storage_id": self.log_storage_id,
            "metrics_storage_id": self.metrics_storage_id,
            "name": self.name,
            "description": self.description,
            "rule": self.rule,
        })
    }
}

impl AlertRuleInput {
    fn payload(&self) -> serde_json::Value {
        json!({
            "metrics_storage_id": self.metrics_storage_id,
            "name": self.name,
            "query": self.query,
            "enabled_warning": self.warning_enabled,
            "enabled_critical": self.critical_enabled,
            "threshold_warning": self.warning_enabled.then_some(&self.threshold_warning),
            "threshold_critical": self.critical_enabled.then_some(&self.threshold_critical),
            "threshold_duration_warning": self.duration_warning,
            "threshold_duration_critical": self.duration_critical,
        })
    }
}

/// 小数を文字列でも受け取る。
fn flexible_float<'de, D>(de: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(
        match Option::<serde_json::Value>::deserialize(de)?.flatten_value() {
            Some(serde_json::Value::Number(n)) => n.as_f64(),
            Some(serde_json::Value::String(s)) => s.parse().ok(),
            _ => None,
        },
    )
}

/// `Option<Value>` の `Null` も `None` として扱うための小さな補助。
trait FlattenValue {
    fn flatten_value(self) -> Option<serde_json::Value>;
}

impl FlattenValue for Option<serde_json::Value> {
    fn flatten_value(self) -> Option<serde_json::Value> {
        self.filter(|v| !v.is_null())
    }
}

fn format_api_error(status: StatusCode, body: &str) -> String {
    let parsed = serde_json::from_str::<ApiError>(body).unwrap_or_default();
    for candidate in [&parsed.detail, &parsed.message, &parsed.error_msg] {
        if !candidate.is_empty() {
            return format!("モニタリングAPIエラー ({status}): {candidate}");
        }
    }
    let head: String = body.trim().chars().take(200).collect();
    if head.is_empty() {
        format!("モニタリングAPIエラー ({status})")
    } else {
        format!("モニタリングAPIエラー ({status}): {head}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_projects() {
        let body = r#"{"count": 1, "from": 0, "total": 1, "is_ok": true, "results": [
            {"id": 12, "name": "prod", "description": null, "tags": [],
             "created_at": "2026-01-02T03:04:05+09:00"}
        ]}"#;
        let res: Paginated<RawProject> = serde_json::from_str(body).unwrap();
        assert_eq!(res.total, 1);
        let items = res.items();
        assert_eq!(items[0].id, 12);
        assert_eq!(items[0].description, "");
    }

    #[test]
    fn parses_rules() {
        let body = r#"{"total": 1, "results": [
            {"uid": "d9428888-122b-11e1-b85c-61cd3cbb3210", "metrics_storage_id": "113700000001",
             "name": "CPU高負荷", "query": "avg(cpu)", "open": true,
             "enabled_warning": true, "enabled_critical": false,
             "threshold_warning": "80", "threshold_critical": null,
             "threshold_duration_warning": 120, "threshold_duration_critical": 60}
        ]}"#;
        let res: Paginated<RawRule> = serde_json::from_str(body).unwrap();
        let rule = res.items().into_iter().next().unwrap();
        assert_eq!(rule.metrics_storage_id, 113_700_000_001);
        assert!(!rule.uid.is_empty());
        assert!(rule.open);
        assert!(rule.enabled_warning);
        assert!(!rule.enabled_critical);
        assert_eq!(rule.threshold_critical, "");
        assert_eq!(rule.threshold_duration_warning, 120);
    }

    #[test]
    fn parses_histories() {
        let body = r#"{"total": 1, "results": [
            {"uid": "h-1", "rule_uid": "r-1", "startsAt": "2026-08-01T10:00:00Z",
             "endsAt": "", "open": true, "labels": "host=web01",
             "severity": "critical", "threshold": "90", "value": 95.5}
        ]}"#;
        let res: Paginated<RawHistory> = serde_json::from_str(body).unwrap();
        let history = res.items().into_iter().next().unwrap();
        assert_eq!(history.severity, "critical");
        assert_eq!(history.value, Some(95.5));
        assert!(history.open);
    }

    /// ログは expire_day、トレースは retention_period_days で保持期間を返す。
    #[test]
    fn storage_retention_comes_from_either_field() {
        fn retention(raw: &RawStorage) -> Option<i64> {
            [raw.expire_day, raw.retention_period_days]
                .into_iter()
                .find(|d| *d > 0)
        }

        let logs: RawStorage =
            serde_json::from_str(r#"{"id": 1, "name": "log", "expire_day": 30}"#).unwrap();
        assert_eq!(retention(&logs), Some(30));

        let traces: RawStorage =
            serde_json::from_str(r#"{"id": 2, "name": "trace", "retention_period_days": 7}"#)
                .unwrap();
        assert_eq!(retention(&traces), Some(7));

        let metrics: RawStorage = serde_json::from_str(r#"{"id": 3, "name": "m"}"#).unwrap();
        assert!(retention(&metrics).is_none());
    }

    #[test]
    fn system_storage_does_not_support_access_keys() {
        let storage = Storage {
            kind: StorageKind::Logs,
            id: 1,
            resource_id: 1,
            name: "system".to_string(),
            description: String::new(),
            classification: "shared".to_string(),
            retention_days: Some(30),
            is_system: true,
        };
        assert!(!storage.supports_access_keys());
        assert!(
            Storage {
                is_system: false,
                ..storage
            }
            .supports_access_keys()
        );
    }

    #[test]
    fn results_null_is_empty() {
        let res: Paginated<RawProject> =
            serde_json::from_str(r#"{"total": 0, "results": null}"#).unwrap();
        assert!(res.items().is_empty());
    }

    #[test]
    fn rule_payload_omits_disabled_threshold_with_null() {
        let input = AlertRuleInput {
            metrics_storage_id: 113_700_000_001,
            name: "CPU".to_string(),
            query: "avg(cpu)".to_string(),
            warning_enabled: true,
            critical_enabled: false,
            threshold_warning: "80".to_string(),
            threshold_critical: String::new(),
            duration_warning: 60,
            duration_critical: 60,
        };
        let payload = input.payload();
        assert_eq!(payload["metrics_storage_id"], 113_700_000_001_i64);
        assert_eq!(payload["threshold_warning"], "80");
        assert!(payload["threshold_critical"].is_null());
    }

    #[test]
    fn parses_wrapped_access_key_secret() {
        let key = access_key_secret(json!({
            "result": {
                "id": "12",
                "uid": "05cfc8ee-56fd-4cec-b490-f13f24c37ac5",
                "token": "writer-token",
                "secret": "writer-secret",
                "description": "collector"
            }
        }))
        .unwrap();
        assert_eq!(key.uid, "05cfc8ee-56fd-4cec-b490-f13f24c37ac5");
        assert_eq!(key.token, "writer-token");
        assert_eq!(key.secret, "writer-secret");
    }

    #[test]
    fn access_key_secret_debug_is_redacted() {
        let key = StorageAccessKeySecret {
            uid: "key-1".to_string(),
            token: "do-not-log-token".to_string(),
            secret: "do-not-log-secret".to_string(),
        };
        let debug = format!("{key:?}");
        assert!(!debug.contains("do-not-log-token"));
        assert!(!debug.contains("do-not-log-secret"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn parses_notification_routing_and_builds_payload() {
        let body = r#"{"total":1,"results":[{
            "uid":"route-1","notification_target":{
                "uid":"target-1","service_type":"SAKURA_EVENT_BUS","description":"autoscale"
            },
            "match_labels":[{"name":"severity","value":"critical"}],
            "resend_interval_minutes":"30","order":"2"
        }]}"#;
        let res: Paginated<RawNotificationRouting> = serde_json::from_str(body).unwrap();
        let routing = res.items().into_iter().next().unwrap();
        assert_eq!(routing.notification_target.uid, "target-1");
        assert_eq!(routing.resend_interval_minutes, 30);
        assert_eq!(routing.match_labels[0].name, "severity");

        let labels = vec![("severity".to_string(), "critical".to_string())];
        let payload = notification_routing_payload("target-1", &labels, Some(30));
        assert_eq!(payload["notification_target_uid"], "target-1");
        assert_eq!(payload["match_labels"][0]["value"], "critical");
        assert_eq!(payload["resend_interval_minutes"], 30);

        let without_resend = notification_routing_payload("target-1", &[], None);
        assert!(without_resend.get("resend_interval_minutes").is_none());
    }

    #[test]
    fn parses_log_measure_rule_and_preserves_matcher_json() {
        let body = r#"{"total":1,"results":[{
            "uid":"measure-1","name":"errors","description":"count errors",
            "log_storage":{"resource_id":"101"},
            "metrics_storage":{"resource_id":"202"},
            "rule":{"version":"v1","query":{"matchers":[
                {"type":"string","operator":"ilike","field":"text_payload","value":"%error%","value_list":[]}
            ]}}
        }]}"#;
        let res: Paginated<RawLogMeasureRule> = serde_json::from_str(body).unwrap();
        let raw = res.items().into_iter().next().unwrap();
        assert_eq!(storage_ref_id(raw.log_storage), 101);
        assert_eq!(storage_ref_id(raw.metrics_storage), 202);
        assert_eq!(raw.rule["query"]["matchers"][0]["operator"], "ilike");

        let input = LogMeasureRuleInput {
            log_storage_id: 101,
            metrics_storage_id: 202,
            name: "errors".to_string(),
            description: "count errors".to_string(),
            rule: raw.rule,
        };
        let payload = input.payload();
        assert_eq!(payload["log_storage_id"], 101);
        assert_eq!(payload["rule"]["query"]["matchers"][0]["value"], "%error%");
    }

    #[test]
    fn parses_log_routing_and_builds_payload() {
        let body = r#"{"total":1,"results":[{
            "uid":"route-1","resource_id":"113700000001",
            "publisher":{"code":"server","description":"サーバー"},
            "variant":"system","log_storage":{"resource_id":"113700000002"}
        }]}"#;
        let res: Paginated<RawLogRouting> = serde_json::from_str(body).unwrap();
        let raw = res.items().into_iter().next().unwrap();
        assert_eq!(raw.publisher.code, "server");
        assert_eq!(optional_i64(&raw.resource_id), Some(113_700_000_001));
        assert_eq!(storage_ref_id(raw.log_storage), 113_700_000_002);

        let payload = LogRoutingInput {
            publisher_code: "server".to_string(),
            resource_id: Some(113_700_000_001),
            variant: "system".to_string(),
            log_storage_id: 113_700_000_002,
        }
        .payload();
        assert_eq!(payload["publisher_code"], "server");
        assert_eq!(payload["resource_id"], 113_700_000_001_i64);
        assert_eq!(payload["log_storage_id"], 113_700_000_002_i64);
    }

    #[test]
    fn formats_error_from_detail() {
        let message = format_api_error(StatusCode::NOT_FOUND, r#"{"detail": "見つかりません"}"#);
        assert!(message.contains("見つかりません"), "{message}");
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    /// 実際のレスポンスでは OpenAPI が integer と書いている ID が
    /// 文字列で返ってくる。これで解析が落ちていた。
    #[test]
    fn accepts_string_ids_in_storages() {
        let body = r#"{"count":2,"from":0,"total":2,"is_ok":true,"results":[
            {"id":"113701924793","name":"システムログ","description":"システムログ領域",
             "tags":[],"icon":null,"expire_day":"30","account_id":"113600000000",
             "resource_id":"113701924793","is_system":true,"classification":"shared"}
        ]}"#;
        let res: Paginated<RawStorage> = serde_json::from_str(body).unwrap();
        assert_eq!(res.total, 2);
        let raw = res.items().into_iter().next().unwrap();
        assert_eq!(raw.id, 113_701_924_793);
        assert_eq!(raw.resource_id, 113_701_924_793);
        assert_eq!(raw.name, "システムログ");
        assert_eq!(raw.expire_day, 30);
        assert!(raw.is_system);
    }

    #[test]
    fn accepts_string_ids_in_projects() {
        let body = r#"{"total":"1","results":[
            {"id":"12","name":"prod","description":null,"tags":[],
             "resource_id":"113700000001","created_at":"2026-01-02T03:04:05+09:00"}
        ]}"#;
        let res: Paginated<RawProject> = serde_json::from_str(body).unwrap();
        assert_eq!(res.total, 1);
        let raw = res.items().into_iter().next().unwrap();
        assert_eq!(raw.id, 12);
        // パスに使うのは resource_id のほう。
        assert_eq!(raw.resource_id, 113_700_000_001);
    }

    /// resource_id が返らない場合は id で代用すること。
    #[test]
    fn falls_back_to_id_when_resource_id_missing() {
        let raw: RawProject = serde_json::from_str(r#"{"id": 7, "name": "x"}"#).unwrap();
        let resource_id = if raw.resource_id > 0 {
            raw.resource_id
        } else {
            raw.id
        };
        assert_eq!(resource_id, 7);
    }

    /// メトリクスには保持期間が無いので、0 を「未設定」として扱う。
    #[test]
    fn zero_retention_is_treated_as_unset() {
        let raw: RawStorage = serde_json::from_str(r#"{"id": 1, "name": "m"}"#).unwrap();
        let retention = [raw.expire_day, raw.retention_period_days]
            .into_iter()
            .find(|d| *d > 0);
        assert!(retention.is_none());
    }

    #[test]
    fn accepts_string_value_in_history() {
        let body = r#"{"results":[
            {"rule_uid":"r-1","severity":"warning","startsAt":"2026-08-01T10:00:00Z",
             "open":false,"labels":"","threshold":"90","value":"95.5"}
        ]}"#;
        let res: Paginated<RawHistory> = serde_json::from_str(body).unwrap();
        assert_eq!(res.items()[0].value, Some(95.5));
    }
}
