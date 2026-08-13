//! さくらのクラウド AppRun（共用型）API クライアント。
//!
//! IaaS とは別のエンドポイントだが、認証は同じ API キー（Basic 認証）を使う。

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::config::ApiCredentials;
use crate::sacloud::null_as_default;

/// `.../cloud/zone` から `/zone` を落とした部分に、各サービスのパスが生える。
fn api_root(creds: &ApiCredentials) -> String {
    let base = creds.api_root().trim_end_matches("/zone");
    format!("{base}/api/apprun/1.0/apprun/api")
}
/// 1 ページあたりの取得件数（API の上限に合わせる）。
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が実態と違う総件数を返しても止まるようにする。
const MAX_PAGES: usize = 100;

/// アプリケーション 1 件（一覧表示用）。
#[derive(Debug, Clone)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub status: String,
    pub public_url: String,
    pub created_at: Option<String>,
}

/// アプリケーションの詳細。
#[derive(Debug, Clone, Default)]
pub struct ApplicationDetail {
    pub port: Option<u32>,
    pub timeout_seconds: Option<u32>,
    pub min_scale: Option<u32>,
    pub max_scale: Option<u32>,
    /// コンテナイメージ（レジストリのイメージ参照）。
    pub images: Vec<String>,
}

/// バージョン 1 件。
#[derive(Debug, Clone)]
pub struct Version {
    pub name: String,
    pub status: String,
    pub created_at: Option<String>,
}

/// トラフィックの振り分け。
#[derive(Debug, Clone)]
pub struct Traffic {
    pub version_name: String,
    pub is_latest: bool,
    pub percent: i32,
}

// --- API のレスポンス形状 ---

#[derive(Debug, Deserialize)]
struct Paged<T> {
    /// `null` で返ることがあるので `Option` で受けて空とみなす。
    data: Option<Vec<T>>,
    meta: Option<Meta>,
}

impl<T> Paged<T> {
    fn items(self) -> Vec<T> {
        self.data.unwrap_or_default()
    }

    fn total(&self) -> usize {
        self.meta.as_ref().map_or(0, |m| m.object_total)
    }
}

#[derive(Debug, Deserialize)]
struct Meta {
    #[serde(default)]
    object_total: usize,
}

#[derive(Debug, Deserialize)]
struct RawApplication {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    public_url: String,
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawApplicationDetail {
    port: Option<u32>,
    timeout_seconds: Option<u32>,
    min_scale: Option<u32>,
    max_scale: Option<u32>,
    #[serde(default, deserialize_with = "null_as_default")]
    components: Vec<RawComponent>,
}

#[derive(Debug, Deserialize)]
struct RawComponent {
    deploy_source: Option<RawDeploySource>,
}

#[derive(Debug, Deserialize)]
struct RawDeploySource {
    container_registry: Option<RawContainerRegistrySource>,
}

#[derive(Debug, Deserialize)]
struct RawContainerRegistrySource {
    #[serde(default, deserialize_with = "null_as_default")]
    image: String,
}

#[derive(Debug, Deserialize)]
struct RawVersion {
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    status: String,
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawTraffic {
    #[serde(default, deserialize_with = "null_as_default")]
    version_name: String,
    #[serde(default)]
    is_latest_version: bool,
    #[serde(default)]
    percent: i32,
}

/// AppRun API がエラー時に返す JSON。
///
/// AppRun 自身の形式（`message` / `detail`）に加え、認証で弾かれた場合は
/// IaaS 側と同じ形式（`error_msg` / `error_code`）で返ってくる。
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
    /// 表示に使う「概要」と「詳細」を、どちらの形式からでも取り出す。
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
pub struct AppRunClient {
    http: reqwest::Client,
    token: String,
    secret: String,
    api_root: String,
}

impl AppRunClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        let http = crate::http::client()?;
        Ok(Self {
            http,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
            api_root: api_root(creds),
        })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}{path}", self.api_root);
        let res = crate::http::send_with_retry(&self.http, || {
            let mut req = self
                .http
                .request(method.clone(), &url)
                .basic_auth(&self.token, Some(&self.secret))
                .query(query);
            if let Some(body) = &body {
                req = req.json(body);
            }
            Ok(req.build()?)
        })
        .await
        .context("AppRun APIへのリクエストに失敗しました")?;
        let status = res.status();
        let text = res
            .text()
            .await
            .context("AppRun APIのレスポンス読み取りに失敗しました")?;

