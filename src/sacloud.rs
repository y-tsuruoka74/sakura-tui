//! さくらのクラウド API v1.1 クライアント（コンテナレジストリ関連）。
//!
//! コンテナレジストリは `commonserviceitem` リソースの一種で、`Provider.Class` が
//! `containerregistry` のものが該当する。ゾーンに依存しないグローバルリソースのため
//! 常に既定ゾーン `is1a` のエンドポイントを使う。

use std::fmt;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::{ApiCredentials, CredentialSource, IamCredentials};

/// プロファイルにゾーン指定が無い場合の既定ゾーン。
const DEFAULT_ZONE: &str = "is1a";
const API_SUFFIX: &str = "api/cloud/1.1";
/// 課金関連のエンドポイント接尾辞。
pub(crate) const BILLING_SUFFIX: &str = "api/system/1.0";
/// `commonserviceitem` のうちコンテナレジストリを表す `Provider.Class`。
const REGISTRY_CLASS: &str = "containerregistry";
/// Find の 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が実態と違う総件数を返しても止まるようにする。
const MAX_PAGES: usize = 100;

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
    /// 更新時に送り返して、他所での変更を上書きしないようにする。
    pub settings_hash: String,
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
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
    #[serde(rename = "Availability", default, deserialize_with = "null_as_default")]
    availability: String,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "ModifiedAt")]
    modified_at: Option<String>,
    #[serde(rename = "Settings")]
    settings: Option<NakedSettings>,
    #[serde(rename = "SettingsHash", default)]
    settings_hash: String,
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
    #[serde(default, deserialize_with = "null_as_default")]
    public: String,
    #[serde(default, deserialize_with = "null_as_default")]
    virtual_domain: String,
}

#[derive(Debug, Deserialize)]
struct NakedStatus {
    #[serde(default, deserialize_with = "null_as_default")]
    registry_name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    hostname: String,
}

/// JSON の `null` を型の既定値として受け取る。
///
/// さくらのクラウド API は未設定の項目をキーごと省くこともあれば `null` で返すこともあり、
/// `#[serde(default)]` だけでは後者で失敗するため。
pub(crate) fn null_as_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

/// 数値のはずの項目を、文字列で返されても受け取る。
///
/// さくらの API は OpenAPI 上 integer と書かれていても実際には
/// `"113701924793"` のように文字列で返すことがある。
pub(crate) fn flexible_number<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + std::str::FromStr + TryFrom<i64> + TryFrom<u64>,
{
    use serde::de::Error as _;
    let Some(value) = Option::<serde_json::Value>::deserialize(de)? else {
        return Ok(T::default());
    };
    match value {
        serde_json::Value::Null => Ok(T::default()),
        serde_json::Value::String(s) if s.is_empty() => Ok(T::default()),
        serde_json::Value::String(s) => s
            .parse()
            .map_err(|_| D::Error::custom(format!("数値として解釈できません: {s}"))),
        serde_json::Value::Number(n) => {
            if let Some(v) = n.as_i64() {
                T::try_from(v).map_err(|_| D::Error::custom(format!("範囲外の数値です: {v}")))
            } else if let Some(v) = n.as_u64() {
                T::try_from(v).map_err(|_| D::Error::custom(format!("範囲外の数値です: {v}")))
            } else {
                Err(D::Error::custom(format!("整数ではありません: {n}")))
            }
        }
        other => Err(D::Error::custom(format!("数値の型が不正です: {other}"))),
    }
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
            access_level: setting
                .as_ref()
                .map(|s| s.public.clone())
                .unwrap_or_default(),
            virtual_domain: setting.map(|s| s.virtual_domain).unwrap_or_default(),
            subdomain_label: status
                .as_ref()
                .map(|s| s.registry_name.clone())
                .unwrap_or_default(),
            fqdn: status.map(|s| s.hostname).unwrap_or_default(),
            settings_hash: naked.settings_hash,
        }
    }
}

/// `commonserviceitem` は DNS・GSLB・シンプル監視などと共用のエンドポイントなので、
/// 各項目をいったん生の JSON で受けて、コンテナレジストリだけを取り出す。
#[derive(Debug, Deserialize)]
struct FindResponse {
    #[serde(
        rename = "CommonServiceItems",
        default,
        deserialize_with = "null_as_default"
    )]
    items: Vec<serde_json::Value>,
    #[serde(rename = "Total", default)]
    total: usize,
}

