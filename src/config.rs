//! 認証情報の読み込みと保存。
//!
//! さくらのクラウド API の認証情報は環境変数と usacloud プロファイルから読む
//! （sacloud/api-client-go と同じ優先順位）。コンテナレジストリ自体への
//! ログイン情報はクラウド API では取得できないため、独自の設定ファイルに置く。

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

/// 本番環境の API ルート。
pub const DEFAULT_API_ROOT: &str = "https://secure.sakura.ad.jp/cloud/zone";
/// 社内テスト環境の API ルート。
pub const TEST_API_ROOT: &str = "https://secure.sakura.ad.jp/cloud-test/zone";

/// 認証情報の出どころ。TUI 内で切り替えるための識別子でもある。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialSource {
    /// 環境変数 `SAKURA_ACCESS_TOKEN` / `SAKURA_ACCESS_TOKEN_SECRET`。
    Env,
    /// usacloud のプロファイル（`~/.usacloud/<名前>/config.json`）。
    ///
    /// usacloud CLI・Terraform・Packer と共用できるが、トークンは平文。
    Profile(String),
    /// この TUI 専用の資格情報。トークンは OS のキーチェーンに置く。
    ///
    /// 平文はどこにも残らないが、`~/.usacloud` を読む他のツールからは使えない。
    Keychain(String),
}

impl CredentialSource {
    /// ヘッダーやピッカーに出す表示名。
    pub fn label(&self) -> String {
        match self {
            CredentialSource::Env => "環境変数".to_string(),
            CredentialSource::Profile(name) | CredentialSource::Keychain(name) => name.clone(),
        }
    }

    /// 保存形式の呼び名（ピッカーで種別を見分けるため）。
    pub fn kind_label(&self) -> &'static str {
        match self {
            CredentialSource::Env => "環境変数",
            CredentialSource::Profile(_) => "usacloud",
            CredentialSource::Keychain(_) => "キーチェーン",
        }
    }

    /// 設定ファイルで色などを紐づけるときのキー。
    ///
    /// 環境変数はプロファイル名を持たないので `@env` を使う。
    /// usacloud のプロファイル名に `@` は使えないため衝突しない。
    pub fn config_key(&self) -> String {
        match self {
            CredentialSource::Env => "@env".to_string(),
            CredentialSource::Profile(name) => name.clone(),
            // usacloud 側と名前が衝突しても別物として扱えるようにする。
            CredentialSource::Keychain(name) => format!("@keychain:{name}"),
        }
    }

    /// プロファイルに設定された既定ゾーン。ピッカーで見分ける手がかりにする。
    pub fn zone(&self) -> Option<String> {
        match self {
            CredentialSource::Env => env_multi(&["SAKURA_ZONE", "SAKURACLOUD_ZONE"]),
            CredentialSource::Profile(name) => {
                load_usacloud_profile(Some(name)).ok().and_then(|c| c.zone)
            }
            CredentialSource::Keychain(name) => Config::load()
                .ok()?
                .credentials
                .get(name)
                .and_then(|c| c.zone.clone()),
        }
    }
}

/// さくらのクラウド API のアクセストークンとシークレット。
#[derive(Debug, Clone)]
pub struct ApiCredentials {
    pub token: String,
    pub secret: String,
    pub source: CredentialSource,
    /// プロファイルに書かれた既定ゾーン。
    pub zone: Option<String>,
    /// API のルート URL。環境（本番 / cloud-test など）を切り替えるのに使う。
    /// 未設定なら本番。
    pub api_root: Option<String>,
}

impl ApiCredentials {
    /// 実際に使う API ルート（末尾にスラッシュを含まない）。
    pub fn api_root(&self) -> &str {
        self.api_root
            .as_deref()
            .filter(|r| !r.is_empty())
            .unwrap_or(DEFAULT_API_ROOT)
    }
}

fn env_multi(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()))
}

/// 環境変数から認証情報を読む。両方揃っていなければ `None`。
fn credentials_from_env() -> Option<ApiCredentials> {
    let token = env_multi(&["SAKURA_ACCESS_TOKEN", "SAKURACLOUD_ACCESS_TOKEN"])?;
    let secret = env_multi(&[
        "SAKURA_ACCESS_TOKEN_SECRET",
        "SAKURACLOUD_ACCESS_TOKEN_SECRET",
    ])?;
    Some(ApiCredentials {
        token,
        secret,
        source: CredentialSource::Env,
        zone: env_multi(&["SAKURA_ZONE", "SAKURACLOUD_ZONE"]),
        api_root: env_multi(&["SAKURA_API_ROOT_URL", "SAKURACLOUD_API_ROOT_URL"]),
    })
}

/// 環境変数 → usacloud プロファイルの順で API 認証情報を探す。
///
/// `prefer_profile` が真（`--profile` 指定時）なら環境変数を飛ばして
/// プロファイルを見る。明示指定を環境変数に潰されないようにするため。
pub fn load_api_credentials(prefer_profile: bool) -> Result<ApiCredentials> {
    if !prefer_profile && let Some(creds) = credentials_from_env() {
        return Ok(creds);
    }

    let profile_name = env_multi(&["SAKURA_PROFILE", "SAKURACLOUD_PROFILE", "USACLOUD_PROFILE"]);
    match load_usacloud_profile(profile_name.as_deref()) {
        Ok(creds) => Ok(creds),
        Err(err) => Err(anyhow!(
            "APIの認証情報が見つかりませんでした。\n\
             環境変数 SAKURA_ACCESS_TOKEN / SAKURA_ACCESS_TOKEN_SECRET を設定するか、\n\
             `usacloud config` でプロファイルを作成してください。\n\
             (プロファイル読み込み時のエラー: {err})"
        )),
    }
}

