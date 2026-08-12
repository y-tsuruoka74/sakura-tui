//! Docker Registry HTTP API V2 クライアント。
//!
//! さくらのコンテナレジストリに登録されたイメージ（リポジトリとタグ）は
//! クラウド API からは取得できないため、レジストリの FQDN に対して
//! 直接 `/v2/` を叩く。認証は Bearer トークン方式・Basic 方式の両方に対応する。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, WWW_AUTHENTICATE};
use reqwest::{Method, Response, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::RegistryLogin;

/// 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 200;
/// マニフェスト情報を同時に取りに行く本数。
const MANIFEST_CONCURRENCY: usize = 8;

/// マニフェストの種類。新しい順に並べて Accept ヘッダに使う。
const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.index.v1+json, \
     application/vnd.oci.image.manifest.v1+json, \
     application/vnd.docker.distribution.manifest.list.v2+json, \
     application/vnd.docker.distribution.manifest.v2+json";

/// タグ 1 件の情報。
#[derive(Debug, Clone)]
pub struct TagInfo {
    pub name: String,
    /// マニフェストのダイジェスト。取得できなかった場合は `None`。
    pub digest: Option<String>,
}

/// 認証チャレンジの内容。
#[derive(Debug, Clone)]
enum Challenge {
    Bearer {
        realm: String,
        service: Option<String>,
    },
    Basic,
}

/// 1 つのレジストリホストに対するクライアント。
#[derive(Debug)]
pub struct RegistryClient {
    http: reqwest::Client,
    host: String,
    login: RegistryLogin,
    /// scope ごとの Bearer トークンのキャッシュ。
    tokens: Mutex<HashMap<String, String>>,
}

impl RegistryClient {
    pub fn new(host: &str, login: RegistryLogin) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("sakura-tui/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .context("HTTPクライアントの初期化に失敗しました")?;
        Ok(Self {
            http,
            host: host.to_string(),
            login,
            tokens: Mutex::new(HashMap::new()),
        })
    }

    /// 認証情報が有効かどうかを `/v2/` へのアクセスで確かめる。
    pub async fn verify(&self) -> Result<()> {
        let url = format!("https://{}/v2/", self.host);
        self.send(Method::GET, &url, "registry:catalog:*", None)
            .await?;
        Ok(())
    }

    /// リポジトリ一覧を取得する（Link ヘッダを辿って全件）。
    pub async fn list_repositories(&self) -> Result<Vec<String>> {
        #[derive(Deserialize)]
        struct Catalog {
            #[serde(default)]
            repositories: Vec<String>,
        }

        let mut url = format!("https://{}/v2/_catalog?n={PAGE_SIZE}", self.host);
        let mut out = Vec::new();
        loop {
            let res = self
                .send(Method::GET, &url, "registry:catalog:*", None)
                .await?;
            let next = next_page_url(&self.host, &res);
            let page: Catalog = json_body(res).await?;
            out.extend(page.repositories);
            match next {
                Some(next) => url = next,
                None => break,
            }
        }
        out.sort();
        Ok(out)
    }

    /// リポジトリのタグ一覧を取得し、各タグのダイジェストも並行して取りに行く。
    pub async fn list_tags(&self, repository: &str) -> Result<Vec<TagInfo>> {
        #[derive(Deserialize)]
        struct TagList {
            #[serde(default)]
            tags: Option<Vec<String>>,
        }

        let scope = format!("repository:{repository}:pull");
        let mut url = format!(
            "https://{}/v2/{}/tags/list?n={PAGE_SIZE}",
            self.host, repository
        );
        let mut names = Vec::new();
        loop {
            let res = self.send(Method::GET, &url, &scope, None).await?;
            let next = next_page_url(&self.host, &res);
            let page: TagList = json_body(res).await?;
            names.extend(page.tags.unwrap_or_default());
            match next {
                Some(next) => url = next,
                None => break,
            }
        }
        names.sort();

        // ダイジェストは付加情報なので、取得に失敗しても一覧自体は返す。
        let digests: Vec<TagInfo> = futures::stream::iter(names.into_iter().map(|name| async {
            let digest = self.manifest_digest(repository, &name).await.ok().flatten();
            TagInfo { name, digest }
        }))
        .buffered(MANIFEST_CONCURRENCY)
        .collect()
        .await;
        Ok(digests)
    }

    /// タグのマニフェストダイジェストを HEAD で取得する。
    async fn manifest_digest(&self, repository: &str, tag: &str) -> Result<Option<String>> {
        let url = format!("https://{}/v2/{}/manifests/{}", self.host, repository, tag);
        let scope = format!("repository:{repository}:pull");
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(MANIFEST_ACCEPT));
        let res = self
            .send(Method::HEAD, &url, &scope, Some(headers))
            .await?;
        Ok(res
            .headers()
            .get("docker-content-digest")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()))
    }

    /// 認証チャレンジに応じてトークンを取得しつつリクエストを送る。
    async fn send(
        &self,
        method: Method,
        url: &str,
        scope: &str,
        headers: Option<HeaderMap>,
    ) -> Result<Response> {
        let cached = self.tokens.lock().await.get(scope).cloned();

        // キャッシュ済みトークンがあればまずそれで試す。
        if let Some(token) = cached {
            let res = self
                .build(method.clone(), url, headers.clone())
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .send()
                .await
                .with_context(|| format!("レジストリへのリクエストに失敗しました: {url}"))?;
            if res.status() != StatusCode::UNAUTHORIZED {
                return check_status(res).await;
            }
            self.tokens.lock().await.remove(scope);
        }

        // 認証なしで送り、返ってきたチャレンジに従って認証をやり直す。
        let res = self
            .build(method.clone(), url, headers.clone())
            .send()
            .await
            .with_context(|| format!("レジストリへのリクエストに失敗しました: {url}"))?;
        if res.status() != StatusCode::UNAUTHORIZED {
            return check_status(res).await;
        }

        let challenge = parse_challenge(&res)
            .context("レジストリの認証方式を判別できませんでした（WWW-Authenticate ヘッダなし）")?;
        let res = match challenge {
            Challenge::Basic => {
                self.build(method, url, headers)
                    .basic_auth(&self.login.username, Some(&self.login.password))
                    .send()
                    .await
                    .with_context(|| format!("レジストリへのリクエストに失敗しました: {url}"))?
            }
            Challenge::Bearer { realm, service } => {
                let token = self.fetch_token(&realm, service.as_deref(), scope).await?;
                let res = self
                    .build(method, url, headers)
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .send()
                    .await
                    .with_context(|| format!("レジストリへのリクエストに失敗しました: {url}"))?;
                self.tokens.lock().await.insert(scope.to_string(), token);
                res
            }
        };
        check_status(res).await
    }

    fn build(
        &self,
        method: Method,
        url: &str,
        headers: Option<HeaderMap>,
    ) -> reqwest::RequestBuilder {
        let mut req = self.http.request(method, url);
        if let Some(headers) = headers {
            req = req.headers(headers);
        }
        req
    }

    /// 認証サーバーから Bearer トークンを取得する。
    async fn fetch_token(&self, realm: &str, service: Option<&str>, scope: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct TokenResponse {
            token: Option<String>,
            access_token: Option<String>,
        }

        let mut req = self
            .http
            .get(realm)
            .query(&[("scope", scope)])
            .basic_auth(&self.login.username, Some(&self.login.password));
        if let Some(service) = service {
            req = req.query(&[("service", service)]);
        }

        let res = req
            .send()
            .await
            .with_context(|| format!("認証トークンの取得に失敗しました: {realm}"))?;
        if res.status() == StatusCode::UNAUTHORIZED || res.status() == StatusCode::FORBIDDEN {
            bail!("レジストリの認証に失敗しました。ユーザー名とパスワードを確認してください。");
        }
        let res = check_status(res).await?;
        let body: TokenResponse = json_body(res).await?;
        body.token
            .or(body.access_token)
            .context("認証レスポンスにトークンが含まれていません")
    }
}

