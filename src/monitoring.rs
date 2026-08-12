//! さくらのクラウド モニタリングスイート API（閲覧のみ）。
//!
//! IaaS とはパスの接尾辞が違う（`api/monitoring/1.0`）ため、専用のクライアントを持つ。
//! アラートプロジェクトを頂点に、ルールと発報履歴がぶら下がる。

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::config::ApiCredentials;
use crate::sacloud::{flexible_number, null_as_default};

const API_ROOT: &str = "https://secure.sakura.ad.jp/cloud/zone";
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
    pub name: String,
    pub query: String,
    /// 発報中かどうか。
    pub open: bool,
    pub warning_enabled: bool,
    pub critical_enabled: bool,
    pub threshold_warning: String,
    pub threshold_critical: String,
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

/// ログ・メトリクス・トレースの保管先。
#[derive(Debug, Clone)]
pub struct Storage {
    pub kind: StorageKind,
    pub id: i64,
    pub name: String,
    pub description: String,
    /// 共用 / 専有。
    pub classification: String,
    /// 保持日数（メトリクスには無い）。
    pub retention_days: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    Logs,
    Metrics,
    Traces,
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

#[derive(Debug, Deserialize)]
struct RawStorage {
    #[serde(default, deserialize_with = "flexible_number")]
    id: i64,
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
}

impl MonitoringClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        let http = crate::http::client()?;
        Ok(Self {
            http,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        zone: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let url = format!("{API_ROOT}/{zone}/{API_SUFFIX}/{path}");
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
                            name: r.name,
                            query: r.query,
                            open: r.open,
                            warning_enabled: r.enabled_warning,
                            critical_enabled: r.enabled_critical,
                            threshold_warning: r.threshold_warning,
                            threshold_critical: r.threshold_critical,
                        })
                        .collect(),
                    total,
                ))
            }
        })
        .await
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
                    name: s.name,
                    description: s.description,
                    classification: s.classification,
                    // 0 は「未設定」とみなす（メトリクスには保持期間の概念が無い）。
                    retention_days: [s.expire_day, s.retention_period_days]
                        .into_iter()
                        .find(|d| *d > 0),
                }
            }));
        }
        Ok(out)
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
            {"uid": "r-1", "name": "CPU高負荷", "query": "avg(cpu)", "open": true,
             "enabled_warning": true, "enabled_critical": false,
             "threshold_warning": "80", "threshold_critical": null}
        ]}"#;
        let res: Paginated<RawRule> = serde_json::from_str(body).unwrap();
        let rule = res.items().into_iter().next().unwrap();
        assert!(rule.open);
        assert!(rule.enabled_warning);
        assert!(!rule.enabled_critical);
        assert_eq!(rule.threshold_critical, "");
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
    fn results_null_is_empty() {
        let res: Paginated<RawProject> =
            serde_json::from_str(r#"{"total": 0, "results": null}"#).unwrap();
        assert!(res.items().is_empty());
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
        assert_eq!(raw.name, "システムログ");
        assert_eq!(raw.expire_day, 30);
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
