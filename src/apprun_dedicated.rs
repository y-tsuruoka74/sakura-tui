//! さくらのクラウド AppRun 専有型 API クライアント（閲覧のみ）。
//!
//! 共用型とは別のエンドポイント・別のリソース体系で、クラスタを頂点に
//! オートスケーリンググループ（ASG）・ワーカーノード・証明書がぶら下がる。
//! ページングは共用型のページ番号方式ではなくカーソル方式。

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::config::ApiCredentials;
use crate::sacloud::null_as_default;

fn api_root(creds: &ApiCredentials) -> String {
    let base = creds.api_root().trim_end_matches("/zone");
    format!("{base}/api/apprun-dedicated/1.0")
}
/// 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 100;
/// カーソルを辿る上限。API が同じカーソルを返し続けても止まるようにする。
const MAX_PAGES: usize = 100;

/// ロードバランサが待ち受けるポート。
#[derive(Debug, Clone)]
pub struct Port {
    pub port: u32,
    pub protocol: String,
}

impl std::fmt::Display for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.port, self.protocol)
    }
}

/// クラスタ 1 件。
#[derive(Debug, Clone)]
pub struct Cluster {
    pub id: String,
    pub name: String,
    /// 作成日時（Unix 秒）。
    pub created: Option<i64>,
    // --- 詳細取得でのみ埋まる ---
    pub ports: Vec<Port>,
    pub service_principal_id: String,
    pub has_lets_encrypt_email: bool,
}

/// アプリケーション 1 件。
#[derive(Debug, Clone)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub active_version: Option<i64>,
    pub desired_count: Option<i64>,
    pub scaling_cooldown_seconds: Option<i64>,
}

/// オートスケーリンググループ 1 件。
#[derive(Debug, Clone)]
pub struct AutoScalingGroup {
    pub id: String,
    pub name: String,
    pub zone: String,
    pub service_class: String,
    pub min_nodes: Option<i64>,
    pub max_nodes: Option<i64>,
    pub worker_node_count: Option<i64>,
    pub deleting: bool,
}

/// ワーカーノード 1 件。
#[derive(Debug, Clone)]
pub struct WorkerNode {
    pub id: String,
    pub status: String,
    pub draining: bool,
    pub archive_version: String,
    pub created: Option<i64>,
    pub error_message: String,
}

/// 証明書 1 件。
#[derive(Debug, Clone)]
pub struct Certificate {
    pub name: String,
    pub common_name: String,
    pub alternative_names: Vec<String>,
    /// 有効期限（Unix 秒）。
    pub not_after: Option<i64>,
}

// --- API のレスポンス形状 ---

#[derive(Debug, Deserialize)]
struct RawCluster {
    #[serde(rename = "clusterID", default, deserialize_with = "null_as_default")]
    cluster_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    created: Option<i64>,
    #[serde(default, deserialize_with = "null_as_default")]
    ports: Vec<RawPort>,
    #[serde(
        rename = "servicePrincipalID",
        default,
        deserialize_with = "null_as_default"
    )]
    service_principal_id: String,
    #[serde(rename = "hasLetsEncryptEmail", default)]
    has_lets_encrypt_email: bool,
}

#[derive(Debug, Deserialize)]
struct RawPort {
    #[serde(default)]
    port: u32,
    #[serde(default, deserialize_with = "null_as_default")]
    protocol: String,
}

#[derive(Debug, Deserialize)]
struct ListClustersResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    clusters: Vec<RawCluster>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetClusterResponse {
    cluster: RawCluster,
}

#[derive(Debug, Deserialize)]
struct RawApplication {
    #[serde(
        rename = "applicationID",
        default,
        deserialize_with = "null_as_default"
    )]
    application_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "activeVersion")]
    active_version: Option<i64>,
    #[serde(rename = "desiredCount")]
    desired_count: Option<i64>,
    #[serde(rename = "scalingCooldownSeconds")]
    scaling_cooldown_seconds: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListApplicationsResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    applications: Vec<RawApplication>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAutoScalingGroup {
    #[serde(
        rename = "autoScalingGroupID",
        default,
        deserialize_with = "null_as_default"
    )]
    auto_scaling_group_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    zone: String,
    #[serde(
        rename = "workerServiceClassPath",
        default,
        deserialize_with = "null_as_default"
    )]
    worker_service_class_path: String,
    #[serde(rename = "minNodes")]
    min_nodes: Option<i64>,
    #[serde(rename = "maxNodes")]
    max_nodes: Option<i64>,
    #[serde(rename = "workerNodeCount")]
    worker_node_count: Option<i64>,
    #[serde(default)]
    deleting: bool,
}