/// `WWW-Authenticate` ヘッダを解析する。
fn parse_challenge(res: &Response) -> Option<Challenge> {
    let raw = res.headers().get(WWW_AUTHENTICATE)?.to_str().ok()?;
    let (scheme, rest) = raw.split_once(' ').unwrap_or((raw, ""));
    if scheme.eq_ignore_ascii_case("basic") {
        return Some(Challenge::Basic);
    }
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }

    let mut realm = None;
    let mut service = None;
    for part in rest.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim().to_ascii_lowercase().as_str() {
            "realm" => realm = Some(value),
            "service" => service = Some(value),
            _ => {}
        }
    }
    Some(Challenge::Bearer {
        realm: realm?,
        service,
    })
}

/// ページングの `Link: <...>; rel="next"` ヘッダから次ページの URL を作る。
fn next_page_url(host: &str, res: &Response) -> Option<String> {
    let link = res.headers().get(reqwest::header::LINK)?.to_str().ok()?;
    let raw = link.split(';').next()?.trim();
    let path = raw.strip_prefix('<')?.strip_suffix('>')?;
    if path.starts_with("http://") || path.starts_with("https://") {
        Some(path.to_string())
    } else {
        Some(format!("https://{host}{path}"))
    }
}

async fn check_status(res: Response) -> Result<Response> {
    let status = res.status();
    if status.is_success() {
        return Ok(res);
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        bail!("レジストリの認証に失敗しました。ユーザー名とパスワードを確認してください。");
    }
    if status == StatusCode::NOT_FOUND {
        bail!("レジストリに該当のリソースがありません ({status})");
    }
    let body = res.text().await.unwrap_or_default();
    let head: String = body.trim().chars().take(200).collect();
    if head.is_empty() {
        bail!("レジストリAPIエラー ({status})");
    }
    bail!("レジストリAPIエラー ({status}): {head}");
}