/// `--profile` に渡された名前を、実在する認証元へ解決する。
///
/// usacloud のプロファイルとキーチェーンのどちらも名前で選べるようにする。
/// 同名が両方にあるときは、以前からの挙動を変えないよう usacloud を採る。
/// 明示したいときは `config_key()` と同じ `@keychain:<名前>` 形式が使える。
pub fn resolve_credential_source(name: &str) -> Result<CredentialSource> {
    if let Some(keychain) = name.strip_prefix("@keychain:") {
        return keychain_source(keychain);
    }
    if name == "@env" {
        return credentials_from_env()
            .map(|_| CredentialSource::Env)
            .context("環境変数に認証情報が設定されていません");
    }

    if list_usacloud_profiles().iter().any(|p| p == name) {
        return Ok(CredentialSource::Profile(name.to_string()));
    }
    if let Ok(source) = keychain_source(name) {
        return Ok(source);
    }

    let known: Vec<String> = available_credential_sources()
        .iter()
        .map(CredentialSource::label)
        .collect();
    if known.is_empty() {
        bail!("認証情報が1件もありません: {name}");
    }
    bail!(
        "{name} という認証情報は見つかりませんでした。\n\
         使えるのは: {}",
        known.join(" / ")
    )
}

/// 設定ファイルに名前が載っているキーチェーン資格情報を指す。
fn keychain_source(name: &str) -> Result<CredentialSource> {
    let config = Config::load().context("設定ファイルを読めませんでした")?;
    if config.credentials.contains_key(name) {
        Ok(CredentialSource::Keychain(name.to_string()))
    } else {
        bail!("キーチェーンに {name} という認証情報はありません")
    }
}

/// 指定の出どころから認証情報を読み直す（TUI 内での切り替え用）。
pub fn load_credentials_from(source: &CredentialSource) -> Result<ApiCredentials> {
    match source {
        CredentialSource::Env => {
            credentials_from_env().context("環境変数に認証情報が設定されていません")
        }
        CredentialSource::Profile(name) => load_usacloud_profile(Some(name)),
        CredentialSource::Keychain(name) => load_keychain_credentials(name),
    }
}

/// 現在のクラウド認証元に紐づくAI Engineアカウントトークンを読む。
///
/// 環境変数は一時利用やCI向けとして最優先し、アプリから登録した値はOSの
/// キーチェーンへ保存する。
pub const AI_ENGINE_ENV_TOKEN_NAME: &str = "環境変数";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiEngineTokenEntry {
    pub name: String,
    pub active: bool,
    pub from_env: bool,
}

/// IAM APIへ接続するサービスプリンシパルの認証情報。
///
/// 秘密鍵をログやDebug表示へ誤って出さないよう、Debugは識別情報だけを表示する。
#[derive(Clone, PartialEq, Eq)]
pub struct IamCredentials {
    pub service_principal_id: String,
    pub key_id: String,
    pub private_key: String,
}

impl std::fmt::Debug for IamCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IamCredentials")
            .field("service_principal_id", &self.service_principal_id)
            .field("key_id", &self.key_id)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

/// IAMサービスプリンシパルを読み込む。
///
/// CIでは公式SDKと同じ環境変数を優先し、対話利用では現在のクラウドAPI認証元に
/// 紐づけたキーチェーンの秘密鍵を使う。
pub fn load_iam_credentials(source: &CredentialSource) -> Result<Option<IamCredentials>> {
    let env_id = env_multi(&["SAKURA_SERVICE_PRINCIPAL_ID"]);
    let env_key_id = env_multi(&["SAKURA_SERVICE_PRINCIPAL_KEY_ID"]);
    let env_private_key = env_multi(&["SAKURA_PRIVATE_KEY"]);
    if env_id.is_some() || env_key_id.is_some() || env_private_key.is_some() {
        let service_principal_id =
            env_id.context("SAKURA_SERVICE_PRINCIPAL_ID が設定されていません")?;
        let key_id = env_key_id.context("SAKURA_SERVICE_PRINCIPAL_KEY_ID が設定されていません")?;
        let private_key = env_private_key.context("SAKURA_PRIVATE_KEY が設定されていません")?;
        return Ok(Some(IamCredentials {
            service_principal_id,
            key_id,
            private_key,
        }));
    }

    let config = Config::load()?;
    let profile_key = source.config_key();
    let Some(metadata) = config.iam_credentials.get(&profile_key) else {
        return Ok(None);
    };
    let private_key = crate::keychain::get_iam_private_key(&profile_key)?
        .context("IAMサービスプリンシパルの秘密鍵がキーチェーンにありません")?;
    Ok(Some(IamCredentials {
        service_principal_id: metadata.service_principal_id.clone(),
        key_id: metadata.key_id.clone(),
        private_key,
    }))
}

/// IAMサービスプリンシパルを現在のクラウドAPI認証元に紐づけて保存する。
pub fn save_iam_credentials(
    source: &CredentialSource,
    credentials: &IamCredentials,
) -> Result<PathBuf> {
    let profile_key = source.config_key();
    crate::keychain::set_iam_private_key(&profile_key, &credentials.private_key)?;
    let mut config = Config::load()?;
    config.iam_credentials.insert(
        profile_key,
        IamCredentialMetadata {
            service_principal_id: credentials.service_principal_id.clone(),
            key_id: credentials.key_id.clone(),
        },
    );
    config.save()
}

/// 現在選択中のAI Engineトークンを読む。
pub fn load_ai_engine_token(source: &CredentialSource) -> Result<Option<String>> {
    let mut config = Config::load()?;
    migrate_legacy_ai_engine_token(&mut config, source)?;
    let profile_key = source.config_key();
    if let Some(profile) = config.ai_engine_tokens.get(&profile_key)
        && let Some(active) = &profile.active
    {
        if active == AI_ENGINE_ENV_TOKEN_NAME {
            return Ok(env_multi(&["SAKURA_AI_ENGINE_TOKEN"]));
        }
        return crate::keychain::get_named_ai_engine_token(&profile_key, active);
    }
    if let Some(token) = env_multi(&["SAKURA_AI_ENGINE_TOKEN"]) {
        return Ok(Some(token));
    }
    Ok(None)
}

