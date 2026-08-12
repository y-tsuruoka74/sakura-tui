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
    /// usacloud のプロファイル。
    Profile(String),
}

impl CredentialSource {
    /// ヘッダーやピッカーに出す表示名。
    pub fn label(&self) -> String {
        match self {
            CredentialSource::Env => "環境変数".to_string(),
            CredentialSource::Profile(name) => name.clone(),
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
        }
    }

    /// プロファイルに設定された既定ゾーン。ピッカーで見分ける手がかりにする。
    pub fn zone(&self) -> Option<String> {
        match self {
            CredentialSource::Env => env_multi(&["SAKURA_ZONE", "SAKURACLOUD_ZONE"]),
            CredentialSource::Profile(name) => {
                load_usacloud_profile(Some(name)).ok().and_then(|c| c.zone)
            }
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
    }
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

/// コンテナレジストリへのログイン情報（Docker Registry v2 API 用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryLogin {
    pub username: String,
    pub password: String,
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

/// `~/.config/sakura-tui/config.toml` の内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// レジストリの FQDN をキーにしたログイン情報。
    #[serde(default)]
    pub registries: BTreeMap<String, RegistryLogin>,
    /// 認証情報（プロファイル名 / `@env`）をキーにした見た目の設定。
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileStyle>,
}

impl Config {
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
            RegistryLogin {
                username: "u".to_string(),
                password: "p".to_string(),
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