/// コンテナレジストリの項目か。
///
/// `Provider.Class` を第一の判断材料にするが、レスポンスに `Provider` が
/// 含まれない場合に全件落としてしまわないよう、`Settings.ContainerRegistry` の
/// 有無でも判定する。
fn is_container_registry(item: &serde_json::Value) -> bool {
    let class = item
        .get("Provider")
        .and_then(|provider| provider.get("Class"))
        .and_then(serde_json::Value::as_str);
    if let Some(class) = class {
        return class == REGISTRY_CLASS;
    }
    item.get("Settings")
        .and_then(|settings| settings.get("ContainerRegistry"))
        .is_some_and(|value| !value.is_null())
}

/// 作成・取得時の単体レスポンス。
#[derive(Debug, Deserialize)]
struct ItemResponse {
    #[serde(rename = "CommonServiceItem")]
    item: Option<serde_json::Value>,
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
///
/// 形式が2種類ある。さくらのゲートウェイが返す認証・権限エラーは
/// `error_msg` / `error_code` だが、セキュリティコントロールのように
/// サービス自身が RFC 7807 の `title` / `detail` を返すものもある。
/// 片方しか見ないと、もう片方で生のJSONがそのまま画面に出てしまう。
#[derive(Debug, Deserialize)]
struct ApiError {
    #[serde(default, deserialize_with = "null_as_default")]
    error_msg: String,
    #[serde(default, deserialize_with = "null_as_default")]
    error_code: String,
    #[serde(default, deserialize_with = "null_as_default")]
    title: String,
    #[serde(default, deserialize_with = "null_as_default")]
    detail: String,
}

impl ApiError {
    /// 利用者に見せる本文。
    fn message(&self) -> &str {
        if self.error_msg.is_empty() {
            &self.title
        } else {
            &self.error_msg
        }
    }