pub fn list_ai_engine_tokens(source: &CredentialSource) -> Result<Vec<AiEngineTokenEntry>> {
    let mut config = Config::load()?;
    migrate_legacy_ai_engine_token(&mut config, source)?;
    let profile_key = source.config_key();
    let profile = config.ai_engine_tokens.get(&profile_key);
    let active = profile.and_then(|profile| profile.active.as_deref());
    let mut entries = Vec::new();
    if env_multi(&["SAKURA_AI_ENGINE_TOKEN"]).is_some() {
        entries.push(AiEngineTokenEntry {
            name: AI_ENGINE_ENV_TOKEN_NAME.to_string(),
            active: active == Some(AI_ENGINE_ENV_TOKEN_NAME)
                || (active.is_none() && profile.is_none()),
            from_env: true,
        });
    }
    if let Some(profile) = profile {
        entries.extend(profile.names.iter().map(|name| AiEngineTokenEntry {
            name: name.clone(),
            active: active == Some(name.as_str()),
            from_env: false,
        }));
    }
    Ok(entries)
}

pub fn save_ai_engine_token(source: &CredentialSource, name: &str, token: &str) -> Result<()> {
    let name = validate_ai_engine_token_name(name)?;
    let profile_key = source.config_key();
    crate::keychain::set_named_ai_engine_token(&profile_key, name, token)?;
    let mut config = Config::load()?;
    let profile = config.ai_engine_tokens.entry(profile_key).or_default();
    if !profile.names.iter().any(|current| current == name) {
        profile.names.push(name.to_string());
        profile.names.sort();
    }
    profile.active = Some(name.to_string());
    config.save()?;
    Ok(())
}

pub fn select_ai_engine_token(source: &CredentialSource, name: &str) -> Result<Option<String>> {
    let profile_key = source.config_key();
    let token = if name == AI_ENGINE_ENV_TOKEN_NAME {
        env_multi(&["SAKURA_AI_ENGINE_TOKEN"])
            .context("環境変数 SAKURA_AI_ENGINE_TOKEN が設定されていません")?
    } else {
        crate::keychain::get_named_ai_engine_token(&profile_key, name)?
            .with_context(|| format!("キーチェーンにAI Engineトークン「{name}」がありません"))?
    };
    let mut config = Config::load()?;
    config
        .ai_engine_tokens
        .entry(profile_key)
        .or_default()
        .active = Some(name.to_string());
    config.save()?;
    Ok(Some(token))
}

pub fn load_named_ai_engine_token(source: &CredentialSource, name: &str) -> Result<Option<String>> {
    if name == AI_ENGINE_ENV_TOKEN_NAME {
        return Ok(env_multi(&["SAKURA_AI_ENGINE_TOKEN"]));
    }
    crate::keychain::get_named_ai_engine_token(&source.config_key(), name)
}

pub fn delete_ai_engine_token(source: &CredentialSource, name: &str) -> Result<()> {
    if name == AI_ENGINE_ENV_TOKEN_NAME {
        bail!("環境変数のトークンはアプリから削除できません");
    }
    let profile_key = source.config_key();
    crate::keychain::delete_named_ai_engine_token(&profile_key, name)?;
    let mut config = Config::load()?;
    if let Some(profile) = config.ai_engine_tokens.get_mut(&profile_key) {
        profile.names.retain(|current| current != name);
        if profile.active.as_deref() == Some(name) {
            profile.active = profile.names.first().cloned().or_else(|| {
                env_multi(&["SAKURA_AI_ENGINE_TOKEN"]).map(|_| AI_ENGINE_ENV_TOKEN_NAME.to_string())
            });
        }
        if profile.names.is_empty() && profile.active.is_none() {
            config.ai_engine_tokens.remove(&profile_key);
        }
    }
    config.save()?;
    Ok(())
}

fn delete_all_ai_engine_tokens(source: &CredentialSource) -> Result<()> {
    let profile_key = source.config_key();
    let mut config = Config::load()?;
    if let Some(profile) = config.ai_engine_tokens.remove(&profile_key) {
        for name in profile.names {
            crate::keychain::delete_named_ai_engine_token(&profile_key, &name)?;
        }
    }
    // 複数保存形式へ移行する前の項目も削除する。
    crate::keychain::delete_ai_engine_token(&profile_key)?;
    config.save()?;
    Ok(())
}

pub fn validate_ai_engine_token_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        bail!("トークン名を入力してください");
    }
    if name == AI_ENGINE_ENV_TOKEN_NAME
        || name
            .chars()
            .any(|c| c == ':' || c == '/' || c == '\\' || c.is_control())
    {
        bail!("トークン名として使えない文字が含まれています");
    }
    Ok(name)
}

fn migrate_legacy_ai_engine_token(config: &mut Config, source: &CredentialSource) -> Result<()> {
    let profile_key = source.config_key();
    if config.ai_engine_tokens.contains_key(&profile_key) {
        return Ok(());
    }
    let Some(token) = crate::keychain::get_ai_engine_token(&profile_key)? else {
        return Ok(());
    };
    let name = "default";
    crate::keychain::set_named_ai_engine_token(&profile_key, name, &token)?;
    crate::keychain::delete_ai_engine_token(&profile_key)?;
    config.ai_engine_tokens.insert(
        profile_key,
        AiEngineTokenProfile {
            active: Some(name.to_string()),
            names: vec![name.to_string()],
        },
    );
    config.save()?;
    Ok(())
}

/// キーチェーンに預けた資格情報を読む。
fn load_keychain_credentials(name: &str) -> Result<ApiCredentials> {
    let config = Config::load()?;
    let entry = config
        .credentials
        .get(name)
        .with_context(|| format!("設定に資格情報 {name} がありません"))?;
    let (token, secret) = crate::keychain::get_api_credentials(name)?
        .with_context(|| format!("キーチェーンに {name} のトークンがありません"))?;
    Ok(ApiCredentials {
        token,
        secret,
        source: CredentialSource::Keychain(name.to_string()),
        zone: entry.zone.clone(),
        api_root: entry.api_root.clone(),
    })
}

/// 選択肢として提示できる認証情報の一覧。
///
/// 環境変数が設定されていれば先頭に置き、続けて usacloud のプロファイルを名前順に並べる。
pub fn available_credential_sources() -> Vec<CredentialSource> {
    let mut sources = Vec::new();
    if credentials_from_env().is_some() {
        sources.push(CredentialSource::Env);
    }
    sources.extend(
        list_usacloud_profiles()
            .into_iter()
            .map(CredentialSource::Profile),
    );
    // キーチェーンに預けたものは設定ファイルに名前だけ載っている。
    if let Ok(config) = Config::load() {
        sources.extend(
            config
                .credentials
                .into_keys()
                .map(CredentialSource::Keychain),
        );
    }
    sources
}