async fn json_body<T: serde::de::DeserializeOwned>(res: Response) -> Result<T> {
    let text = res
        .text()
        .await
        .context("レジストリのレスポンス読み取りに失敗しました")?;
    serde_json::from_str(&text).with_context(|| {
        let head: String = text.chars().take(200).collect();
        format!("レジストリのレスポンス解析に失敗しました: {head}")
    })
}

/// FQDN ごとの `RegistryClient` を使い回すためのキャッシュ。
#[derive(Debug, Default)]
pub struct RegistryClients {
    clients: HashMap<String, Arc<RegistryClient>>,
}

impl RegistryClients {
    pub fn get(&self, host: &str) -> Option<Arc<RegistryClient>> {
        self.clients.get(host).cloned()
    }

    pub fn insert(&mut self, host: &str, login: RegistryLogin) -> Result<Arc<RegistryClient>> {
        let client = Arc::new(RegistryClient::new(host, login)?);
        self.clients.insert(host.to_string(), client.clone());
        Ok(client)
    }

    pub fn remove(&mut self, host: &str) {
        self.clients.remove(host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(headers: &[(&str, &str)]) -> Response {
        let mut builder = http::Response::builder().status(401);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        Response::from(builder.body("").unwrap())
    }

    #[test]
    fn parses_bearer_challenge() {
        let res = response(&[(
            "www-authenticate",
            r#"Bearer realm="https://auth.example.com/token",service="registry.example.com",scope="registry:catalog:*""#,
        )]);
        match parse_challenge(&res).unwrap() {
            Challenge::Bearer { realm, service } => {
                assert_eq!(realm, "https://auth.example.com/token");
                assert_eq!(service.as_deref(), Some("registry.example.com"));
            }
            other => panic!("Bearer を期待したが {other:?}"),
        }
    }

    #[test]
    fn parses_basic_challenge() {
        let res = response(&[("www-authenticate", r#"Basic realm="Registry Realm""#)]);
        assert!(matches!(parse_challenge(&res), Some(Challenge::Basic)));
    }

    #[test]
    fn no_challenge_header_yields_none() {
        assert!(parse_challenge(&response(&[])).is_none());
    }

    #[test]
    fn resolves_relative_next_page_link() {
        let res = response(&[(
            "link",
            r#"</v2/_catalog?n=200&last=foo>; rel="next""#,
        )]);
        assert_eq!(
            next_page_url("registry.example.com", &res).as_deref(),
            Some("https://registry.example.com/v2/_catalog?n=200&last=foo")
        );
    }

    #[test]
    fn keeps_absolute_next_page_link() {
        let res = response(&[(
            "link",
            r#"<https://other.example.com/v2/_catalog?last=foo>; rel="next""#,
        )]);
        assert_eq!(
            next_page_url("registry.example.com", &res).as_deref(),
            Some("https://other.example.com/v2/_catalog?last=foo")
        );
    }

    #[test]
    fn no_link_header_ends_pagination() {
        assert!(next_page_url("registry.example.com", &response(&[])).is_none());
    }
}
