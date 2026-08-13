//! さくらのクラウド シークレットマネージャ API。
//!
//! IaaS と同じ `api/cloud/1.1` の下にあるため `SacloudClient` を流用できる。
//! Vault はグローバルリソースなので、通信には既定ゾーンを使う。
//!
//! 値の取得（unveil）は明示的に要求したときだけ行う。一覧では名前と
//! 最新バージョンしか返らないので、一覧を眺めているだけで値が漏れることはない。

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{SacloudClient, flexible_number, null_as_default};

const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が `Total` を実態と違う値で返しても止まるようにする。
const MAX_PAGES: usize = 100;

/// シークレットを束ねる Vault。
#[derive(Debug, Clone)]
pub struct Vault {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub kms_key_id: String,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
}

/// Vault 内のシークレット。値は含まない。
#[derive(Debug, Clone)]
pub struct Secret {
    pub name: String,
    pub latest_version: Option<i64>,
}

// --- API のレスポンス形状 ---

#[derive(Debug, Deserialize)]
struct PaginatedVaultList {
    #[serde(rename = "Vaults", default, deserialize_with = "null_as_default")]
    vaults: Vec<RawVault>,
    #[serde(rename = "Total", default, deserialize_with = "flexible_number")]
    total: usize,
}

#[derive(Debug, Deserialize)]
struct RawVault {
    #[serde(rename = "ID", default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Description", default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "Tags", default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
    #[serde(rename = "KmsKeyID", default, deserialize_with = "null_as_default")]
    kms_key_id: String,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "ModifiedAt")]
    modified_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PaginatedSecretList {
    #[serde(rename = "Secrets", default, deserialize_with = "null_as_default")]
    secrets: Vec<RawSecret>,
    #[serde(rename = "Total", default, deserialize_with = "flexible_number")]
    total: usize,
}

#[derive(Debug, Deserialize)]
struct RawSecret {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(
        rename = "LatestVersion",
        default,
        deserialize_with = "flexible_number"
    )]
    latest_version: i64,
}

#[derive(Debug, Deserialize)]
struct UnveilResponse {
    #[serde(rename = "Secret")]
    secret: Option<UnveiledSecret>,
}

#[derive(Debug, Deserialize)]
struct UnveiledSecret {
    #[serde(rename = "Value", default, deserialize_with = "null_as_default")]
    value: String,
}

impl From<RawVault> for Vault {
    fn from(raw: RawVault) -> Self {
        Vault {
            id: raw.id,
            name: raw.name,
            description: raw.description,
            tags: raw.tags,
            kms_key_id: raw.kms_key_id,
            created_at: raw.created_at,
            modified_at: raw.modified_at,
        }
    }
}