#[derive(Debug, Deserialize)]
struct ListAsgResponse {
    #[serde(
        rename = "autoScalingGroups",
        default,
        deserialize_with = "null_as_default"
    )]
    auto_scaling_groups: Vec<RawAutoScalingGroup>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawWorkerNode {
    #[serde(rename = "workerNodeID", default, deserialize_with = "null_as_default")]
    worker_node_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    status: String,
    #[serde(default)]
    draining: bool,
    #[serde(
        rename = "archiveVersion",
        default,
        deserialize_with = "null_as_default"
    )]
    archive_version: String,
    created: Option<i64>,
    #[serde(
        rename = "createErrorMessage",
        default,
        deserialize_with = "null_as_default"
    )]
    create_error_message: String,
}

#[derive(Debug, Deserialize)]
struct ListWorkerNodesResponse {
    #[serde(rename = "workerNodes", default, deserialize_with = "null_as_default")]
    worker_nodes: Vec<RawWorkerNode>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCertificate {
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "commonName", default, deserialize_with = "null_as_default")]
    common_name: String,
    #[serde(
        rename = "subjectAlternativeNames",
        default,
        deserialize_with = "null_as_default"
    )]
    subject_alternative_names: Vec<String>,
    #[serde(rename = "notAfterSec")]
    not_after_sec: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListCertificatesResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    certificates: Vec<RawCertificate>,
    #[serde(rename = "nextCursor")]
    next_cursor: Option<String>,
}

/// エラー時のレスポンス。
#[derive(Debug, Default, Deserialize)]
struct ApiError {
    #[serde(default, deserialize_with = "null_as_default")]
    message: String,
    #[serde(default, deserialize_with = "null_as_default")]
    detail: String,
    #[serde(default, deserialize_with = "null_as_default")]
    error_msg: String,
    #[serde(default, deserialize_with = "null_as_default")]
    error_code: String,
}

impl ApiError {
    fn parts(&self) -> Option<(&str, &str)> {
        if !self.message.is_empty() {
            return Some((&self.message, &self.detail));
        }
        if !self.error_msg.is_empty() {
            return Some((&self.error_msg, &self.error_code));
        }
        None
    }
}

#[derive(Debug)]
pub struct DedicatedClient {
    http: reqwest::Client,
    token: String,
    secret: String,
    api_root: String,
}

impl DedicatedClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        let http = crate::http::client()?;
        Ok(Self {
            http,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
            api_root: api_root(creds),
        })
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let url = format!("{}{path}", self.api_root);
        let res = crate::http::send_with_retry(&self.http, || {
            Ok(self
                .http
                .request(Method::GET, &url)
                .basic_auth(&self.token, Some(&self.secret))
                .query(query)
                .build()?)
        })
        .await
        .context("AppRun専有型APIへのリクエストに失敗しました")?;
        let status = res.status();
        let text = res
            .text()
            .await
            .context("AppRun専有型APIのレスポンス読み取りに失敗しました")?;

        if !status.is_success() {
            bail!("{}", format_api_error(status, &text));
        }
        let text = if text.trim().is_empty() { "{}" } else { &text };
        serde_json::from_str(text).with_context(|| {
            let head: String = text.chars().take(200).collect();
            format!("AppRun専有型APIのレスポンス解析に失敗しました: {head}")
        })
    }

    /// カーソルを辿って全件集める。
    ///
    /// `fetch` はカーソルを受け取り `(そのページの要素, 次のカーソル)` を返す。
    async fn collect<T, F, Fut>(&self, mut fetch: F) -> Result<Vec<T>>
    where
        F: FnMut(Option<String>) -> Fut,
        Fut: Future<Output = Result<(Vec<T>, Option<String>)>>,
    {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let (items, next) = fetch(cursor.clone()).await?;
            let received = items.len();
            out.extend(items);
            match next {
                // 同じカーソルが返ってきたら進んでいないので打ち切る。
                Some(next) if received > 0 && Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        Ok(out)
    }

    fn page_query(cursor: Option<String>) -> Vec<(&'static str, String)> {
        let mut query = vec![("maxItems", PAGE_SIZE.to_string())];
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor));
        }
        query
    }

    pub async fn list_clusters(&self) -> Result<Vec<Cluster>> {
        self.collect(|cursor| async move {
            let res: ListClustersResponse =
                self.get("/clusters", &Self::page_query(cursor)).await?;
            Ok((
                res.clusters.into_iter().map(Cluster::from).collect(),
                res.next_cursor,
            ))
        })
        .await
    }

    /// クラスタの詳細（ポートやサービスプリンシパルは一覧には含まれない）。
    pub async fn cluster_detail(&self, id: &str) -> Result<Cluster> {
        let res: GetClusterResponse = self.get(&format!("/clusters/{id}"), &[]).await?;
        Ok(Cluster::from(res.cluster))
    }

    /// クラスタに属するアプリケーション。
    pub async fn list_applications(&self, cluster_id: &str) -> Result<Vec<Application>> {
        self.collect(|cursor| async move {
            let mut query = Self::page_query(cursor);
            query.push(("clusterID", cluster_id.to_string()));
            let res: ListApplicationsResponse = self.get("/applications", &query).await?;
            Ok((
                res.applications
                    .into_iter()
                    .map(Application::from)
                    .collect(),
                res.next_cursor,
            ))
        })
        .await
    }

    pub async fn list_asg(&self, cluster_id: &str) -> Result<Vec<AutoScalingGroup>> {
        let path = format!("/clusters/{cluster_id}/asg");
        self.collect(|cursor| {
            let path = path.clone();
            async move {
                let res: ListAsgResponse = self.get(&path, &Self::page_query(cursor)).await?;
                Ok((
                    res.auto_scaling_groups
                        .into_iter()
                        .map(AutoScalingGroup::from)
                        .collect(),
                    res.next_cursor,
                ))
            }
        })
        .await
    }

    pub async fn list_worker_nodes(
        &self,
        cluster_id: &str,
        asg_id: &str,
    ) -> Result<Vec<WorkerNode>> {
        let path = format!("/clusters/{cluster_id}/asg/{asg_id}/worker_nodes");
        self.collect(|cursor| {
            let path = path.clone();
            async move {
                let res: ListWorkerNodesResponse =
                    self.get(&path, &Self::page_query(cursor)).await?;
                Ok((
                    res.worker_nodes.into_iter().map(WorkerNode::from).collect(),
                    res.next_cursor,
                ))
            }
        })
        .await
    }

    pub async fn list_certificates(&self, cluster_id: &str) -> Result<Vec<Certificate>> {
        let path = format!("/clusters/{cluster_id}/certificates");
        self.collect(|cursor| {
            let path = path.clone();
            async move {
                let res: ListCertificatesResponse =
                    self.get(&path, &Self::page_query(cursor)).await?;
                Ok((
                    res.certificates
                        .into_iter()
                        .map(Certificate::from)
                        .collect(),
                    res.next_cursor,
                ))
            }
        })
        .await
    }
}