        if !status.is_success() {
            bail!("{}", format_api_error(status, &text));
        }
        let text = if text.trim().is_empty() { "{}" } else { &text };
        serde_json::from_str(text).with_context(|| {
            let head: String = text.chars().take(200).collect();
            format!("AppRun APIのレスポンス解析に失敗しました: {head}")
        })
    }

    /// アプリケーションを全件取得する。
    pub async fn list_applications(&self) -> Result<Vec<Application>> {
        let mut out = Vec::new();
        for page in (1usize..).take(MAX_PAGES) {
            let query = [
                ("page_num", page.to_string()),
                ("page_size", PAGE_SIZE.to_string()),
            ];
            let res: Paged<RawApplication> = self
                .request(Method::GET, "/applications", &query, None)
                .await?;
            let total = res.total();
            let items = res.items();
            let received = items.len();
            out.extend(items.into_iter().map(|app| Application {
                id: app.id,
                name: app.name,
                status: app.status,
                public_url: app.public_url,
                created_at: app.created_at,
            }));
            if received == 0 || out.len() >= total {
                break;
            }
        }
        Ok(out)
    }

    pub async fn application_detail(&self, id: &str) -> Result<ApplicationDetail> {
        let path = format!("/applications/{id}");
        let raw: RawApplicationDetail = self.request(Method::GET, &path, &[], None).await?;
        Ok(ApplicationDetail {
            port: raw.port,
            timeout_seconds: raw.timeout_seconds,
            min_scale: raw.min_scale,
            max_scale: raw.max_scale,
            images: raw
                .components
                .into_iter()
                .filter_map(|c| c.deploy_source?.container_registry)
                .map(|source| source.image)
                .filter(|image| !image.is_empty())
                .collect(),
        })
    }

    pub async fn list_versions(&self, id: &str) -> Result<Vec<Version>> {
        let path = format!("/applications/{id}/versions");
        let query = [("page_size", PAGE_SIZE.to_string())];
        let res: Paged<RawVersion> = self.request(Method::GET, &path, &query, None).await?;
        Ok(res
            .items()
            .into_iter()
            .map(|v| Version {
                name: v.name,
                status: v.status,
                created_at: v.created_at,
            })
            .collect())
    }

    pub async fn list_traffics(&self, id: &str) -> Result<Vec<Traffic>> {
        let path = format!("/applications/{id}/traffics");
        let res: Paged<RawTraffic> = self.request(Method::GET, &path, &[], None).await?;
        Ok(res
            .items()
            .into_iter()
            .map(|t| Traffic {
                version_name: t.version_name,
                is_latest: t.is_latest_version,
                percent: t.percent,
            })
            .collect())
    }

    /// 指定バージョンにトラフィックを 100% 振り向ける。
    pub async fn route_all_traffic(&self, id: &str, version_name: &str) -> Result<()> {
        let path = format!("/applications/{id}/traffics");
        let body = serde_json::json!([{ "version_name": version_name, "percent": 100 }]);
        let _: serde_json::Value = self.request(Method::PUT, &path, &[], Some(body)).await?;
        Ok(())
    }
}