impl SacloudClient {
    pub async fn list_vaults(&self) -> Result<Vec<Vault>> {
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({ "From": from, "Count": PAGE_SIZE });
            let res: PaginatedVaultList = self
                .request_common(Method::GET, "secretmanager/vaults", Some(body))
                .await?;
            let received = res.vaults.len();
            out.extend(res.vaults.into_iter().map(Vault::from));
            if received == 0 || out.len() >= res.total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    /// Vault 件数だけを数える。
    pub async fn count_vaults(&self) -> Result<usize> {
        let body = json!({ "From": 0, "Count": 1 });
        let res: PaginatedVaultList = self
            .request_common(Method::GET, "secretmanager/vaults", Some(body))
            .await?;
        Ok(res.total)
    }

    pub async fn list_secrets(&self, vault_id: &str) -> Result<Vec<Secret>> {
        let path = format!("secretmanager/vaults/{vault_id}/secrets");
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({ "From": from, "Count": PAGE_SIZE });
            let res: PaginatedSecretList =
                self.request_common(Method::GET, &path, Some(body)).await?;
            let received = res.secrets.len();
            out.extend(res.secrets.into_iter().map(|s| Secret {
                name: s.name,
                // 0 はバージョン未設定とみなす。
                latest_version: (s.latest_version > 0).then_some(s.latest_version),
            }));
            if received == 0 || out.len() >= res.total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    /// シークレットの値を取り出す。
    ///
    /// 明示的に要求されたときだけ呼ぶこと。`version` が `None` なら最新版。
    pub async fn unveil_secret(
        &self,
        vault_id: &str,
        name: &str,
        version: Option<i64>,
    ) -> Result<String> {
        let path = format!("secretmanager/vaults/{vault_id}/secrets/unveil");
        let mut payload = json!({ "Name": name });
        if let Some(version) = version {
            payload["Version"] = json!(version);
        }
        let res: UnveilResponse = self
            .request_common(Method::POST, &path, Some(json!({ "Secret": payload })))
            .await?;
        Ok(res.secret.map(|s| s.value).unwrap_or_default())
    }

    pub async fn create_vault(
        &self,
        name: &str,
        description: &str,
        kms_key_id: &str,
        tags: &[String],
    ) -> Result<()> {
        let body = json!({
            "Vault": {
                "Name": name,
                "Description": description,
                "KmsKeyID": kms_key_id,
                "Tags": tags,
            }
        });
        let _: serde_json::Value = self
            .request_common(Method::POST, "secretmanager/vaults", Some(body))
            .await?;
        Ok(())
    }

    /// Vault のメタデータを更新する。KMS 鍵は現在値を維持する。
    pub async fn update_vault(
        &self,
        vault: &Vault,
        name: &str,
        description: &str,
        tags: &[String],
    ) -> Result<()> {
        let body = json!({
            "Vault": {
                "ID": vault.id,
                "Name": name,
                "Description": description,
                "KmsKeyID": vault.kms_key_id,
                "Tags": tags,
                "CreatedAt": vault.created_at,
                "ModifiedAt": vault.modified_at,
            }
        });
        let path = format!("secretmanager/vaults/{}", vault.id);
        let _: serde_json::Value = self.request_common(Method::PUT, &path, Some(body)).await?;
        Ok(())
    }

    pub async fn delete_vault(&self, vault_id: &str) -> Result<()> {
        let path = format!("secretmanager/vaults/{vault_id}");
        let _: serde_json::Value = self.request_common(Method::DELETE, &path, None).await?;
        Ok(())
    }

    /// 同名が存在する場合は新しいバージョンを登録する。
    pub async fn put_secret(&self, vault_id: &str, name: &str, value: String) -> Result<()> {
        let path = format!("secretmanager/vaults/{vault_id}/secrets");
        let body = json!({ "Secret": { "Name": name, "Value": value } });
        let _: serde_json::Value = self.request_common(Method::POST, &path, Some(body)).await?;
        Ok(())
    }

    pub async fn delete_secret(&self, vault_id: &str, name: &str) -> Result<()> {
        let path = format!("secretmanager/vaults/{vault_id}/secrets");
        let body = json!({ "Secret": { "Name": name } });
        let _: serde_json::Value = self
            .request_common(Method::DELETE, &path, Some(body))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vault_list() {
        let body = r#"{"Count": 1, "From": 0, "Total": 1, "Vaults": [
            {"ID": "113900000000", "Name": "prod", "Description": null,
             "Tags": ["prod"], "KmsKeyID": "113800000000",
             "CreatedAt": "2026-01-02T03:04:05+09:00"}
        ]}"#;
        let res: PaginatedVaultList = serde_json::from_str(body).unwrap();
        let vault = Vault::from(res.vaults.into_iter().next().unwrap());
        assert_eq!(vault.name, "prod");
        assert_eq!(vault.description, "");
        assert_eq!(vault.kms_key_id, "113800000000");
    }

    /// 一覧に値が含まれないこと（含まれていたら設計上の事故）。
    #[test]
    fn secret_list_has_no_values() {
        let body = r#"{"Count": 2, "From": 0, "Total": 2, "Secrets": [
            {"Name": "db-password", "LatestVersion": 3},
            {"Name": "api-token", "LatestVersion": null}
        ]}"#;
        let res: PaginatedSecretList = serde_json::from_str(body).unwrap();
        assert_eq!(res.secrets.len(), 2);
        assert_eq!(res.secrets[0].latest_version, 3);
        // null は 0（未設定）として受ける。
        assert_eq!(res.secrets[1].latest_version, 0);
        // Secret に値を持つフィールドが無いことは型で保証される。
    }

    #[test]
    fn parses_unveiled_value() {
        let body = r#"{"Secret": {"Name": "db-password", "Value": "s3cret", "Version": 3}}"#;
        let res: UnveilResponse = serde_json::from_str(body).unwrap();
        assert_eq!(res.secret.unwrap().value, "s3cret");
    }

    /// 数値項目が文字列で返ってきても受けられること。
    #[test]
    fn accepts_string_numbers() {
        let body = r#"{"Count": "1", "From": 0, "Total": "1", "Secrets": [
            {"Name": "db-password", "LatestVersion": "3"}
        ]}"#;
        let res: PaginatedSecretList = serde_json::from_str(body).unwrap();
        assert_eq!(res.total, 1);
        assert_eq!(res.secrets[0].latest_version, 3);
    }

    #[test]
    fn missing_unveil_payload_is_empty() {
        let res: UnveilResponse = serde_json::from_str("{}").unwrap();
        assert!(res.secret.is_none());
    }
}
