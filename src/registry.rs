//! Docker Registry HTTP API V2 クライアント。
//!
//! さくらのコンテナレジストリに登録されたイメージ（リポジトリとタグ）は
//! クラウド API からは取得できないため、レジストリの FQDN に対して
//! 直接 `/v2/` を叩く。認証は Bearer トークン方式・Basic 方式の両方に対応する。

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, WWW_AUTHENTICATE};
use reqwest::{Method, Response, StatusCode};
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::RegistryLogin;

/// 1 ページあたりの取得件数。
///
/// さくらのコンテナレジストリは大きすぎる値を `PAGINATION_NUMBER_INVALID` で
/// 拒否する。Docker Distribution の一般的な上限に合わせて 100 にしている。
const PAGE_SIZE: usize = 100;
/// 件数指定が拒否されたときのエラーコード。
const PAGINATION_ERROR: &str = "PAGINATION_NUMBER_INVALID";
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

/// イメージが対象とするプラットフォーム。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub architecture: String,
    pub variant: Option<String>,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.os, self.architecture)?;
        match &self.variant {
            Some(variant) if !variant.is_empty() => write!(f, "/{variant}"),
            _ => Ok(()),
        }
    }
}

/// 選択中のタグについて追加で取得する詳細。
#[derive(Debug, Clone, Default)]
pub struct TagDetail {
    pub digest: Option<String>,
    pub media_type: String,
    /// config + 全レイヤの合計バイト数（マニフェスト上の圧縮後サイズ）。
    pub size: Option<u64>,
    pub layers: Option<usize>,
    /// マルチアーキテクチャイメージなら複数返る。
    pub platforms: Vec<Platform>,
    pub created: Option<String>,
}

// --- マニフェストの JSON 表現 ---

#[derive(Debug, Default, Deserialize)]
struct RawManifest {
    #[serde(rename = "mediaType", default)]
    media_type: String,
    /// イメージインデックス（マルチアーキテクチャ）の場合のみ非空。
    #[serde(default)]
    manifests: Vec<RawIndexEntry>,
    config: Option<RawDescriptor>,
    #[serde(default)]
    layers: Vec<RawDescriptor>,
}

#[derive(Debug, Deserialize)]
struct RawDescriptor {
    #[serde(default)]
    digest: String,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Deserialize)]
struct RawIndexEntry {
    #[serde(default)]
    digest: String,
    platform: Option<RawPlatform>,
}

#[derive(Debug, Deserialize)]
struct RawPlatform {
    #[serde(default)]
    os: String,
    #[serde(default)]
    architecture: String,
    variant: Option<String>,
}

/// イメージの config blob（`architecture` などが入っている）。
#[derive(Debug, Deserialize)]
struct ImageConfig {
    created: Option<String>,
    #[serde(default)]
    architecture: String,
    #[serde(default)]
    os: String,
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
        let http = crate::http::client()?;
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

    /// 件数を指定して取得し、拒否されたら指定なしで取り直す。
    ///
    /// レジストリによって `n` に許される上限が違い、超えると
    /// `PAGINATION_NUMBER_INVALID` で 400 が返る。その場合はレジストリ既定の
    /// 件数に任せる（Link ヘッダを辿るので全件は取得できる）。
    async fn send_paginated(&self, url: &mut String, scope: &str) -> Result<Response> {
        match self.send(Method::GET, url, scope, None).await {
            Ok(res) => Ok(res),
            Err(err) if is_pagination_error(&err) && url.contains("?n=") => {
                *url = strip_page_size(url);
                self.send(Method::GET, url, scope, None).await
            }
            Err(err) => Err(err),
        }
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
            let res = self.send_paginated(&mut url, "registry:catalog:*").await?;
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
            let res = self.send_paginated(&mut url, &scope).await?;
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
        let res = self.send(Method::HEAD, &url, &scope, Some(headers)).await?;
        Ok(digest_header(&res))
    }