    /// 本文に併記する識別子。無ければ空。
    fn code(&self) -> &str {
        if self.error_code.is_empty() {
            &self.detail
        } else {
            &self.error_code
        }
    }
}

/// さくらのクラウド API クライアント。
#[derive(Debug)]
pub struct SacloudClient {
    http: reqwest::Client,
    token: String,
    secret: String,
    default_zone: String,
    /// API のルート URL（末尾にスラッシュを含まない）。環境ごとに変わる。
    api_root: String,
    credential_source: CredentialSource,
    iam_token: tokio::sync::Mutex<Option<CachedIamToken>>,
}

#[derive(Debug)]
struct CachedIamToken {
    value: String,
    fingerprint: u64,
    expires_at: Instant,
}

fn global_api_root(api_root: &str) -> &str {
    api_root.strip_suffix("/zone").unwrap_or(api_root)
}

impl SacloudClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        let http = crate::http::client()?;
        Ok(Self {
            http,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
            default_zone: creds
                .zone
                .clone()
                .unwrap_or_else(|| DEFAULT_ZONE.to_string()),
            api_root: creds.api_root().to_string(),
            credential_source: creds.source.clone(),
            iam_token: tokio::sync::Mutex::new(None),
        })
    }

    /// プロファイルに書かれた既定ゾーン（無ければ `is1a`）。
    pub fn default_zone(&self) -> &str {
        &self.default_zone
    }

    /// 接続先の API ルート。
    pub fn api_root(&self) -> &str {
        &self.api_root
    }

    /// ゾーンに依存しないリソース向け（他モジュールから使う入口）。
    ///
    /// ゾーンに依存しないリソースでも URL にはゾーンが要る。どのゾーン経由でも
    /// 同じ結果になるので、その環境に確実に存在する既定ゾーンを使う。
    /// （本番以外の環境ではゾーン名が違うため、`is1a` の決め打ちはできない）
    pub(crate) async fn request_common<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        self.request_in_zone(&self.default_zone, method, path, body)
            .await
    }

    /// ゾーンに依存しないリソース向け。
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        self.request_common(method, path, body).await
    }

    /// ゾーンを指定して呼ぶ。
    pub(crate) async fn request_in_zone<T: DeserializeOwned>(
        &self,
        zone: &str,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        self.request_with_suffix(zone, API_SUFFIX, method, path, body)
            .await
    }

    /// エンドポイントの接尾辞まで指定して呼ぶ。
    ///
    /// 課金関連は IaaS とは別の接尾辞（`api/system/1.0`）にぶら下がっている。
    pub(crate) async fn request_with_suffix<T: DeserializeOwned>(
        &self,
        zone: &str,
        suffix: &str,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let base = format!("{}/{zone}/{suffix}", self.api_root);
        let mut url = reqwest::Url::parse(&format!("{base}/{path}"))
            .with_context(|| format!("URLの組み立てに失敗しました: {base}/{path}"))?;

        // さくらのクラウド API は GET のリクエストボディを読まない。
        // 検索条件は JSON をそのままクエリ文字列に載せて渡す。
        let send_as_query = method == Method::GET;
        if send_as_query && let Some(body) = &body {
            url.set_query(Some(&serde_json::to_string(body)?));
        }

        let res = crate::http::send_with_retry(&self.http, || {
            let mut req = self
                .http
                .request(method.clone(), url.clone())
                .basic_auth(&self.token, Some(&self.secret));
            if !send_as_query && let Some(body) = &body {
                req = req.json(body);
            }
            Ok(req.build()?)
        })
        .await
        .context("さくらのクラウドAPIへのリクエストに失敗しました")?;
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

    /// ゾーン配下にある別系統のAPIを、普通のクエリ文字列で呼ぶ。
    ///
    /// IaaS API は検索条件を JSON にしてクエリへ丸ごと載せるが、
    /// セキュリティコントロールのように `?page_size=100&next=...` という
    /// 一般的な形を取るAPIもある。`request_global` はルートの末尾から `/zone`
    /// を落として組み立てるので、ゾーンは接尾辞の側に含めて渡す。
    pub(crate) async fn request_zoned_service<T: DeserializeOwned>(
        &self,
        zone: &str,
        suffix: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        self.request_global(
            Method::GET,
            &format!("zone/{zone}/{suffix}"),
            path,
            query,
            None,
        )
        .await
    }

    /// ゾーンをURLに含まないグローバルAPIを呼ぶ。
    ///
    /// IAM APIは `/cloud/api/iam/1.0` にあり、IaaS APIのような
    /// `/zone/{zone}` を経由しない。テスト環境のルートも維持できるよう、
    /// 設定済みのAPIルートから末尾の `/zone` だけを取り除いて組み立てる。
    pub(crate) async fn request_global_with_query<T: DeserializeOwned>(
        &self,
        suffix: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        self.request_global(Method::GET, suffix, path, query, None)
            .await
    }

    pub(crate) async fn request_global<T: DeserializeOwned>(
        &self,
        method: Method,
        suffix: &str,
        path: &str,
        query: &[(&str, String)],
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let root = global_api_root(&self.api_root);
        let base = format!("{root}/{suffix}");
        let mut url = reqwest::Url::parse(&format!("{base}/{path}"))
            .with_context(|| format!("URLの組み立てに失敗しました: {base}/{path}"))?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));

        let bearer = if suffix == "api/iam/1.0" {
            let credentials = crate::config::load_iam_credentials(&self.credential_source)?
                .context(
                    "IAM認証が未設定です。IAM画面で a キーを押し、サービスプリンシパルを設定してください",
                )?;
            Some(self.iam_access_token(&credentials).await?)
        } else {
            None
        };
        let res = crate::http::send_with_retry(&self.http, || {
            let mut request = self.http.request(method.clone(), url.clone());
            request = if let Some(token) = &bearer {
                request
                    .bearer_auth(token)
                    .header("X-Requested-With", "XMLHttpRequest")
            } else {
                request.basic_auth(&self.token, Some(&self.secret))
            };
            if let Some(body) = &body {
                request = request.json(body);
            }
            Ok(request.build()?)
        })
        .await
        .context("さくらのクラウドAPIへのリクエストに失敗しました")?;
        let status = res.status();
        let text = res
            .text()
            .await
            .context("APIレスポンスの読み取りに失敗しました")?;
        if !status.is_success() {
            bail!("{}", format_api_error(status, &text));
        }
        let text = if text.trim().is_empty() { "{}" } else { &text };
        serde_json::from_str(text).with_context(|| {
            let head: String = text.chars().take(200).collect();
            format!("APIレスポンスの解析に失敗しました: {head}")
        })
    }

    async fn iam_access_token(&self, credentials: &IamCredentials) -> Result<String> {
        let fingerprint = crate::iam_auth::credentials_fingerprint(credentials);
        let mut cached = self.iam_token.lock().await;
        if let Some(token) = cached.as_ref()
            && token.fingerprint == fingerprint
            && token.expires_at > Instant::now() + Duration::from_secs(60)
        {
            return Ok(token.value.clone());
        }
        let issued =
            crate::iam_auth::issue_access_token(&self.http, &self.api_root, credentials).await?;
        let value = issued.value.clone();
        *cached = Some(CachedIamToken {
            value: issued.value,
            fingerprint,
            expires_at: Instant::now()
                + Duration::from_secs(issued.expires_in.saturating_sub(30).max(1)),
        });
        Ok(value)
    }

    /// 入力されたIAMサービスプリンシパルの認証とユーザー参照権限を保存前に検証する。
    pub async fn verify_iam_credentials(&self, credentials: &IamCredentials) -> Result<()> {
        let token =
            crate::iam_auth::issue_access_token(&self.http, &self.api_root, credentials).await?;
        let root = global_api_root(&self.api_root);
        let url = format!("{root}/api/iam/1.0/compat/users?page=1&per_page=1");
        let response = self
            .http
            .get(url)
            .bearer_auth(token.value)
            .header("X-Requested-With", "XMLHttpRequest")
            .send()
            .await
            .context("IAMユーザー一覧の検証リクエストに失敗しました")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("IAMユーザー一覧の検証応答を読み取れませんでした")?;
        if !status.is_success() {
            let hint = if status == StatusCode::FORBIDDEN {
                "\nサービスプリンシパルへIDポリシーの「ID閲覧者」または「ID管理者」を付与してください。"
            } else {
                ""
            };
            bail!(
                "IAMユーザー一覧を取得できませんでした ({}): {}{hint}",
                status,
                body.chars().take(300).collect::<String>()
            );
        }
        Ok(())
    }

    /// コンテナレジストリを全件取得する（ページングを辿る）。
    pub async fn list_registries(&self) -> Result<Vec<ContainerRegistry>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        let mut fetched = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({
                "Filter": { "Provider.Class": REGISTRY_CLASS },
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let res: FindResponse = self
                .request(Method::GET, "commonserviceitem", Some(body))
                .await?;
            let received = res.items.len();
            // サーバ側フィルタが効かなかった場合に備えて、ここでも種別を確かめる。
            for item in res.items.into_iter().filter(is_container_registry) {
                let naked: NakedRegistry = serde_json::from_value(item)
                    .context("コンテナレジストリの解析に失敗しました")?;
                out.push(ContainerRegistry::from(naked));
            }
            // 進捗が無い場合に無限ループしないよう received == 0 で必ず抜ける。
            // `total` は絞り込み前の件数なので、受信件数の累計で判定する。
            fetched += received;
            if received == 0 || fetched >= res.total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    /// コンテナレジストリを作成する。
    ///
    /// `subdomain` は `<subdomain>.sakuracr.jp` になる部分で、作成後は変更できない。
    pub async fn create_registry(
        &self,
        name: &str,
        subdomain: &str,
        description: &str,
    ) -> Result<ContainerRegistry> {
        let body = json!({
            "CommonServiceItem": {
                "Name": name,
                "Description": description,
                "Tags": [],
                "Provider": { "Class": "containerregistry" },
                "Status": { "registry_name": subdomain },
                "Settings": {
                    "ContainerRegistry": {
                        // 公開設定は廃止予定のため常に非公開で作る。
                        "public": "none",
                        "virtual_domain": "",
                    }
                },
            }
        });
        let res: ItemResponse = self
            .request(Method::POST, "commonserviceitem", Some(body))
            .await?;
        let item = res
            .item
            .context("作成レスポンスにレジストリが含まれていません")?;
        let naked: NakedRegistry =
            serde_json::from_value(item).context("作成したレジストリの解析に失敗しました")?;
        Ok(ContainerRegistry::from(naked))
    }

    /// 名前・説明・独自ドメインを更新する。
    pub async fn update_registry(
        &self,
        registry: &ContainerRegistry,
        name: &str,
        description: &str,
        virtual_domain: &str,
    ) -> Result<()> {
        let path = format!("commonserviceitem/{}", registry.id);
        let body = json!({
            "CommonServiceItem": {
                "Name": name,
                "Description": description,
                "Tags": registry.tags,
                "Settings": {
                    "ContainerRegistry": {
                        "public": if registry.access_level.is_empty() {
                            "none"
                        } else {
                            registry.access_level.as_str()
                        },
                        "virtual_domain": virtual_domain,
                    }
                },
                // 他所での変更を上書きしないよう、読み込み時のハッシュを送る。
                "SettingsHash": registry.settings_hash,
            }
        });
        let _: serde_json::Value = self.request(Method::PUT, &path, Some(body)).await?;
        Ok(())
    }

    pub async fn delete_registry(&self, id: ResourceId) -> Result<()> {
        let path = format!("commonserviceitem/{id}");
        let _: serde_json::Value = self.request(Method::DELETE, &path, None).await?;
        Ok(())
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
        && !err.message().is_empty()
    {
        return if err.code().is_empty() {
            format!("API エラー ({status}): {}", err.message())
        } else {
            format!("API エラー ({status}): {} [{}]", err.message(), err.code())
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
        let item = parsed.items.into_iter().next().unwrap();
        let naked: NakedRegistry = serde_json::from_value(item).unwrap();
        let registry = ContainerRegistry::from(naked);
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
        assert_eq!(
            ContainerRegistry::from(naked).host(),
            "registry.example.com"
        );
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

    /// セキュリティコントロールなどは RFC 7807 形式で返す。
    /// 実際に crane74 で受け取ったレスポンス。
    #[test]
    fn formats_api_error_from_rfc7807_json() {
        let body = r#"{"title":"project is not activated","status":403}"#;
        let message = format_api_error(StatusCode::FORBIDDEN, body);
        assert!(message.contains("project is not activated"), "{message}");
        // 生のJSONがそのまま出ないこと。
        assert!(!message.contains('{'), "{message}");

        let with_detail = r#"{"type":"about:blank","title":"不適切な要求です。",
            "status":400,"detail":"invalid"}"#;
        let message = format_api_error(StatusCode::BAD_REQUEST, with_detail);
        assert!(message.contains("不適切な要求です。"), "{message}");
        assert!(message.contains("invalid"), "{message}");
    }

    /// さくら形式と RFC 7807 が混ざっていても、さくら側を優先する。
    #[test]
    fn sakura_error_fields_win_over_rfc7807() {
        let body = r#"{"error_msg":"さくら側","error_code":"forbidden",
            "title":"rfc側","detail":"d"}"#;
        let message = format_api_error(StatusCode::FORBIDDEN, body);
        assert!(message.contains("さくら側"), "{message}");
        assert!(!message.contains("rfc側"), "{message}");
    }

    #[test]
    fn formats_api_error_from_non_json() {
        let message = format_api_error(StatusCode::BAD_GATEWAY, "<html>oops</html>");
        assert!(message.contains("502"), "{message}");
        assert!(message.contains("oops"), "{message}");
    }
}

#[cfg(test)]
mod find_tests {
    use super::*;

    /// `commonserviceitem` は DNS などと共用なので、種別で絞れること。
    #[test]
    fn filters_out_other_common_service_items() {
        let body = r#"{
            "From": 0, "Count": 2, "Total": 2,
            "CommonServiceItems": [
                {
                    "ID": "113701924283",
                    "Name": "example.jp",
                    "Provider": {"Class": "dns"},
                    "Settings": {"DNS": {"ResourceRecordSets": [
                        {"Name": "app", "Type": "ALIAS", "RData": null}
                    ]}}
                },
                {
                    "ID": "112900000000",
                    "Name": "my-registry",
                    "Provider": {"Class": "containerregistry"},
                    "Settings": {"ContainerRegistry": {"public": "none", "virtual_domain": ""}},
                    "Status": {"registry_name": "my-registry", "hostname": "my-registry.sakuracr.jp"}
                }
            ]
        }"#;
        let res: FindResponse = serde_json::from_str(body).unwrap();
        assert_eq!(res.items.len(), 2, "生の項目は全件受け取る");

        let registries: Vec<ContainerRegistry> = res
            .items
            .into_iter()
            .filter(is_container_registry)
            .map(|item| {
                let naked: NakedRegistry = serde_json::from_value(item).unwrap();
                ContainerRegistry::from(naked)
            })
            .collect();
        assert_eq!(registries.len(), 1);
        assert_eq!(registries[0].name, "my-registry");
    }

    /// 未設定の項目が `null` で返ってきても既定値として受けられること。
    #[test]
    fn accepts_null_strings() {
        let body = r#"{
            "ID": 1,
            "Name": "x",
            "Description": null,
            "Tags": null,
            "Availability": null,
            "Settings": {"ContainerRegistry": {"public": null, "virtual_domain": null}},
            "Status": {"registry_name": null, "hostname": null}
        }"#;
        let naked: NakedRegistry = serde_json::from_str(body).unwrap();
        let registry = ContainerRegistry::from(naked);
        assert_eq!(registry.description, "");
        assert!(registry.tags.is_empty());
        assert_eq!(registry.availability, "");
        assert_eq!(registry.fqdn, "");
    }

    /// GET は検索条件をクエリ文字列で送る（ボディは読まれない）。
    #[test]
    fn get_puts_search_conditions_in_query_string() {
        let mut url = reqwest::Url::parse("https://example.com/api/commonserviceitem").unwrap();
        let body = json!({ "Filter": { "Provider.Class": REGISTRY_CLASS } });
        url.set_query(Some(&serde_json::to_string(&body).unwrap()));
        let query = url.query().unwrap();
        assert!(query.contains("Provider.Class"), "{query}");
        assert!(query.contains(REGISTRY_CLASS), "{query}");
    }

    #[test]
    fn global_api_root_removes_only_the_zone_suffix() {
        assert_eq!(
            global_api_root("https://secure.sakura.ad.jp/cloud/zone"),
            "https://secure.sakura.ad.jp/cloud"
        );
        assert_eq!(
            global_api_root("https://secure.sakura.ad.jp/cloud-test/zone"),
            "https://secure.sakura.ad.jp/cloud-test"
        );
        assert_eq!(
            global_api_root("https://example.com/custom"),
            "https://example.com/custom"
        );
    }
}