impl From<RawCluster> for Cluster {
    fn from(raw: RawCluster) -> Self {
        Cluster {
            id: raw.cluster_id,
            name: raw.name,
            created: raw.created,
            ports: raw
                .ports
                .into_iter()
                .map(|p| Port {
                    port: p.port,
                    protocol: p.protocol,
                })
                .collect(),
            service_principal_id: raw.service_principal_id,
            has_lets_encrypt_email: raw.has_lets_encrypt_email,
        }
    }
}

impl From<RawApplication> for Application {
    fn from(raw: RawApplication) -> Self {
        Application {
            id: raw.application_id,
            name: raw.name,
            active_version: raw.active_version,
            desired_count: raw.desired_count,
            scaling_cooldown_seconds: raw.scaling_cooldown_seconds,
        }
    }
}

impl From<RawAutoScalingGroup> for AutoScalingGroup {
    fn from(raw: RawAutoScalingGroup) -> Self {
        AutoScalingGroup {
            id: raw.auto_scaling_group_id,
            name: raw.name,
            zone: raw.zone,
            service_class: raw.worker_service_class_path,
            min_nodes: raw.min_nodes,
            max_nodes: raw.max_nodes,
            worker_node_count: raw.worker_node_count,
            deleting: raw.deleting,
        }
    }
}

impl From<RawWorkerNode> for WorkerNode {
    fn from(raw: RawWorkerNode) -> Self {
        WorkerNode {
            id: raw.worker_node_id,
            status: raw.status,
            draining: raw.draining,
            archive_version: raw.archive_version,
            created: raw.created,
            error_message: raw.create_error_message,
        }
    }
}

impl From<RawCertificate> for Certificate {
    fn from(raw: RawCertificate) -> Self {
        Certificate {
            name: raw.name,
            common_name: raw.common_name,
            alternative_names: raw.subject_alternative_names,
            not_after: raw.not_after_sec,
        }
    }
}

/// 403 のときに添える案内。専有型はサービスプリンシパルが前提になる。
const FORBIDDEN_HINT: &str = "\n\nAppRun専有型 API には、APIキーに専有型の権限があることと、\n\
     クラスタに紐づくサービスプリンシパルが設定されていることが必要です。";