/// `~/.usacloud/*/config.json` があるディレクトリ名をプロファイル名として集める。
fn list_usacloud_profiles() -> Vec<String> {
    let Ok(dir) = usacloud_config_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.path().join("config.json").is_file() {
                return None;
            }
            entry.file_name().into_string().ok()
        })
        .collect();
    names.sort();
    names
}

/// usacloud のプロファイル格納ディレクトリ（`~/.usacloud`）。
fn usacloud_config_dir() -> Result<PathBuf> {
    let base = match env_multi(&[
        "SAKURA_PROFILE_DIR",
        "SAKURACLOUD_PROFILE_DIR",
        "USACLOUD_PROFILE_DIR",
    ]) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir().context("ホームディレクトリを特定できませんでした")?,
    };
    Ok(base.join(".usacloud"))
}

/// usacloud のプロファイル（`~/.usacloud/<name>/config.json`）を読む。
fn load_usacloud_profile(name: Option<&str>) -> Result<ApiCredentials> {
    #[derive(Deserialize)]
    struct ProfileConfig {
        #[serde(rename = "AccessToken", default)]
        access_token: String,
        #[serde(rename = "AccessTokenSecret", default)]
        access_token_secret: String,
        #[serde(rename = "Zone", default)]
        zone: String,
        #[serde(rename = "APIRootURL", default)]
        api_root_url: String,
    }

    let dir = usacloud_config_dir()?;
    let name = match name {
        Some(name) if !name.is_empty() => name.to_string(),
        // プロファイル名の指定がなければ `current` ファイルの内容、それも無ければ "default"。
        _ => std::fs::read_to_string(dir.join("current"))
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "default".to_string()),
    };
    if name.contains('/') || name.contains(':') {
        bail!("プロファイル名として不正な文字が含まれています: {name}");
    }

    let path = dir.join(&name).join("config.json");
    let body = std::fs::read_to_string(&path)
        .with_context(|| format!("{} を読めませんでした", path.display()))?;
    let config: ProfileConfig = serde_json::from_str(&body)
        .with_context(|| format!("{} の解析に失敗しました", path.display()))?;

    if config.access_token.is_empty() || config.access_token_secret.is_empty() {
        bail!("プロファイル {name} にトークンが設定されていません");
    }
    Ok(ApiCredentials {
        token: config.access_token,
        secret: config.access_token_secret,
        source: CredentialSource::Profile(name),
        zone: Some(config.zone).filter(|z| !z.is_empty()),
        // 明示指定（--api-root / 環境変数）があればプロファイルより優先する。
        api_root: env_multi(&["SAKURA_API_ROOT_URL", "SAKURACLOUD_API_ROOT_URL"])
            .or(Some(config.api_root_url).filter(|r| !r.is_empty())),
    })
}

/// usacloud のプロファイルとして書き出す内容。
///
/// usacloud が書くのと同じキーを揃えておく。値を空にしておけば usacloud 側は
/// それぞれの既定値として扱うため、`usacloud config` で作ったものと同じように使える。
#[derive(Debug, Serialize)]
struct UsacloudProfileFile {
    #[serde(rename = "AccessToken")]
    access_token: String,
    #[serde(rename = "AccessTokenSecret")]
    access_token_secret: String,
    #[serde(rename = "Zone")]
    zone: String,
    #[serde(rename = "Zones")]
    zones: Option<Vec<String>>,
    #[serde(rename = "AcceptLanguage")]
    accept_language: String,
    #[serde(rename = "Gzip")]
    gzip: bool,
    #[serde(rename = "RetryMax")]
    retry_max: u32,
    #[serde(rename = "RetryWaitMin")]
    retry_wait_min: u32,
    #[serde(rename = "RetryWaitMax")]
    retry_wait_max: u32,
    #[serde(rename = "StatePollingTimeout")]
    state_polling_timeout: u32,
    #[serde(rename = "StatePollingInterval")]
    state_polling_interval: u32,
    #[serde(rename = "HTTPRequestTimeout")]
    http_request_timeout: u32,
    #[serde(rename = "HTTPRequestRateLimit")]
    http_request_rate_limit: u32,
    #[serde(rename = "APIRootURL")]
    api_root_url: String,
    #[serde(rename = "DefaultZone")]
    default_zone: String,
    #[serde(rename = "TraceMode")]
    trace_mode: String,
    #[serde(rename = "FakeMode")]
    fake_mode: bool,
    #[serde(rename = "FakeStorePath")]
    fake_store_path: String,
    #[serde(rename = "DefaultOutputType")]
    default_output_type: String,
    #[serde(rename = "NoColor")]
    no_color: bool,
    #[serde(rename = "ProcessTimeoutSec")]
    process_timeout_sec: u32,
    #[serde(rename = "ArgumentMatchMode")]
    argument_match_mode: String,
    #[serde(rename = "DefaultQueryDriver")]
    default_query_driver: String,
}

impl UsacloudProfileFile {
    fn new(token: &str, secret: &str, zone: &str, api_root: &str) -> Self {
        Self {
            access_token: token.to_string(),
            access_token_secret: secret.to_string(),
            zone: zone.to_string(),
            zones: None,
            accept_language: String::new(),
            gzip: false,
            retry_max: 0,
            retry_wait_min: 0,
            retry_wait_max: 0,
            state_polling_timeout: 0,
            state_polling_interval: 0,
            http_request_timeout: 0,
            http_request_rate_limit: 0,
            api_root_url: api_root.to_string(),
            default_zone: String::new(),
            trace_mode: String::new(),
            fake_mode: false,
            fake_store_path: String::new(),
            default_output_type: String::new(),
            no_color: false,
            process_timeout_sec: 0,
            argument_match_mode: String::new(),
            default_query_driver: String::new(),
        }
    }
}

