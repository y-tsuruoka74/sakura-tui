//! IAM API向けサービスプリンシパル認証。
//!
//! RSA秘密鍵で短命JWTを署名し、IAMのOAuth2エンドポイントでBearerトークンへ交換する。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result, bail};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::config::IamCredentials;

#[derive(Debug, Serialize)]
struct Claims<'a> {
    aud: &'a str,
    exp: i64,
    iat: i64,
    iss: &'a str,
    sub: &'a str,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

fn default_expires_in() -> u64 {
    3600
}

#[derive(Debug)]
pub struct AccessToken {
    pub value: String,
    pub expires_in: u64,
}

pub fn credentials_fingerprint(credentials: &IamCredentials) -> u64 {
    let mut hasher = DefaultHasher::new();
    credentials.service_principal_id.hash(&mut hasher);
    credentials.key_id.hash(&mut hasher);
    credentials.private_key.hash(&mut hasher);
    hasher.finish()
}

pub fn token_endpoint(api_root: &str) -> String {
    let global_root = api_root.strip_suffix("/zone").unwrap_or(api_root);
    format!(
        "{}/api/iam/1.0/service-principals/oauth2/token",
        global_root.trim_end_matches('/')
    )
}

fn assertion(credentials: &IamCredentials, audience: &str) -> Result<String> {
    let now = chrono::Utc::now().timestamp();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(credentials.key_id.clone());
    let claims = Claims {
        aud: audience,
        exp: now + 300,
        iat: now,
        iss: &credentials.service_principal_id,
        sub: &credentials.service_principal_id,
    };
    let key = EncodingKey::from_rsa_pem(credentials.private_key.as_bytes()).context(
        "RSA秘密鍵を読み取れませんでした。PEM形式のPKCS#8またはPKCS#1秘密鍵を指定してください",
    )?;
    encode(&header, &claims, &key).context("サービスプリンシパルJWTを署名できませんでした")
}

pub async fn issue_access_token(
    http: &reqwest::Client,
    api_root: &str,
    credentials: &IamCredentials,
) -> Result<AccessToken> {
    let endpoint = token_endpoint(api_root);
    let jwt = assertion(credentials, &endpoint)?;
    let response = http
        .post(&endpoint)
        .header("X-Requested-With", "XMLHttpRequest")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", jwt.as_str()),
        ])
        .send()
        .await
        .context("IAMアクセストークンの発行リクエストに失敗しました")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("IAMアクセストークンの応答を読み取れませんでした")?;
    if !status.is_success() {
        bail!(
            "IAMアクセストークンを発行できませんでした ({}): {}",
            status,
            token_error_message(status, &body)
        );
    }
    let token: TokenResponse =
        serde_json::from_str(&body).context("IAMアクセストークンの応答を解析できませんでした")?;
    if token.access_token.is_empty() {
        bail!("IAMアクセストークンの応答が空でした");
    }
    Ok(AccessToken {
        value: token.access_token,
        expires_in: token.expires_in,
    })
}

fn token_error_message(status: StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            ["error_description", "message", "error"]
                .iter()
                .find_map(|key| value.get(key).and_then(|item| item.as_str()))
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    if detail.trim().is_empty() {
        status
            .canonical_reason()
            .unwrap_or("認証エラー")
            .to_string()
    } else {
        detail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_production_and_test_token_endpoints() {
        assert_eq!(
            token_endpoint("https://secure.sakura.ad.jp/cloud/zone"),
            "https://secure.sakura.ad.jp/cloud/api/iam/1.0/service-principals/oauth2/token"
        );
        assert_eq!(
            token_endpoint("https://secure.sakura.ad.jp/cloud-test/zone"),
            "https://secure.sakura.ad.jp/cloud-test/api/iam/1.0/service-principals/oauth2/token"
        );
    }

    #[test]
    fn fingerprint_changes_with_key_material() {
        let first = IamCredentials {
            service_principal_id: "sp-1".into(),
            key_id: "key-1".into(),
            private_key: "one".into(),
        };
        let mut second = first.clone();
        second.private_key = "two".into();
        assert_ne!(
            credentials_fingerprint(&first),
            credentials_fingerprint(&second)
        );
    }
}