    /// 選択中のタグだけについて、サイズ・レイヤ数・プラットフォーム・ビルド日時を取る。
    ///
    /// 一覧取得時に全タグ分やると重いので、選択されたタグに対してのみ呼ぶ。
    pub async fn tag_detail(&self, repository: &str, tag: &str) -> Result<TagDetail> {
        let scope = format!("repository:{repository}:pull");
        let (digest, manifest) = self.fetch_manifest(repository, tag, &scope).await?;
        let mut detail = TagDetail {
            digest,
            media_type: manifest.media_type.clone(),
            ..TagDetail::default()
        };

        if manifest.manifests.is_empty() {
            // 単一アーキテクチャのイメージマニフェスト。
            self.fill_from_image_manifest(repository, &manifest, &scope, &mut detail)
                .await;
            return Ok(detail);
        }

        // イメージインデックス。プラットフォームを列挙し、代表 1 つの中身を見る。
        detail.platforms = manifest
            .manifests
            .iter()
            .filter_map(|entry| entry.platform.as_ref())
            // attestation manifest などは platform が unknown なので除く。
            .filter(|platform| platform.architecture != "unknown")
            .map(|platform| Platform {
                os: platform.os.clone(),
                architecture: platform.architecture.clone(),
                variant: platform.variant.clone(),
            })
            .collect();

        if let Some(child) = representative_entry(&manifest.manifests)
            && let Ok((_, child_manifest)) =
                self.fetch_manifest(repository, &child.digest, &scope).await
        {
            // プラットフォームはインデックス側の情報を優先する。
            let platforms = std::mem::take(&mut detail.platforms);
            self.fill_from_image_manifest(repository, &child_manifest, &scope, &mut detail)
                .await;
            detail.platforms = platforms;
        }
        Ok(detail)
    }

    /// イメージマニフェストからサイズ・レイヤ数を、config blob から日時などを埋める。
    async fn fill_from_image_manifest(
        &self,
        repository: &str,
        manifest: &RawManifest,
        scope: &str,
        detail: &mut TagDetail,
    ) {
        let config_size = manifest.config.as_ref().map_or(0, |c| c.size);
        let layers_size: u64 = manifest.layers.iter().map(|l| l.size).sum();
        detail.size = Some(config_size + layers_size);
        detail.layers = Some(manifest.layers.len());

        // config blob は付加情報なので、取れなくても他の情報は返す。
        let Some(config) = &manifest.config else {
            return;
        };
        let Ok(image) = self.fetch_config(repository, &config.digest, scope).await else {
            return;
        };
        detail.created = image.created;
        if detail.platforms.is_empty() && !image.architecture.is_empty() {
            detail.platforms.push(Platform {
                os: image.os,
                architecture: image.architecture,
                variant: None,
            });
        }
    }

    /// マニフェストを削除する。同じダイジェストを指す全てのタグが消える。
    ///
    /// レジストリ側で削除が無効化されていることがあるため、405 は専用のメッセージにする。
    pub async fn delete_manifest(&self, repository: &str, digest: &str) -> Result<()> {
        let url = format!(
            "https://{}/v2/{}/manifests/{}",
            self.host, repository, digest
        );
        let scope = format!("repository:{repository}:pull,delete");
        match self.send(Method::DELETE, &url, &scope, None).await {
            Ok(_) => Ok(()),
            Err(err) if err.to_string().contains("405") => bail!(
                "このレジストリではイメージの削除が有効になっていません (405 Method Not Allowed)"
            ),
            Err(err) => Err(err),
        }
    }