/// トークンやシークレットから、見えない文字を取り除く。
///
/// Web ページからコピーすると、ノーブレークスペース（U+00A0）やゼロ幅
/// スペース（U+200B）が紛れ込むことがある。見た目では気づけないのに
/// 認証は 401 で弾かれるため、ここで落とす。
/// トークンは英数字とハイフンだけなので、空白・制御・書式文字は消してよい。
pub fn clean_secret(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control() && !is_format_char(*c))
        .collect()
}

/// 表示幅を持たない書式用の文字か（ゼロ幅スペースなど）。
fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{00ad}'           // soft hyphen
        | '\u{200b}'..='\u{200f}' // zero width space 〜 RLM
        | '\u{2028}'..='\u{202e}' // line/paragraph separator, 方向制御
        | '\u{2060}'..='\u{2064}' // word joiner など
        | '\u{feff}'          // BOM
    )
}

/// プロファイル名として使える文字列か検証する。
///
/// ディレクトリ名になるため、パス区切りや `.` 単体を弾く。
pub fn validate_profile_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("プロファイル名を入力してください");
    }
    if name == "." || name == ".." {
        bail!("プロファイル名として使えません: {name}");
    }
    // `current` は「現在のプロファイル名」を書くファイルとして予約されている。
    if name == "current" {
        bail!("`current` は usacloud が使う名前のため指定できません");
    }
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control())
    {
        bail!("プロファイル名に使えない文字が含まれています: {name}");
    }
    Ok(())
}

/// キーチェーンに預ける資格情報を新規作成する。
///
/// `~/.usacloud` には何も作らない。トークンはキーチェーンにだけ置き、
/// 設定ファイルには名前と既定ゾーンだけを書く。
pub fn create_keychain_credential(
    name: &str,
    token: &str,
    secret: &str,
    zone: &str,
    api_root: &str,
) -> Result<PathBuf> {
    validate_profile_name(name)?;
    let name = name.trim();
    let (token, secret) = (clean_secret(token), clean_secret(secret));
    if token.is_empty() || secret.is_empty() {
        bail!("アクセストークンとシークレットを入力してください");
    }

    let mut config = Config::load()?;
    if config.credentials.contains_key(name) {
        bail!("同じ名前の資格情報が既にあります: {name}");
    }
    crate::keychain::set_api_credentials(name, &token, &secret)?;
    config.credentials.insert(
        name.to_string(),
        KeychainCredential {
            zone: Some(zone.trim().to_string()).filter(|z| !z.is_empty()),
            // 本番なら書かない（既定なので）。
            api_root: Some(api_root.trim().to_string())
                .filter(|r| !r.is_empty() && r != DEFAULT_API_ROOT),
        },
    );
    config.save()
}

/// キーチェーンに預けた資格情報を削除する。
///
/// usacloud のプロファイルは他のツールも使うため、ここでは消さない。
pub fn delete_keychain_credential(name: &str) -> Result<()> {
    let mut config = Config::load()?;
    if config.credentials.remove(name).is_none() {
        bail!("設定に資格情報 {name} がありません");
    }
    crate::keychain::delete_api_credentials(name)?;
    let source = CredentialSource::Keychain(name.to_string());
    delete_all_ai_engine_tokens(&source)?;
    let profile_key = source.config_key();
    config.ai_engine_tokens.remove(&profile_key);
    config.iam_credentials.remove(&profile_key);
    crate::keychain::delete_iam_private_key(&profile_key)?;
    config.save()?;
    Ok(())
}

/// usacloud のプロファイルを新規作成する。
///
/// 既にある名前は上書きしない。ファイルは usacloud と同じ 0600 で書く。
pub fn create_usacloud_profile(
    name: &str,
    token: &str,
    secret: &str,
    zone: &str,
    api_root: &str,
) -> Result<PathBuf> {
    validate_profile_name(name)?;
    let name = name.trim();
    let (token, secret) = (clean_secret(token), clean_secret(secret));
    if token.is_empty() || secret.is_empty() {
        bail!("アクセストークンとシークレットを入力してください");
    }

    let dir = usacloud_config_dir()?.join(name);
    let path = dir.join("config.json");
    if path.exists() {
        bail!("同じ名前のプロファイルが既にあります: {}", path.display());
    }

    std::fs::create_dir_all(&dir)
        .with_context(|| format!("{} を作成できませんでした", dir.display()))?;
    restrict_dir_permissions(&dir)?;

    // usacloud も同じ `APIRootURL` を見るので、そのまま書けば共用できる。
    let api_root = if api_root.trim() == DEFAULT_API_ROOT {
        ""
    } else {
        api_root.trim()
    };
    let profile = UsacloudProfileFile::new(&token, &secret, zone.trim(), api_root);
    let body = serde_json::to_string_pretty(&profile)
        .context("プロファイルのシリアライズに失敗しました")?;
    std::fs::write(&path, body)
        .with_context(|| format!("{} に書き込めませんでした", path.display()))?;
    restrict_permissions(&path)?;
    Ok(path)
}

/// コンテナレジストリへのログイン情報（Docker Registry v2 API 用）。
///
/// これはメモリ上でだけ扱う形。設定ファイルにはユーザー名しか書かず、
/// パスワードは OS のキーチェーンに預ける。
#[derive(Debug, Clone)]
pub struct RegistryLogin {
    pub username: String,
    pub password: String,
}

/// 設定ファイルに書くレジストリの項目。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryAccount {
    pub username: String,
    /// 以前のバージョンが平文で書いていたパスワード。
    ///
    /// 読み込み時にキーチェーンへ移してから消すため、書き出しはしない。
    #[serde(default, skip_serializing)]
    pub password: Option<String>,
}

/// プロファイルごとの見た目の設定。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileStyle {
    /// ヘッダーとピッカーでの表示色。
    ///
    /// `red` / `yellow` / `green` / `cyan` / `blue` / `magenta` / `gray` か、
    /// `#RRGGBB` 形式で指定する。未指定なら既定色。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// この TUI 専用の資格情報。トークン自体はキーチェーンにあり、ここには載せない。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeychainCredential {
    /// 既定ゾーン。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// API ルート URL。未設定なら本番。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_root: Option<String>,
}

/// クラウドAPIプロファイルごとのAI Engineトークン一覧。
/// トークン本体はキーチェーンにあり、名前と選択状態だけを保存する。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiEngineTokenProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
}