/// 401 のときに添える案内。
///
/// 同じ API キーで他のサービスが見えているのに AppRun だけ 401 になる場合、
/// その環境に AppRun が無いか、AppRun 側のユーザーが未作成のことが多い。
const UNAUTHORIZED_HINT: &str = "\n\n     同じキーで他のサービスが見えているなら、次のどちらかです:\n\
     ・この環境に AppRun が無い（社内テスト環境では未提供のことがあります）\n\
     ・AppRun 側のユーザーが未作成\n\
     --trace を付けて起動すると、実際に叩いた URL を確認できます。";

fn format_api_error(status: StatusCode, body: &str) -> String {
    let hint = match status {
        StatusCode::FORBIDDEN => FORBIDDEN_HINT,
        StatusCode::UNAUTHORIZED => UNAUTHORIZED_HINT,
        _ => "",
    };
    let parsed = serde_json::from_str::<ApiError>(body).unwrap_or_default();
    if let Some((summary, detail)) = parsed.parts() {
        return if detail.is_empty() {
            format!("AppRun専有型APIエラー ({status}): {summary}{hint}")
        } else {
            format!("AppRun専有型APIエラー ({status}): {summary} [{detail}]{hint}")
        };
    }
    let head: String = body.trim().chars().take(200).collect();
    if head.is_empty() {
        format!("AppRun専有型APIエラー ({status}){hint}")
    } else {
        format!("AppRun専有型APIエラー ({status}): {head}{hint}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cluster_list() {
        let body = r#"{"clusters": [
            {"clusterID": "13B9EA83-DDB0-4385-9533-3D693A6A310F",
             "name": "prod", "created": 1767225845}
        ], "nextCursor": null}"#;
        let res: ListClustersResponse = serde_json::from_str(body).unwrap();
        let cluster = Cluster::from(res.clusters.into_iter().next().unwrap());
        assert_eq!(cluster.name, "prod");
        assert_eq!(cluster.created, Some(1_767_225_845));
        // 一覧にはポートが含まれない。
        assert!(cluster.ports.is_empty());
        assert!(res.next_cursor.is_none());
    }

    #[test]
    fn parses_cluster_detail_with_ports() {
        let body = r#"{"cluster": {
            "clusterID": "abc", "name": "prod", "created": 1767225845,
            "ports": [{"port": 443, "protocol": "https"}, {"port": 80, "protocol": "http"}],
            "servicePrincipalID": "sp-1", "hasLetsEncryptEmail": true
        }}"#;
        let res: GetClusterResponse = serde_json::from_str(body).unwrap();
        let cluster = Cluster::from(res.cluster);
        assert_eq!(cluster.ports.len(), 2);
        assert_eq!(cluster.ports[0].to_string(), "443/https");
        assert!(cluster.has_lets_encrypt_email);
        assert_eq!(cluster.service_principal_id, "sp-1");
    }

    #[test]
    fn parses_worker_nodes() {
        let body = r#"{"workerNodes": [
            {"workerNodeID": "wn-1", "status": "healthy", "draining": false,
             "archiveVersion": "2026.01", "created": 1767225845, "createErrorMessage": ""}
        ], "nextCursor": null}"#;
        let res: ListWorkerNodesResponse = serde_json::from_str(body).unwrap();
        let node = WorkerNode::from(res.worker_nodes.into_iter().next().unwrap());
        assert_eq!(node.status, "healthy");
        assert!(!node.draining);
    }

    #[test]
    fn parses_certificates() {
        let body = r#"{"certificates": [
            {"certificateID": "c-1", "name": "web", "commonName": "example.jp",
             "subjectAlternativeNames": ["www.example.jp"], "notAfterSec": 1790000000}
        ]}"#;
        let res: ListCertificatesResponse = serde_json::from_str(body).unwrap();
        let cert = Certificate::from(res.certificates.into_iter().next().unwrap());
        assert_eq!(cert.common_name, "example.jp");
        assert_eq!(cert.alternative_names, vec!["www.example.jp"]);
        assert_eq!(cert.not_after, Some(1_790_000_000));
    }

    /// 未設定の項目が `null` でも既定値として受けられること。
    #[test]
    fn tolerates_nulls() {
        let body = r#"{"autoScalingGroups": [
            {"autoScalingGroupID": "asg-1", "name": null, "zone": null,
             "workerServiceClassPath": null, "minNodes": null, "maxNodes": null,
             "workerNodeCount": null}
        ], "nextCursor": null}"#;
        let res: ListAsgResponse = serde_json::from_str(body).unwrap();
        let asg = AutoScalingGroup::from(res.auto_scaling_groups.into_iter().next().unwrap());
        assert_eq!(asg.name, "");
        assert!(asg.min_nodes.is_none());
    }

    #[test]
    fn forbidden_includes_hint() {
        let message = format_api_error(StatusCode::FORBIDDEN, r#"{"message": "forbidden"}"#);
        assert!(message.contains("サービスプリンシパル"), "{message}");
    }
}
