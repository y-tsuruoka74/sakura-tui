//! さくらのクラウド API v1.1 クライアント（コンテナレジストリ関連）。
//!
//! コンテナレジストリは `commonserviceitem` リソースの一種で、`Provider.Class` が
//! `containerregistry` のものが該当する。ゾーンに依存しないグローバルリソースのため
//! 常に既定ゾーン `is1a` のエンドポイントを使う。

use std::fmt;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::ApiCredentials;

const API_ROOT: &str = "https://secure.sakura.ad.jp/cloud/zone";
/// グローバルリソース用の既定ゾーン。
const DEFAULT_ZONE: &str = "is1a";
const API_SUFFIX: &str = "api/cloud/1.1";
/// Find の 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 100;

/// さくらのクラウドのリソース ID。API は文字列でも数値でも返してくるため両方受ける。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId(pub u64);

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for ResourceId {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        match serde_json::Value::deserialize(de)? {
            serde_json::Value::String(s) => s.parse().map(ResourceId).map_err(D::Error::custom),
            serde_json::Value::Number(n) => n
                .as_u64()
                .map(ResourceId)
                .ok_or_else(|| D::Error::custom("IDが符号なし整数ではありません")),
            other => Err(D::Error::custom(format!("IDの型が不正です: {other}"))),
        }
    }
}

/// レジストリユーザーの権限。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "readwrite")]
    ReadWrite,
    #[serde(rename = "readonly")]
    ReadOnly,
}

impl Permission {
    pub const ALL: [Permission; 3] = [Permission::All, Permission::ReadWrite, Permission::ReadOnly];

    pub fn as_str(self) -> &'static str {
        match self {
            Permission::All => "all",
            Permission::ReadWrite => "readwrite",
            Permission::ReadOnly => "readonly",
        }
    }

    /// 日本語の説明（フォーム表示用）。
    pub fn description(self) -> &'static str {
        match self {
            Permission::All => "all (push/pull + ユーザー管理)",
            Permission::ReadWrite => "readwrite (push/pull)",
            Permission::ReadOnly => "readonly (pullのみ)",
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// コンテナレジストリ 1 件。
#[derive(Debug, Clone)]
pub struct ContainerRegistry {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub availability: String,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    /// 公開設定（`readonly` / `none`）。廃止予定の項目。
    pub access_level: String,
    pub virtual_domain: String,
    pub subdomain_label: String,
    pub fqdn: String,
}

impl ContainerRegistry {
    /// docker login / pull に使うホスト名。独自ドメイン設定があればそちらを優先する。
    pub fn host(&self) -> &str {
        if self.virtual_domain.is_empty() {
            &self.fqdn
        } else {
            &self.virtual_domain
        }
    }
}

/// レジストリに登録されたユーザー。
#[derive(Debug, Clone)]
pub struct RegistryUser {
    pub username: String,
    pub permission: Permission,
}

// --- API のレスポンス形状（naked 表現） ---

#[derive(Debug, Deserialize)]
struct NakedRegistry {
    #[serde(rename = "ID")]
    id: ResourceId,
    #[serde(rename = "Name", default)]
    name: String,
    #[serde(rename = "Description", default)]
    description: String,
    #[serde(rename = "Tags", default)]
    tags: Vec<String>,
    #[serde(rename = "Availability", default)]
    availability: String,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "ModifiedAt")]
    modified_at: Option<String>,
    #[serde(rename = "Settings")]
    settings: Option<NakedSettings>,
    #[serde(rename = "Status")]
    status: Option<NakedStatus>,
}

#[derive(Debug, Deserialize)]
struct NakedSettings {
    #[serde(rename = "ContainerRegistry")]
    container_registry: Option<NakedSetting>,
}

#[derive(Debug, Deserialize)]
struct NakedSetting {
    #[serde(default)]
    public: String,
    #[serde(default)]
    virtual_domain: String,
}

#[derive(Debug, Deserialize)]
struct NakedStatus {
    #[serde(default)]
    registry_name: String,
    #[serde(default)]
    hostname: String,
}