/// IAMの識別情報。秘密鍵本体はOSのキーチェーンにのみ保存する。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IamCredentialMetadata {
    pub service_principal_id: String,
    pub key_id: String,
}

/// `~/.config/sakura-tui/config.toml` の内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// レジストリの FQDN をキーにしたユーザー名。
    #[serde(default)]
    pub registries: BTreeMap<String, RegistryAccount>,
    /// レジストリごとに保存済みのユーザー名一覧（ログイン時に選ぶため）。
    /// パスワードはユーザーごとにキーチェーンへ預ける。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub registry_accounts: BTreeMap<String, Vec<String>>,
    /// 認証情報をキーにした見た目の設定。
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileStyle>,
    /// キーチェーンに預けた資格情報（名前と既定ゾーンだけ）。
    #[serde(default)]
    pub credentials: BTreeMap<String, KeychainCredential>,
    /// AI Engineトークンの名前と選択状態。秘密値は含まない。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ai_engine_tokens: BTreeMap<String, AiEngineTokenProfile>,
    /// クラウドAPI認証元ごとのIAMサービスプリンシパル識別情報。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub iam_credentials: BTreeMap<String, IamCredentialMetadata>,
}

impl Config {
    /// 保存済みのログイン情報を、キーチェーンのパスワードと組にして返す。
    ///
    /// パスワードが取り出せない（キーチェーンが使えない・登録が消えた）場合は
    /// 「保存されていない」として扱い、改めてログインしてもらう。
    pub fn registry_login(&self, host: &str) -> Option<RegistryLogin> {
        let account = self.registries.get(host)?;
        let password = crate::keychain::get_password(host).ok().flatten()?;
        Some(RegistryLogin {
            username: account.username.clone(),
            password,
        })
    }

    /// ログイン情報を保存する。パスワードはキーチェーンにだけ書く。
    ///
    /// 既定の1件（自動ログインで使う）と、ユーザーごとの1件（ログイン時に
    /// 選べるようにするため）の両方に書く。
    pub fn save_registry_login(&mut self, host: &str, login: &RegistryLogin) -> Result<PathBuf> {
        crate::keychain::set_password(host, &login.password)?;
        crate::keychain::set_registry_user_password(host, &login.username, &login.password)?;
        self.registries.insert(
            host.to_string(),
            RegistryAccount {
                username: login.username.clone(),
                password: None,
            },
        );
        let names = self.registry_accounts.entry(host.to_string()).or_default();
        if !names.contains(&login.username) {
            names.push(login.username.clone());
        }
        self.save()
    }

    /// ログイン時に選べる、保存済みのユーザー名一覧。
    ///
    /// 以前のバージョンが保存した「既定の1件」しか無い場合もここに含める。
    pub fn registry_account_names(&self, host: &str) -> Vec<String> {
        let mut names = self
            .registry_accounts
            .get(host)
            .cloned()
            .unwrap_or_default();
        if let Some(account) = self.registries.get(host)
            && !names.contains(&account.username)
        {
            names.push(account.username.clone());
        }
        names
    }

    /// 指定したユーザー名で保存されているログイン情報を取り出す。
    pub fn registry_user_login(&self, host: &str, username: &str) -> Option<RegistryLogin> {
        let password = crate::keychain::get_registry_user_password(host, username)
            .ok()
            .flatten()
            .or_else(|| {
                // 以前のバージョンが保存した「既定の1件」からの取り出しにも対応する。
                let account = self.registries.get(host)?;
                if account.username != username {
                    return None;
                }
                crate::keychain::get_password(host).ok().flatten()
            })?;
        Some(RegistryLogin {
            username: username.to_string(),
            password,
        })
    }

    /// ログイン情報を破棄する。保存済みの全ユーザー分をキーチェーンからも消す。
    pub fn forget_registry_login(&mut self, host: &str) -> Result<bool> {
        let removed_default = self.registries.remove(host).is_some();
        let usernames = self.registry_accounts.remove(host).unwrap_or_default();
        // 設定ファイルに項目が無くてもキーチェーンには残っていることがある。
        crate::keychain::delete_password(host)?;
        for username in &usernames {
            crate::keychain::delete_registry_user_password(host, username)?;
        }
        let removed = removed_default || !usernames.is_empty();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// 平文で保存されていたパスワードをキーチェーンへ移す。
    ///
    /// 移せた件数を返す。1 件でも移したら設定ファイルを書き直して平文を消す。
    pub fn migrate_plaintext_passwords(&mut self) -> Result<usize> {
        let plaintext: Vec<(String, String)> = self
            .registries
            .iter()
            .filter_map(|(host, account)| {
                let password = account.password.clone()?;
                (!password.is_empty()).then(|| (host.clone(), password))
            })
            .collect();
        if plaintext.is_empty() {
            return Ok(0);
        }

        let mut moved = 0;
        for (host, password) in plaintext {
            crate::keychain::migrate(&host, &password)?;
            if let Some(account) = self.registries.get_mut(&host) {
                account.password = None;
            }
            moved += 1;
        }
        // `password` は skip_serializing なので、書き直せば平文は消える。
        self.save()?;
        Ok(moved)
    }

    /// 指定の認証情報に紐づけられた色名。
    pub fn profile_color(&self, source: &CredentialSource) -> Option<&str> {
        self.profiles
            .get(&source.config_key())?
            .color
            .as_deref()
            .filter(|c| !c.is_empty())
    }

    /// 色を設定する。`None` を渡すと既定色に戻す。
    pub fn set_profile_color(&mut self, source: &CredentialSource, color: Option<String>) {
        let key = source.config_key();
        match color {
            Some(color) => {
                self.profiles.entry(key).or_default().color = Some(color);
            }
            None => {
                if let Some(style) = self.profiles.get_mut(&key) {
                    style.color = None;
                }
                // 中身が空になった項目は残さない。
                self.profiles.retain(|_, style| style.color.is_some());
            }
        }
    }
}

/// 設定ファイルのパス。`SAKURA_TUI_CONFIG` で上書きできる。
pub fn config_path() -> Result<PathBuf> {
    if let Some(path) = env_multi(&["SAKURA_TUI_CONFIG"]) {
        return Ok(PathBuf::from(path));
    }
    let base = match env_multi(&["XDG_CONFIG_HOME"]) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .context("ホームディレクトリを特定できませんでした")?
            .join(".config"),
    };
    Ok(base.join("sakura-tui").join("config.toml"))
}