    /// マニフェストを取得し、`(ダイジェスト, 内容)` を返す。
    async fn fetch_manifest(
        &self,
        repository: &str,
        reference: &str,
        scope: &str,
    ) -> Result<(Option<String>, RawManifest)> {
        let url = format!(
            "https://{}/v2/{}/manifests/{}",
            self.host, repository, reference
        );
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static(MANIFEST_ACCEPT));
        let res = self.send(Method::GET, &url, scope, Some(headers)).await?;
        let digest = digest_header(&res);
        Ok((digest, json_body(res).await?))
    }

    async fn fetch_config(
        &self,
        repository: &str,
        digest: &str,
        scope: &str,
    ) -> Result<ImageConfig> {
        let url = format!("https://{}/v2/{}/blobs/{}", self.host, repository, digest);
        let res = self.send(Method::GET, &url, scope, None).await?;
        json_body(res).await
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
                .send_built(
                    self.build(method.clone(), url, headers.clone())
                        .header(AUTHORIZATION, format!("Bearer {token}")),
                )
                .await
                .context("レジストリへのリクエストに失敗しました")?;
            if res.status() != StatusCode::UNAUTHORIZED {
                return check_status(res).await;
            }
            self.tokens.lock().await.remove(scope);
        }

        // 認証なしで送り、返ってきたチャレンジに従って認証をやり直す。
        let res = self
            .send_built(self.build(method.clone(), url, headers.clone()))
            .await
            .context("レジストリへのリクエストに失敗しました")?;
        if res.status() != StatusCode::UNAUTHORIZED {
            return check_status(res).await;
        }

        let challenge = parse_challenge(&res)
            .context("レジストリの認証方式を判別できませんでした（WWW-Authenticate ヘッダなし）")?;
        let res = match challenge {
            Challenge::Basic => self
                .send_built(
                    self.build(method, url, headers)
                        .basic_auth(&self.login.username, Some(&self.login.password)),
                )
                .await
                .context("レジストリへのリクエストに失敗しました")?,
            Challenge::Bearer { realm, service } => {
                let token = self.fetch_token(&realm, service.as_deref(), scope).await?;
                let res = self
                    .send_built(
                        self.build(method, url, headers)
                            .header(AUTHORIZATION, format!("Bearer {token}")),
                    )
                    .await
                    .context("レジストリへのリクエストに失敗しました")?;
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

    /// リトライを挟んで送る。
    async fn send_built(&self, req: reqwest::RequestBuilder) -> Result<Response> {
        let request = req.build()?;
        crate::http::send_with_retry(&self.http, || {
            request
                .try_clone()
                .context("リクエストを複製できませんでした")
        })
        .await
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

        let res = self
            .send_built(req)
            .await
            .context("認証トークンの取得に失敗しました")?;
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

/// レジストリが返す正規のダイジェスト。
fn digest_header(res: &Response) -> Option<String> {
    res.headers()
        .get("docker-content-digest")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// イメージインデックスから、中身を代表して見に行くエントリを選ぶ。
/// linux/amd64 → linux/* → 先頭 の順で優先する。
fn representative_entry(entries: &[RawIndexEntry]) -> Option<&RawIndexEntry> {
    let usable = |entry: &&RawIndexEntry| {
        entry
            .platform
            .as_ref()
            .is_none_or(|p| p.architecture != "unknown")
    };
    entries
        .iter()
        .find(|entry| {
            entry
                .platform
                .as_ref()
                .is_some_and(|p| p.os == "linux" && p.architecture == "amd64")
        })
        .or_else(|| {
            entries
                .iter()
                .find(|entry| entry.platform.as_ref().is_some_and(|p| p.os == "linux"))
        })
        .or_else(|| entries.iter().find(usable))
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

/// Docker Registry v2 が返すエラー本文。
#[derive(Debug, Deserialize)]
struct RegistryErrors {
    #[serde(default)]
    errors: Vec<RegistryError>,
}

#[derive(Debug, Deserialize)]
struct RegistryError {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

/// エラー本文を `メッセージ [コード]` の形にする。
fn format_registry_errors(status: StatusCode, body: &str) -> String {
    let parsed = serde_json::from_str::<RegistryErrors>(body)
        .unwrap_or(RegistryErrors { errors: Vec::new() });
    let described: Vec<String> = parsed
        .errors
        .iter()
        .filter(|e| !e.message.is_empty() || !e.code.is_empty())
        .map(|e| match (e.message.is_empty(), e.code.is_empty()) {
            (false, false) => format!("{} [{}]", e.message, e.code),
            (false, true) => e.message.clone(),
            _ => e.code.clone(),
        })
        .collect();
    if !described.is_empty() {
        return format!("レジストリAPIエラー ({status}): {}", described.join(" / "));
    }
    let head: String = body.trim().chars().take(200).collect();
    if head.is_empty() {
        format!("レジストリAPIエラー ({status})")
    } else {
        format!("レジストリAPIエラー ({status}): {head}")
    }
}

/// 件数指定が拒否されたエラーか。
fn is_pagination_error(err: &anyhow::Error) -> bool {
    err.to_string().contains(PAGINATION_ERROR)
}

/// URL から `n=...` の指定だけを取り除く。他のクエリ（`last=` など）は残す。
fn strip_page_size(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let rest: Vec<&str> = query
        .split('&')
        .filter(|param| !param.starts_with("n="))
        .collect();
    if rest.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", rest.join("&"))
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
    bail!("{}", format_registry_errors(status, &body));
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
        let res = response(&[("link", r#"</v2/_catalog?n=200&last=foo>; rel="next""#)]);
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

#[cfg(test)]
mod detail_tests {
    use super::*;

    fn entry(os: &str, arch: &str) -> RawIndexEntry {
        RawIndexEntry {
            digest: format!("sha256:{os}-{arch}"),
            platform: Some(RawPlatform {
                os: os.to_string(),
                architecture: arch.to_string(),
                variant: None,
            }),
        }
    }

    #[test]
    fn prefers_linux_amd64_from_index() {
        let entries = vec![entry("linux", "arm64"), entry("linux", "amd64")];
        let picked = representative_entry(&entries).unwrap();
        assert_eq!(picked.digest, "sha256:linux-amd64");
    }

    #[test]
    fn falls_back_to_any_linux() {
        let entries = vec![entry("windows", "amd64"), entry("linux", "s390x")];
        let picked = representative_entry(&entries).unwrap();
        assert_eq!(picked.digest, "sha256:linux-s390x");
    }

    #[test]
    fn skips_attestation_entries() {
        // buildx の attestation manifest は platform が unknown/unknown。
        let entries = vec![entry("unknown", "unknown"), entry("windows", "amd64")];
        let picked = representative_entry(&entries).unwrap();
        assert_eq!(picked.digest, "sha256:windows-amd64");
    }

    #[test]
    fn parses_image_index() {
        let body = r#"{
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                {"digest": "sha256:aaa", "platform": {"os": "linux", "architecture": "amd64"}},
                {"digest": "sha256:bbb", "platform": {"os": "linux", "architecture": "arm64", "variant": "v8"}}
            ]
        }"#;
        let manifest: RawManifest = serde_json::from_str(body).unwrap();
        assert_eq!(manifest.manifests.len(), 2);
        assert!(manifest.config.is_none());
    }

    #[test]
    fn parses_image_manifest_sizes() {
        let body = r#"{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {"digest": "sha256:cfg", "size": 100},
            "layers": [{"digest": "sha256:l1", "size": 1000}, {"digest": "sha256:l2", "size": 2000}]
        }"#;
        let manifest: RawManifest = serde_json::from_str(body).unwrap();
        assert!(manifest.manifests.is_empty());
        assert_eq!(manifest.layers.len(), 2);
        let total: u64 = manifest.config.as_ref().unwrap().size
            + manifest.layers.iter().map(|l| l.size).sum::<u64>();
        assert_eq!(total, 3100);
    }

    #[test]
    fn platform_display_includes_variant() {
        let platform = Platform {
            os: "linux".into(),
            architecture: "arm64".into(),
            variant: Some("v8".into()),
        };
        assert_eq!(platform.to_string(), "linux/arm64/v8");
        let plain = Platform {
            os: "linux".into(),
            architecture: "amd64".into(),
            variant: None,
        };
        assert_eq!(plain.to_string(), "linux/amd64");
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;

    /// さくらのコンテナレジストリが実際に返したエラー本文。
    const SAKURA_PAGINATION_ERROR: &str = r#"{"errors":[{"code":"PAGINATION_NUMBER_INVALID","message":"invalid number of results requested","detail":{"n":200}}]}"#;

    /// 生の JSON ではなくメッセージとコードを出すこと。
    #[test]
    fn formats_registry_error_body() {
        let message = format_registry_errors(StatusCode::BAD_REQUEST, SAKURA_PAGINATION_ERROR);
        assert!(
            message.contains("invalid number of results requested"),
            "{message}"
        );
        assert!(message.contains(PAGINATION_ERROR), "{message}");
        assert!(
            !message.contains("{\"errors\""),
            "生JSONが混ざっている: {message}"
        );
    }

    #[test]
    fn detects_pagination_error() {
        let err = anyhow::anyhow!(
            "{}",
            format_registry_errors(StatusCode::BAD_REQUEST, SAKURA_PAGINATION_ERROR)
        );
        assert!(is_pagination_error(&err));

        let other = anyhow::anyhow!("レジストリの認証に失敗しました。");
        assert!(!is_pagination_error(&other));
    }

    /// `n` だけを外し、ページングの継続に必要な `last` は残すこと。
    #[test]
    fn strips_only_the_page_size() {
        assert_eq!(
            strip_page_size("https://r.example.jp/v2/_catalog?n=100"),
            "https://r.example.jp/v2/_catalog"
        );
        assert_eq!(
            strip_page_size("https://r.example.jp/v2/_catalog?n=100&last=foo"),
            "https://r.example.jp/v2/_catalog?last=foo"
        );
        assert_eq!(
            strip_page_size("https://r.example.jp/v2/app/tags/list?last=v1&n=100"),
            "https://r.example.jp/v2/app/tags/list?last=v1"
        );
        // クエリが無いものはそのまま。
        assert_eq!(
            strip_page_size("https://r.example.jp/v2/_catalog"),
            "https://r.example.jp/v2/_catalog"
        );
    }

    /// 空の errors 配列でも生 JSON を垂れ流さないこと。
    #[test]
    fn falls_back_to_body_excerpt() {
        let message = format_registry_errors(StatusCode::BAD_GATEWAY, "<html>oops</html>");
        assert!(message.contains("502"), "{message}");
        assert!(message.contains("oops"), "{message}");
    }
}