impl From<NakedRegistry> for ContainerRegistry {
    fn from(naked: NakedRegistry) -> Self {
        let setting = naked.settings.and_then(|s| s.container_registry);
        let status = naked.status;
        ContainerRegistry {
            id: naked.id,
            name: naked.name,
            description: naked.description,
            tags: naked.tags,
            availability: naked.availability,
            created_at: naked.created_at,
            modified_at: naked.modified_at,
            access_level: setting.as_ref().map(|s| s.public.clone()).unwrap_or_default(),
            virtual_domain: setting.map(|s| s.virtual_domain).unwrap_or_default(),
            subdomain_label: status
                .as_ref()
                .map(|s| s.registry_name.clone())
                .unwrap_or_default(),
            fqdn: status.map(|s| s.hostname).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FindResponse {
    #[serde(rename = "CommonServiceItems", default)]
    items: Vec<NakedRegistry>,
    #[serde(rename = "Total", default)]
    total: usize,
}

#[derive(Debug, Deserialize)]
struct ListUsersResponse {
    #[serde(rename = "ContainerRegistry")]
    container_registry: Option<NakedUsers>,
}

#[derive(Debug, Deserialize)]
struct NakedUsers {
    #[serde(default)]
    users: Vec<NakedUser>,
}

#[derive(Debug, Deserialize)]
struct NakedUser {
    #[serde(default)]
    username: String,
    permission: Option<Permission>,
}

/// API がエラー時に返す JSON。
#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default)]
    error_msg: String,
    #[serde(default)]
    error_code: String,
}

/// さくらのクラウド API クライアント。
#[derive(Debug)]
pub struct SacloudClient {
    http: reqwest::Client,
    token: String,
    secret: String,
    base: String,
}

impl SacloudClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("sakura-tui/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .context("HTTPクライアントの初期化に失敗しました")?;
        Ok(Self {
            http,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
            base: format!("{API_ROOT}/{DEFAULT_ZONE}/{API_SUFFIX}"),
        })
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let url = format!("{}/{}", self.base, path);
        let mut req = self
            .http
            .request(method, &url)
            .basic_auth(&self.token, Some(&self.secret));
        if let Some(body) = &body {
            req = req.json(body);
        }

        let res = req
            .send()
            .await
            .with_context(|| format!("APIリクエストに失敗しました: {url}"))?;
        let status = res.status();
        let text = res
            .text()
            .await
            .context("APIレスポンスの読み取りに失敗しました")?;

        if !status.is_success() {
            bail!("{}", format_api_error(status, &text));
        }
        // 204 など本文が無い場合は空 JSON として扱う。
        let text = if text.trim().is_empty() { "{}" } else { &text };
        serde_json::from_str(text).with_context(|| {
            let head: String = text.chars().take(200).collect();
            format!("APIレスポンスの解析に失敗しました: {head}")
        })
    }

    /// コンテナレジストリを全件取得する（ページングを辿る）。
    pub async fn list_registries(&self) -> Result<Vec<ContainerRegistry>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        loop {
            let body = json!({
                "Filter": { "Provider.Class": "containerregistry" },
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let res: FindResponse = self
                .request(Method::GET, "commonserviceitem", Some(body))
                .await?;
            let received = res.items.len();
            out.extend(res.items.into_iter().map(ContainerRegistry::from));
            // 進捗が無い場合に無限ループしないよう received == 0 で必ず抜ける。
            if received == 0 || out.len() >= res.total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    pub async fn list_users(&self, id: ResourceId) -> Result<Vec<RegistryUser>> {
        let path = format!("commonserviceitem/{id}/containerregistry/users");
        let res: ListUsersResponse = self.request(Method::GET, &path, None).await?;
        let users = res
            .container_registry
            .map(|c| c.users)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|u| {
                Some(RegistryUser {
                    username: u.username,
                    permission: u.permission?,
                })
            })
            .collect();
        Ok(users)
    }

    pub async fn add_user(
        &self,
        id: ResourceId,
        username: &str,
        password: &str,
        permission: Permission,
    ) -> Result<()> {
        let path = format!("commonserviceitem/{id}/containerregistry/users");
        let body = json!({
            "ContainerRegistry": {
                "username": username,
                "password": password,
                "permission": permission.as_str(),
            }
        });
        let _: serde_json::Value = self.request(Method::POST, &path, Some(body)).await?;
        Ok(())
    }

    /// ユーザーを更新する。`password` が `None` の場合はパスワードを送らず権限のみ変更する。
    pub async fn update_user(
        &self,
        id: ResourceId,
        username: &str,
        password: Option<&str>,
        permission: Permission,
    ) -> Result<()> {
        let path = format!("commonserviceitem/{id}/containerregistry/users/{username}");
        let mut payload = json!({ "permission": permission.as_str() });
        if let Some(password) = password {
            payload["password"] = json!(password);
        }
        let body = json!({ "ContainerRegistry": payload });
        let _: serde_json::Value = self.request(Method::PUT, &path, Some(body)).await?;
        Ok(())
    }

    pub async fn delete_user(&self, id: ResourceId, username: &str) -> Result<()> {
        let path = format!("commonserviceitem/{id}/containerregistry/users/{username}");
        let _: serde_json::Value = self.request(Method::DELETE, &path, None).await?;
        Ok(())
    }
}

/// エラーレスポンスから人間が読めるメッセージを組み立てる。
fn format_api_error(status: StatusCode, body: &str) -> String {
    if let Ok(err) = serde_json::from_str::<ApiError>(body)
        && !err.error_msg.is_empty()
    {
        return if err.error_code.is_empty() {
            format!("API エラー ({status}): {}", err.error_msg)
        } else {
            format!(
                "API エラー ({status}): {} [{}]",
                err.error_msg, err.error_code
            )
        };
    }
    let head: String = body.trim().chars().take(200).collect();
    if head.is_empty() {
        format!("API エラー ({status})")
    } else {
        format!("API エラー ({status}): {head}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_id_accepts_string_and_number() {
        // API は ID を文字列で返すことも数値で返すこともある。
        let from_string: ResourceId = serde_json::from_str("\"112900000000\"").unwrap();
        let from_number: ResourceId = serde_json::from_str("112900000000").unwrap();
        assert_eq!(from_string, ResourceId(112_900_000_000));
        assert_eq!(from_string, from_number);
    }

    #[test]
    fn parses_find_response() {
        let body = r#"{
            "Total": 1, "From": 0, "Count": 1,
            "CommonServiceItems": [{
                "ID": "112900000000",
                "Name": "example",
                "Description": "テスト",
                "Tags": ["a"],
                "Availability": "available",
                "CreatedAt": "2026-01-02T03:04:05+09:00",
                "Settings": {"ContainerRegistry": {"public": "none", "virtual_domain": ""}},
                "Status": {"registry_name": "example", "hostname": "example.sakuracr.jp"}
            }],
            "is_ok": true
        }"#;
        let parsed: FindResponse = serde_json::from_str(body).unwrap();
        let registry = ContainerRegistry::from(parsed.items.into_iter().next().unwrap());
        assert_eq!(registry.id, ResourceId(112_900_000_000));
        assert_eq!(registry.name, "example");
        assert_eq!(registry.fqdn, "example.sakuracr.jp");
        assert_eq!(registry.access_level, "none");
        // 独自ドメインが空なら FQDN を使う。
        assert_eq!(registry.host(), "example.sakuracr.jp");
    }

    #[test]
    fn virtual_domain_wins_over_fqdn() {
        let body = r#"{"ID": 1, "Settings": {"ContainerRegistry":
            {"public": "none", "virtual_domain": "registry.example.com"}},
            "Status": {"registry_name": "x", "hostname": "x.sakuracr.jp"}}"#;
        let naked: NakedRegistry = serde_json::from_str(body).unwrap();
        assert_eq!(ContainerRegistry::from(naked).host(), "registry.example.com");
    }

    #[test]
    fn parses_users_response() {
        let body = r#"{"ContainerRegistry": {"users": [
            {"username": "alice", "permission": "all"},
            {"username": "bob", "permission": "readonly"}
        ]}, "is_ok": true}"#;
        let parsed: ListUsersResponse = serde_json::from_str(body).unwrap();
        let users = parsed.container_registry.unwrap().users;
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].permission, Some(Permission::All));
        assert_eq!(users[1].permission, Some(Permission::ReadOnly));
    }

    #[test]
    fn formats_api_error_from_json() {
        // 実際の 401 レスポンス。
        let body = r#"{"is_fatal":true,"serial":"abc","status":"401 Unauthorized",
            "error_code":"unauthorized","error_msg":"error-unauthorized"}"#;
        let message = format_api_error(StatusCode::UNAUTHORIZED, body);
        assert!(message.contains("error-unauthorized"), "{message}");
        assert!(message.contains("unauthorized"), "{message}");
    }

    #[test]
    fn formats_api_error_from_non_json() {
        let message = format_api_error(StatusCode::BAD_GATEWAY, "<html>oops</html>");
        assert!(message.contains("502"), "{message}");
        assert!(message.contains("oops"), "{message}");
    }
}
