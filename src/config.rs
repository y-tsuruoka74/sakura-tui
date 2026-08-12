//! 認証情報の読み込みと保存。
//!
//! さくらのクラウド API の認証情報は環境変数と usacloud プロファイルから読む
//! （sacloud/api-client-go と同じ優先順位）。コンテナレジストリ自体への
//! ログイン情報はクラウド API では取得できないため、独自の設定ファイルに置く。

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

/// さくらのクラウド API のアクセストークンとシークレット。
#[derive(Debug, Clone)]
pub struct ApiCredentials {
    pub token: String,
    pub secret: String,
    /// どこから読んだかの説明（ステータスバー表示用）。
    pub source: String,
}

fn env_multi(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()))
}

/// 環境変数 → usacloud プロファイルの順で API 認証情報を探す。
pub fn load_api_credentials() -> Result<ApiCredentials> {
    let token = env_multi(&["SAKURA_ACCESS_TOKEN", "SAKURACLOUD_ACCESS_TOKEN"]);
    let secret = env_multi(&[
        "SAKURA_ACCESS_TOKEN_SECRET",
        "SAKURACLOUD_ACCESS_TOKEN_SECRET",
    ]);
    if let (Some(token), Some(secret)) = (token, secret) {
        return Ok(ApiCredentials {
            token,
            secret,
            source: "環境変数".to_string(),
        });
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
        source: format!("usacloudプロファイル({name})"),
    })
}

/// コンテナレジストリへのログイン情報（Docker Registry v2 API 用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryLogin {
    pub username: String,
    pub password: String,
}

/// `~/.config/sakura-tui/config.toml` の内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// レジストリの FQDN をキーにしたログイン情報。
    #[serde(default)]
    pub registries: BTreeMap<String, RegistryLogin>,
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
fn restrict_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("{} のパーミッション変更に失敗しました", path.display()))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}