impl Config {
    /// 設定ファイルを読む。存在しない場合は空の設定を返す。
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        match std::fs::read_to_string(&path) {
            Ok(body) => toml::from_str(&body)
                .with_context(|| format!("{} の解析に失敗しました", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err).with_context(|| format!("{} を読めませんでした", path.display())),
        }
    }

    /// 設定ファイルを保存する。パスワードを含むのでパーミッションは 0600 にする。
    pub fn save(&self) -> Result<PathBuf> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("{} を作成できませんでした", parent.display()))?;
        }
        let body = toml::to_string_pretty(self).context("設定のシリアライズに失敗しました")?;
        std::fs::write(&path, body)
            .with_context(|| format!("{} に書き込めませんでした", path.display()))?;
        restrict_permissions(&path)?;
        Ok(path)
    }
}

#[cfg(unix)]
fn restrict_dir_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("{} のパーミッション変更に失敗しました", path.display()))
}

#[cfg(not(unix))]
fn restrict_dir_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("{} のパーミッション変更に失敗しました", path.display()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 設定ファイルにパスワードを書き出さないこと。
    #[test]
    fn password_is_never_serialized() {
        let mut config = Config::default();
        config.registries.insert(
            "example.sakuracr.jp".to_string(),
            RegistryAccount {
                username: "alice".to_string(),
                // 旧バージョンの設定を読み込んだ直後を模す。
                password: Some("s3cret".to_string()),
            },
        );
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("alice"), "{text}");
        assert!(!text.contains("s3cret"), "平文が書き出されている: {text}");
        assert!(!text.contains("password"), "{text}");
    }

    #[test]
    fn ai_engine_config_stores_names_but_not_tokens() {
        let mut config = Config::default();
        config.ai_engine_tokens.insert(
            "prod".to_string(),
            AiEngineTokenProfile {
                active: Some("batch".to_string()),
                names: vec!["batch".to_string(), "interactive".to_string()],
            },
        );
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("batch"));
        assert!(text.contains("interactive"));
        assert!(!text.contains("uuid:secret"));
    }

    #[test]
    fn iam_config_stores_ids_but_has_no_private_key_field() {
        let mut config = Config::default();
        config.iam_credentials.insert(
            "prod".to_string(),
            IamCredentialMetadata {
                service_principal_id: "sp-resource-id".to_string(),
                key_id: "key-id".to_string(),
            },
        );
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("sp-resource-id"));
        assert!(text.contains("key-id"));
        assert!(!text.contains("private_key"));
        assert!(!text.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn ai_engine_token_names_are_safe_for_keychain_keys() {
        assert_eq!(
            validate_ai_engine_token_name("batch-prod").unwrap(),
            "batch-prod"
        );
        for invalid in ["", "環境変数", "a:b", "a/b", "a\\b", "a\nb"] {
            assert!(
                validate_ai_engine_token_name(invalid).is_err(),
                "{invalid:?}"
            );
        }
    }

    /// 旧形式（平文パスワード入り）の設定ファイルも読めること。
    #[test]
    fn reads_legacy_config_with_plaintext_password() {
        let text = r#"
[registries."example.sakuracr.jp"]
username = "alice"
password = "s3cret"
"#;
        let config: Config = toml::from_str(text).unwrap();
        let account = &config.registries["example.sakuracr.jp"];
        assert_eq!(account.username, "alice");
        assert_eq!(account.password.as_deref(), Some("s3cret"));
    }

    /// 新形式（ユーザー名のみ）も読めること。
    #[test]
    fn reads_config_without_password() {
        let text = r#"
[registries."example.sakuracr.jp"]
username = "alice"
"#;
        let config: Config = toml::from_str(text).unwrap();
        assert_eq!(config.registries["example.sakuracr.jp"].password, None);
    }

    #[test]
    fn config_key_distinguishes_env_from_profiles() {
        assert_eq!(CredentialSource::Env.config_key(), "@env");
        assert_eq!(
            CredentialSource::Profile("prod".to_string()).config_key(),
            "prod"
        );
    }

    #[test]
    fn stores_and_clears_profile_color() {
        let source = CredentialSource::Profile("ixt15226_aipf-prod".to_string());
        let mut config = Config::default();
        assert_eq!(config.profile_color(&source), None);

        config.set_profile_color(&source, Some("red".to_string()));
        assert_eq!(config.profile_color(&source), Some("red"));

        // 既定色に戻したら設定自体を残さない。
        config.set_profile_color(&source, None);
        assert_eq!(config.profile_color(&source), None);
        assert!(config.profiles.is_empty());
    }

    /// 色を設定してもレジストリのログイン情報は壊れないこと。
    #[test]
    fn round_trips_through_toml() {
        let source = CredentialSource::Env;
        let mut config = Config::default();
        config.registries.insert(
            "example.sakuracr.jp".to_string(),
            RegistryAccount {
                username: "u".to_string(),
                password: None,
            },
        );
        config.set_profile_color(&source, Some("#ff8800".to_string()));

        let text = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.profile_color(&source), Some("#ff8800"));
        assert_eq!(parsed.registries.len(), 1);
    }

    #[test]
    fn label_reads_naturally() {
        assert_eq!(CredentialSource::Env.label(), "環境変数");
        assert_eq!(
            CredentialSource::Profile("default".to_string()).label(),
            "default"
        );
    }
}

#[cfg(test)]
mod profile_creation_tests {
    use super::*;

    #[test]
    fn accepts_ordinary_names() {
        for name in ["default", "ixt15226_aipf-dev", "my.profile", "本番"] {
            assert!(validate_profile_name(name).is_ok(), "{name}");
        }
    }

