//! さくらのAI Engine APIクライアント。
//!
//! 推論APIとは別のアカウントトークンをBearer認証で使用する。

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde_json::Value;

use crate::managed_resources::ManagedResource;

const API_ROOT: &str = "https://api.ai.sakura.ad.jp";

#[derive(Clone)]
pub struct AiEngineClient {
    http: reqwest::Client,
    token: String,
}

impl AiEngineClient {
    pub fn new(token: impl Into<String>) -> Result<Self> {
        Ok(Self {
            http: crate::http::client()?,
            token: token.into(),
        })
    }

    /// OpenAI互換のモデル一覧を取得する。
    ///
    /// 公式OpenAPIには掲載されていないため、レスポンスに追加項目があっても壊れない
    /// ようJSONを柔軟に読む。
    pub async fn list_models(&self) -> Result<Vec<ManagedResource>> {
        let text = self.get_text("/v1/models", &[]).await?;
        parse_models(&text)
    }

    /// Bearer 認証で multipart を送る。
    ///
    /// RAGのアップロードは `multipart/form-data` が主たる形式。
    /// 再送すると同じファイルを二重に登録しかねないので、リトライは挟まない。
    pub(crate) async fn post_multipart(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<String> {
        let url = reqwest::Url::parse(&format!("{API_ROOT}{path}"))?;
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .header(reqwest::header::ACCEPT, "application/json")
            .multipart(form)
            .send()
            .await
            .context("AI Engine APIへのアップロードに失敗しました")?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("AI Engine APIレスポンスの読み取りに失敗しました")?;
        if !status.is_success() {
            bail!("{}", format_error(status, &text));
        }
        Ok(text)
    }

    /// Bearer 認証で JSON を PUT する。
    pub(crate) async fn put_json(&self, path: &str, body: serde_json::Value) -> Result<String> {
        let url = reqwest::Url::parse(&format!("{API_ROOT}{path}"))?;
        let response = crate::http::send_with_retry(&self.http, || {
            Ok(self
                .http
                .request(Method::PUT, url.clone())
                .bearer_auth(&self.token)
                .header(reqwest::header::ACCEPT, "application/json")
                .json(&body)
                .build()?)
        })
        .await
        .context("AI Engine APIへの更新リクエストに失敗しました")?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("AI Engine APIレスポンスの読み取りに失敗しました")?;
        if !status.is_success() {
            bail!("{}", format_error(status, &text));
        }
        Ok(text)
    }

    /// Bearer 認証で DELETE する。成功時は本文が無い（204）。
    pub(crate) async fn delete(&self, path: &str) -> Result<()> {
        let url = reqwest::Url::parse(&format!("{API_ROOT}{path}"))?;
        let response = crate::http::send_with_retry(&self.http, || {
            Ok(self
                .http
                .request(Method::DELETE, url.clone())
                .bearer_auth(&self.token)
                .build()?)
        })
        .await
        .context("AI Engine APIへの削除リクエストに失敗しました")?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let text = response.text().await.unwrap_or_default();
        bail!("{}", format_error(status, &text))
    }

    /// Bearer 認証で GET して本文をそのまま返す。
    ///
    /// モデル一覧と RAG は同じホスト・同じトークンなので、送受信をここへ寄せる。
    /// 解析は用途ごとに違うため呼び出し側に任せる。
    pub(crate) async fn get_text(&self, path: &str, query: &[(&str, String)]) -> Result<String> {
        let mut url = reqwest::Url::parse(&format!("{API_ROOT}{path}"))?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
        let response = crate::http::send_with_retry(&self.http, || {
            Ok(self
                .http
                .request(Method::GET, url.clone())
                .bearer_auth(&self.token)
                .header(reqwest::header::ACCEPT, "application/json")
                .build()?)
        })
        .await
        .context("AI Engine APIへのリクエストに失敗しました")?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("AI Engine APIレスポンスの読み取りに失敗しました")?;
        if !status.is_success() {
            bail!("{}", format_error(status, &text));
        }
        Ok(text)
    }
}

fn format_error(status: StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            ["message", "detail", "error"]
                .into_iter()
                .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_string))
        })
        .unwrap_or_else(|| body.trim().chars().take(200).collect());
    let hint = if status == StatusCode::UNAUTHORIZED {
        "（t キーでアカウントトークンを確認してください）"
    } else {
        ""
    };
    if detail.is_empty() {
        format!("AI Engine APIエラー ({status}){hint}")
    } else {
        format!("AI Engine APIエラー ({status}): {detail}{hint}")
    }
}

fn parse_models(text: &str) -> Result<Vec<ManagedResource>> {
    let value: Value = serde_json::from_str(text).context("モデル一覧の解析に失敗しました")?;
    let models = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .context("モデル一覧にdata配列がありません")?;
    let mut out: Vec<ManagedResource> = models.iter().filter_map(parse_model).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn parse_model(value: &Value) -> Option<ManagedResource> {
    let id = string(value, "id");
    if id.is_empty() {
        return None;
    }
    let name = non_empty([string(value, "display_name"), id.clone()]);
    let resource_type = non_empty([
        string(value, "type"),
        string(value, "object"),
        "model".to_string(),
    ]);
    let owner = non_empty([string(value, "owned_by"), string(value, "provider")]);
    let mut details = vec![("モデルID".to_string(), id.clone())];
    add_detail(&mut details, "種別", resource_type.clone());
    add_detail(&mut details, "提供元", owner.clone());
    add_detail(&mut details, "説明", string(value, "description"));
    add_detail(
        &mut details,
        "コンテキスト長",
        value
            .get("context_length")
            .map(value_text)
            .unwrap_or_default(),
    );
    if let Some(capabilities) = value.get("capabilities") {
        add_detail(&mut details, "対応機能", capability_text(capabilities));
    }
    if let Some(created) = value.get("created") {
        add_detail(&mut details, "作成値", value_text(created));
    }
    Some(ManagedResource {
        id,
        name,
        description: string(value, "description"),
        tags: Vec::new(),
        resource_type,
        status: non_empty([string(value, "status"), "available".to_string()]),
        plan: owner,
        created_at: String::new(),
        details,
    })
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => String::new(),
    }
}

fn capability_text(value: &Value) -> String {
    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(values) => values
            .iter()
            .filter(|(_, enabled)| enabled.as_bool().unwrap_or(false))
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    }
}

fn non_empty<const N: usize>(values: [String; N]) -> String {
    values
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn add_detail(details: &mut Vec<(String, String)>, label: &str, value: String) {
    if !value.is_empty() && !details.iter().any(|(_, current)| current == &value) {
        details.push((label.to_string(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_compatible_models() {
        let models = parse_models(
            r#"{
                "object":"list",
                "data":[
                    {"id":"gpt-oss-120b","object":"model","owned_by":"sakura"},
                    {"id":"Qwen3-Embedding-4B","type":"embedding","capabilities":{"embeddings":true,"chat":false}}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Qwen3-Embedding-4B");
        assert!(
            models[0]
                .details
                .iter()
                .any(|(_, value)| value == "embeddings")
        );
        assert_eq!(models[1].plan, "sakura");
    }

    #[test]
    fn ignores_entries_without_an_id() {
        let models = parse_models(r#"{"data":[{}, {"id":"usable"}]}"#).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "usable");
    }

    #[test]
    fn authentication_error_has_a_setup_hint() {
        let message = format_error(StatusCode::UNAUTHORIZED, r#"{"detail":"invalid token"}"#);
        assert!(message.contains("invalid token"));
        assert!(message.contains("t キー"));
    }
}