#[cfg(test)]
mod class_tests {
    use super::*;

    #[test]
    fn detects_registry_by_provider_class() {
        let item = serde_json::json!({"Provider": {"Class": "containerregistry"}});
        assert!(is_container_registry(&item));
    }

    #[test]
    fn rejects_other_classes() {
        let item = serde_json::json!({
            "Provider": {"Class": "dns"},
            "Settings": {"DNS": {"ResourceRecordSets": []}}
        });
        assert!(!is_container_registry(&item));
    }

    /// Provider が返らない場合でも Settings で判定できること。
    #[test]
    fn falls_back_to_settings_shape() {
        let registry = serde_json::json!({
            "Settings": {"ContainerRegistry": {"public": "none"}}
        });
        assert!(is_container_registry(&registry));

        let dns = serde_json::json!({"Settings": {"DNS": {"ResourceRecordSets": []}}});
        assert!(!is_container_registry(&dns));
    }

    #[test]
    fn rejects_items_without_any_signal() {
        assert!(!is_container_registry(&serde_json::json!({"ID": "1"})));
    }
}

#[cfg(test)]
mod number_tests {
    use super::*;

    #[derive(Deserialize)]
    struct Sample {
        #[serde(default, deserialize_with = "flexible_number")]
        value: i64,
        #[serde(default, deserialize_with = "flexible_number")]
        count: usize,
    }

    #[test]
    fn accepts_numbers_and_numeric_strings() {
        let from_number: Sample = serde_json::from_str(r#"{"value": 42, "count": 3}"#).unwrap();
        assert_eq!((from_number.value, from_number.count), (42, 3));

        let from_string: Sample =
            serde_json::from_str(r#"{"value": "113701924793", "count": "7"}"#).unwrap();
        assert_eq!((from_string.value, from_string.count), (113_701_924_793, 7));
    }

    #[test]
    fn missing_null_and_empty_become_default() {
        for body in [r#"{}"#, r#"{"value": null}"#, r#"{"value": ""}"#] {
            let parsed: Sample = serde_json::from_str(body).unwrap();
            assert_eq!(parsed.value, 0, "{body}");
        }
    }

    #[test]
    fn rejects_non_numeric_strings() {
        assert!(serde_json::from_str::<Sample>(r#"{"value": "abc"}"#).is_err());
    }
}