    /// ディレクトリ名になるため、パスを跨げる文字は弾く。
    #[test]
    fn rejects_path_traversal_and_separators() {
        for name in ["", "  ", ".", "..", "a/b", "a\\b", "a:b", "a\nb"] {
            assert!(validate_profile_name(name).is_err(), "{name:?}");
        }
    }

    /// `current` は usacloud が現在のプロファイル名を書くファイル。
    #[test]
    fn rejects_reserved_name() {
        assert!(validate_profile_name("current").is_err());
    }

    /// usacloud が読むキーを揃えて書き出すこと。
    #[test]
    fn writes_usacloud_compatible_json() {
        let profile = UsacloudProfileFile::new("tok", "sec", "is1a", "");
        let text = serde_json::to_string_pretty(&profile).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(parsed["AccessToken"], "tok");
        assert_eq!(parsed["AccessTokenSecret"], "sec");
        assert_eq!(parsed["Zone"], "is1a");
        // usacloud が参照する主要なキーが欠けていないこと。
        for key in [
            "Zones",
            "APIRootURL",
            "DefaultZone",
            "TraceMode",
            "FakeMode",
        ] {
            assert!(parsed.get(key).is_some(), "{key} が無い");
        }
    }

    /// キーチェーン方式は設定ファイルにトークンを書かないこと。
    #[test]
    fn keychain_credential_stores_no_secret() {
        let mut config = Config::default();
        config.credentials.insert(
            "myaccount".to_string(),
            KeychainCredential {
                zone: Some("tk1b".to_string()),
                api_root: None,
            },
        );
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("myaccount"), "{text}");
        assert!(text.contains("tk1b"), "{text}");
        assert!(!text.to_lowercase().contains("token"), "{text}");
        assert!(!text.to_lowercase().contains("secret"), "{text}");
    }

    /// 保存形式ごとに設定キーが分かれること（同名でも別物として扱う）。
    #[test]
    fn keychain_and_usacloud_names_do_not_collide() {
        let usacloud = CredentialSource::Profile("prod".to_string());
        let keychain = CredentialSource::Keychain("prod".to_string());
        assert_ne!(usacloud.config_key(), keychain.config_key());
        assert_eq!(usacloud.label(), keychain.label());
        assert_eq!(usacloud.kind_label(), "usacloud");
        assert_eq!(keychain.kind_label(), "キーチェーン");
    }
}

#[cfg(test)]
mod secret_cleaning_tests {
    use super::*;

    /// 正しいトークンはそのまま通ること（36文字のUUID形式）。
    #[test]
    fn keeps_valid_tokens_intact() {
        let token = "12345678-90ab-cdef-1234-567890abcdef";
        assert_eq!(clean_secret(token), token);
        assert_eq!(clean_secret(token).len(), 36);

        let secret = "a".repeat(64);
        assert_eq!(clean_secret(&secret), secret);
    }

    /// Web からコピーすると紛れ込む見えない文字を落とすこと。
    #[test]
    fn strips_invisible_characters() {
        // ノーブレークスペース・ゼロ幅スペース・BOM。
        let dirty = "\u{feff}1234\u{00a0}5678\u{200b}90ab";
        assert_eq!(clean_secret(dirty), "1234567890ab");
    }

    #[test]
    fn strips_surrounding_and_inner_whitespace() {
        assert_eq!(clean_secret("  abc  def\n"), "abcdef");
        assert_eq!(clean_secret("abc\tdef"), "abcdef");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert!(clean_secret("").is_empty());
        assert!(clean_secret("   \u{200b} ").is_empty());
    }
}

#[cfg(test)]
mod api_root_tests {
    use super::*;

    /// 未指定なら本番に繋ぐこと。
    #[test]
    fn defaults_to_production() {
        let creds = ApiCredentials {
            token: "t".into(),
            secret: "s".into(),
            source: CredentialSource::Env,
            zone: None,
            api_root: None,
        };
        assert_eq!(creds.api_root(), DEFAULT_API_ROOT);

        let empty = ApiCredentials {
            api_root: Some(String::new()),
            ..creds.clone()
        };
        assert_eq!(empty.api_root(), DEFAULT_API_ROOT);
    }

    #[test]
    fn uses_configured_root() {
        let creds = ApiCredentials {
            token: "t".into(),
            secret: "s".into(),
            source: CredentialSource::Env,
            zone: None,
            api_root: Some(TEST_API_ROOT.to_string()),
        };
        assert_eq!(creds.api_root(), TEST_API_ROOT);
        assert!(creds.api_root().contains("cloud-test"));
    }

    /// 本番と社内テストで URL の環境部分だけが変わること。
    #[test]
    fn roots_differ_only_in_environment_segment() {
        assert_eq!(
            DEFAULT_API_ROOT.replace("/cloud/", "/cloud-test/"),
            TEST_API_ROOT
        );
    }

    /// `@keychain:` 接頭辞は、usacloud と同名でもキーチェーン側を選ぶ手段。
    /// 接頭辞の切り出しだけを検証する（実際の解決は設定ファイルに依存するため）。
    #[test]
    fn keychain_prefix_is_stripped() {
        assert_eq!(
            "@keychain:crane74".strip_prefix("@keychain:"),
            Some("crane74")
        );
        // 接頭辞が無ければ素の名前として扱う。
        assert_eq!("crane74".strip_prefix("@keychain:"), None);
        // 名前に @ を含む usacloud プロファイルは作れないので取り違えない。
        assert_eq!("@env".strip_prefix("@keychain:"), None);
    }

    /// 見つからない名前はエラーにする。黙って別の認証情報へ落ちない。
    #[test]
    fn unknown_credential_name_is_an_error() {
        let err = resolve_credential_source("この名前は存在しないはず-9f3a").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("この名前は存在しないはず-9f3a"),
            "{message}"
        );
    }

    /// usacloud のプロファイルには、本番なら APIRootURL を書かないこと。
    #[test]
    fn production_writes_empty_api_root() {
        let production = UsacloudProfileFile::new("t", "s", "is1a", "");
        let text = serde_json::to_string(&production).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["APIRootURL"], "");

        let test_env = UsacloudProfileFile::new("t", "s", "is1a", TEST_API_ROOT);
        let text = serde_json::to_string(&test_env).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["APIRootURL"], TEST_API_ROOT);
    }
}