/// 403 のときに添える案内。
///
/// AppRun は「APIキーにAppRun権限を付ける」「AppRunのユーザーを作る」の
/// 2つが前提で、どちらが欠けても 403 になる。エラーだけでは切り分けられないため、
/// 両方を案内する。
const FORBIDDEN_HINT: &str = "\n\nAppRun API には次の2つが必要です:\n\
     1. APIキーの作成時に「AppRun」の権限にチェックが入っていること\n\
        （アクセスレベルとは別の項目です）\n\
     2. AppRun のユーザーが作成済みであること\n\
        コントロールパネルで AppRun を一度開くか、次のコマンドで作成できます:\n\
        curl -X POST -u \"$SAKURA_ACCESS_TOKEN:$SAKURA_ACCESS_TOKEN_SECRET\" \\\n\
          https://secure.sakura.ad.jp/cloud/api/apprun/1.0/apprun/api/user";

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
            format!("AppRun APIエラー ({status}): {summary}{hint}")
        } else {
            format!("AppRun APIエラー ({status}): {summary} [{detail}]{hint}")
        };
    }
    let head: String = body.trim().chars().take(200).collect();
    if head.is_empty() {
        format!("AppRun APIエラー ({status}){hint}")
    } else {
        format!("AppRun APIエラー ({status}): {head}{hint}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_application_list() {
        let body = r#"{
            "meta": {"page_num": 1, "page_size": 100, "object_total": 1,
                     "sort_field": "created_at", "sort_order": "asc"},
            "data": [{
                "id": "abc-123",
                "name": "my-app",
                "status": "Success",
                "public_url": "https://my-app.apprun.sakura.ne.jp",
                "created_at": "2026-01-02T03:04:05Z"
            }]
        }"#;
        let res: Paged<RawApplication> = serde_json::from_str(body).unwrap();
        assert_eq!(res.total(), 1);
        let items = res.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "my-app");
    }

    /// コンテナレジストリのイメージ参照を取り出せること。
    #[test]
    fn extracts_container_images_from_components() {
        let body = r#"{
            "port": 8080, "timeout_seconds": 60, "min_scale": 0, "max_scale": 10,
            "components": [{
                "name": "main",
                "deploy_source": {
                    "container_registry": {
                        "image": "example.sakuracr.jp/app:v1",
                        "server": "example.sakuracr.jp"
                    }
                }
            }]
        }"#;
        let raw: RawApplicationDetail = serde_json::from_str(body).unwrap();
        let images: Vec<String> = raw
            .components
            .into_iter()
            .filter_map(|c| c.deploy_source?.container_registry)
            .map(|s| s.image)
            .collect();
        assert_eq!(images, vec!["example.sakuracr.jp/app:v1"]);
    }

    /// deploy_source が別種（アーカイブなど）でも落ちないこと。
    #[test]
    fn tolerates_components_without_registry_source() {
        let body = r#"{"components": [{"name": "main", "deploy_source": {}}]}"#;
        let raw: RawApplicationDetail = serde_json::from_str(body).unwrap();
        let images: Vec<String> = raw
            .components
            .into_iter()
            .filter_map(|c| c.deploy_source?.container_registry)
            .map(|s| s.image)
            .collect();
        assert!(images.is_empty());
    }

    #[test]
    fn parses_traffics() {
        let body = r#"{"data": [
            {"version_name": "v2", "is_latest_version": true, "percent": 80},
            {"version_name": "v1", "is_latest_version": false, "percent": 20}
        ]}"#;
        let res: Paged<RawTraffic> = serde_json::from_str(body).unwrap();
        let items = res.items();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_latest_version);
        assert_eq!(items[1].percent, 20);
    }

    #[test]
    fn formats_api_error() {
        let body = r#"{"message": "not found", "detail": "application does not exist"}"#;
        let message = format_api_error(StatusCode::NOT_FOUND, body);
        assert!(message.contains("not found"), "{message}");
        assert!(message.contains("does not exist"), "{message}");
    }

    /// 403 には前提条件の案内を添えること。
    #[test]
    fn forbidden_includes_setup_hint() {
        let body = r#"{"message": "要求された操作は許可されていません。権限エラー。",
            "detail": "forbidden"}"#;
        let message = format_api_error(StatusCode::FORBIDDEN, body);
        assert!(message.contains("権限エラー"), "{message}");
        assert!(message.contains("AppRun のユーザーが作成済み"), "{message}");
        assert!(message.contains("apprun/api/user"), "{message}");
    }

    /// 403 以外には案内を付けないこと。
    #[test]
    fn other_errors_have_no_setup_hint() {
        let body = r#"{"message": "not found"}"#;
        let message = format_api_error(StatusCode::NOT_FOUND, body);
        assert!(!message.contains("apprun/api/user"), "{message}");
    }

    /// 認証エラーは IaaS と同じ形式で返るので、生 JSON を出さないこと。
    #[test]
    fn formats_iaas_style_auth_error() {
        let body = r#"{"is_fatal":true,"serial":"abc","status":"401 Unauthorized",
            "error_code":"unauthorized","error_msg":"error-unauthorized"}"#;
        let message = format_api_error(StatusCode::UNAUTHORIZED, body);
        assert!(message.contains("error-unauthorized"), "{message}");
        assert!(
            !message.contains("is_fatal"),
            "生JSONが混ざっている: {message}"
        );
    }
}
