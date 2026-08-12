//! 認証情報の読み込みと保存。
//!
//! さくらのクラウド API の認証情報は環境変数と usacloud プロファイルから読む
//! （sacloud/api-client-go と同じ優先順位）。コンテナレジストリ自体への
//! ログイン情報はクラウド API では取得できないため、独自の設定ファイルに置く。

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

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
    fn new(token: &str, secret: &str, zone: &str) -> Self {
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
            api_root_url: String::new(),
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
) -> Result<PathBuf> {
    validate_profile_name(name)?;
    let name = name.trim();
    if token.trim().is_empty() || secret.trim().is_empty() {
        bail!("アクセストークンとシークレットを入力してください");
    }

    let mut config = Config::load()?;
    if config.credentials.contains_key(name) {
        bail!("同じ名前の資格情報が既にあります: {name}");
    }
    crate::keychain::set_api_credentials(name, token.trim(), secret.trim())?;
    config.credentials.insert(
        name.to_string(),
        KeychainCredential {
            zone: Some(zone.trim().to_string()).filter(|z| !z.is_empty()),
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
) -> Result<PathBuf> {
    validate_profile_name(name)?;
    let name = name.trim();
    if token.trim().is_empty() || secret.trim().is_empty() {
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

    let profile = UsacloudProfileFile::new(token.trim(), secret.trim(), zone.trim());
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
}

/// `~/.config/sakura-tui/config.toml` の内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// レジストリの FQDN をキーにしたユーザー名。
    #[serde(default)]
    pub registries: BTreeMap<String, RegistryAccount>,
    /// 認証情報をキーにした見た目の設定。
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileStyle>,
    /// キーチェーンに預けた資格情報（名前と既定ゾーンだけ）。
    #[serde(default)]
    pub credentials: BTreeMap<String, KeychainCredential>,
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
    pub fn save_registry_login(&mut self, host: &str, login: &RegistryLogin) -> Result<PathBuf> {
        crate::keychain::set_password(host, &login.password)?;
        self.registries.insert(
            host.to_string(),
            RegistryAccount {
                username: login.username.clone(),
                password: None,
            },
        );
        self.save()
    }

    /// ログイン情報を破棄する。キーチェーンからも消す。
    pub fn forget_registry_login(&mut self, host: &str) -> Result<bool> {
        let removed = self.registries.remove(host).is_some();
        // 設定ファイルに項目が無くてもキーチェーンには残っていることがある。
        crate::keychain::delete_password(host)?;
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
        let profile = UsacloudProfileFile::new("tok", "sec", "is1a");
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
