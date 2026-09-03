//! さくらのAI Engine コントロールパネルAPI（`/cloud/api/ai/1.0`）のクライアント。
//!
//! 推論API・RAG API（`api.ai.sakura.ad.jp` にアカウントトークンのBearer認証）とは
//! 別系統で、IaaS APIと同じAPIキーのBasic認証を使い、ホストも
//! `secure.sakura.ad.jp` 側にある。APIポータルには仕様が載っていないため、
//! コントロールパネル（<https://secure.sakura.ad.jp/ai/>）が呼ぶものに合わせている。
//!
//! パスの末尾スラッシュは必須。落とすと 404 ではなく 403 が返るので、
//! 権限不足と見分けが付かなくなる。

use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use crate::config::ApiCredentials;

/// ゾーンを含まないグローバルAPI。テスト環境のルートも維持できるよう、
/// 設定済みのAPIルートから末尾の `/zone` だけを取り除いて組み立てる。
fn api_root(creds: &ApiCredentials) -> String {
    let base = creds.api_root().trim_end_matches("/zone");
    format!("{base}/api/ai/1.0")
}

#[derive(Clone)]
pub struct AiEngineCloudClient {
    http: reqwest::Client,
    token: String,
    secret: String,
    api_root: String,
}

impl AiEngineCloudClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        Ok(Self {
            http: crate::http::client()?,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
            api_root: api_root(creds),
        })
    }

    #[cfg(test)]
    fn with_api_root(api_root: impl Into<String>) -> Result<Self> {
        Ok(Self {
            http: crate::http::client()?,
            token: "test-token".to_string(),
            secret: "test-secret".to_string(),
            api_root: api_root.into(),
        })
    }

    /// 認証情報。アカウント・会員ID・契約プランがここで一度に取れる。
    pub async fn auth(&self) -> Result<CloudAuth> {
        parse_auth(&self.get_text("/auth/", &[]).await?)
    }

    /// 利用できるモデルの一覧。1ページに収まるよう上限まで引く。
    pub async fn models(&self) -> Result<Vec<CloudModel>> {
        let query = [("page_size", "100".to_string())];
        parse_models(&self.get_text("/models/", &query).await?)
    }

    /// 日別のリクエスト数。期間の指定は必須。
    pub async fn request_usages(&self, start: &str, end: &str) -> Result<Vec<CloudUsage>> {
        let query = [
            ("type", "request".to_string()),
            ("start_at", start.to_string()),
            ("end_at", end.to_string()),
        ];
        parse_usages(&self.get_text("/usages/", &query).await?)
    }

    /// 日別のドキュメントチャンク数。期間の指定は必須。
    pub async fn document_usages(&self, start: &str, end: &str) -> Result<Vec<CloudDocumentUsage>> {
        let query = [
            ("type", "document".to_string()),
            ("start_at", start.to_string()),
            ("end_at", end.to_string()),
        ];
        parse_document_usages(&self.get_text("/usages/", &query).await?)
    }

    /// 指定月の請求。月ごとに1件で、内訳が `details` に入る。
    pub async fn bill(&self, year_month: &str) -> Result<CloudBill> {
        let path = bill_path(year_month)?;
        parse_bill(year_month, &self.get_text(&path, &[]).await?)
    }

    async fn get_text(&self, path: &str, query: &[(&str, String)]) -> Result<String> {
        let base = self.api_root.trim_end_matches('/');
        let mut url = reqwest::Url::parse(&format!("{base}{path}"))?;
        // 空のまま query_pairs_mut を呼ぶと末尾に `?` だけが付く。
        if !query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
        }
        let response = crate::http::send_with_retry(&self.http, || {
            Ok(self
                .http
                .request(Method::GET, url.clone())
                .basic_auth(&self.token, Some(&self.secret))
                .header(reqwest::header::ACCEPT, "application/json")
                .build()?)
        })
        .await
        .context("AI Engine コントロールパネルAPIへのリクエストに失敗しました")?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("AI Engine コントロールパネルAPIのレスポンス読み取りに失敗しました")?;
        if !status.is_success() {
            bail!("{}", format_error(status, &text, &self.token, &self.secret));
        }
        Ok(text)
    }
}

/// `yyyymm` 以外を弾く。URLへそのまま埋めるので、経路を変えられないようにする。
fn bill_path(year_month: &str) -> Result<String> {
    if year_month.len() != 6 || !year_month.chars().all(|c| c.is_ascii_digit()) {
        bail!("請求年月の形式が不正です: {year_month}");
    }
    Ok(format!("/bills/{year_month}/"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudField {
    pub label: String,
    pub value: String,
}

impl CloudField {
    fn new(label: &str, value: impl Into<String>) -> Self {
        Self {
            label: label.to_string(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CloudAuth {
    pub account_id: String,
    pub account_code: String,
    pub account_name: String,
    pub member_id: String,
    pub tos_agreed_at: String,
    pub created_at: String,
    /// 契約中のプラン名。
    pub plan: String,
    /// プランの内訳（識別子・リクエスト上限・従量課金の可否など）。
    pub plan_details: Vec<CloudField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudModel {
    /// 推論APIで指定するモデル名。
    pub id: String,
    /// 画面に出す名前。表示名が無いモデルは `id` と同じ。
    pub name: String,
    /// APIが返す生の状態値。表示は [`CloudModel::status_label`] を使う。
    pub status: String,
    /// 対応する用途（チャット生成・埋め込みなど）。
    pub features: Vec<String>,
    pub tags: Vec<String>,
    /// コントロールパネル内の連番。
    pub number: String,
    /// 音声合成モデルの声色。
    pub styles: Vec<String>,
    pub tos_link: String,
}

impl CloudModel {
    /// 状態は英語のまま出すと読み手に伝わらないので日本語にする。
    pub fn status_label(&self) -> String {
        match self.status.as_str() {
            "available" => "利用可能",
            "deprecated" => "提供終了予定",
            "approval_required" => "要申請",
            "tos_agreement_required" => "要規約同意",
            other => other,
        }
        .to_string()
    }

    /// 絞り込みの対象。表示している列は全て引っかかるようにする。
    pub fn searchable(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.id,
            self.name,
            self.status_label(),
            self.features.join(" "),
            self.tags.join(" "),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudUsage {
    pub time: String,
    pub total: i64,
    pub details: Vec<CloudField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudDocumentUsage {
    pub time: String,
    pub chunk_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CloudBill {
    pub year_month: String,
    pub updated_at: String,
    pub close_date: String,
    pub details: Vec<CloudBillDetail>,
}

impl CloudBill {
    pub fn total(&self) -> f64 {
        self.details.iter().map(|detail| detail.amount).sum()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudBillDetail {
    pub no: i64,
    pub usage_type: String,
    pub usage: f64,
    pub amount: f64,
    pub description: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawAuth {
    account: Option<RawAccount>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAccount {
    #[serde(default)]
    account_id: String,
    #[serde(default)]
    account_code: String,
    #[serde(default)]
    account_name: String,
    member: Option<RawAccountMember>,
    #[serde(default)]
    tos_agreed_at: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    plan: Option<RawPlan>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAccountMember {
    #[serde(default)]
    member_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawPlan {
    #[serde(default)]
    name: String,
    #[serde(default)]
    label: String,
    request_limit_chat_completions: Option<i64>,
    request_limit_embeddings: Option<i64>,
    request_limit_audio_transcriptions: Option<i64>,
    request_limit_audio_speeches: Option<i64>,
    is_allow_pay_as_you_go: Option<bool>,
    price: Option<f64>,
}

fn parse_auth(text: &str) -> Result<CloudAuth> {
    let raw: RawAuth = serde_json::from_str(text).context("認証情報の解析に失敗しました")?;
    let Some(account) = raw.account else {
        bail!("認証情報にアカウントが含まれていません")
    };
    let plan = account.plan.unwrap_or_default();
    let mut plan_details = Vec::new();
    if !plan.label.is_empty() {
        plan_details.push(CloudField::new("プランID", plan.label));
    }
    // 種類ごとに行を分けるとラベルが長くなり、値と詰まって読みにくい。1行にまとめる。
    let limits: Vec<String> = [
        ("チャット生成", plan.request_limit_chat_completions),
        ("埋め込み", plan.request_limit_embeddings),
        ("文字起こし", plan.request_limit_audio_transcriptions),
        ("音声合成", plan.request_limit_audio_speeches),
    ]
    .into_iter()
    .filter_map(|(label, limit)| limit.map(|limit| format!("{label} {limit}")))
    .collect();
    if !limits.is_empty() {
        plan_details.push(CloudField::new("上限", limits.join(" / ")));
    }
    if let Some(allowed) = plan.is_allow_pay_as_you_go {
        let value = if allowed { "可" } else { "不可" };
        plan_details.push(CloudField::new("従量課金", value));
    }
    if let Some(price) = plan.price {
        plan_details.push(CloudField::new(
            "月額",
            format!("{}円", format_amount(price)),
        ));
    }
    Ok(CloudAuth {
        account_id: account.account_id,
        account_code: account.account_code,
        account_name: account.account_name,
        member_id: account.member.unwrap_or_default().member_id,
        tos_agreed_at: account.tos_agreed_at.unwrap_or_default(),
        created_at: account.created_at.unwrap_or_default(),
        plan: plan.name,
        plan_details,
    })
}

#[derive(Debug, Default, Deserialize)]
struct RawModelList {
    #[serde(default)]
    results: Vec<RawModel>,
}

#[derive(Debug, Default, Deserialize)]
struct RawModel {
    #[serde(default)]
    id: Value,
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    status: String,
    features: Option<RawModelFeatures>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    styles: Vec<RawModelStyle>,
    #[serde(default)]
    tos_link: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawModelStyle {
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
}

#[derive(Debug, Default, Deserialize)]
struct RawModelFeatures {
    #[serde(default)]
    chat_completions: bool,
    #[serde(default)]
    embeddings: bool,
    #[serde(default)]
    audio_transcriptions: bool,
    #[serde(default)]
    audio_speeches: bool,
}

fn parse_models(text: &str) -> Result<Vec<CloudModel>> {
    let raw: RawModelList = serde_json::from_str(text).context("モデル一覧の解析に失敗しました")?;
    Ok(raw
        .results
        .into_iter()
        .filter(|model| !model.name.is_empty())
        .map(|model| {
            let features = model.features.unwrap_or_default();
            CloudModel {
                name: if model.display_name.is_empty() {
                    model.name.clone()
                } else {
                    model.display_name
                },
                id: model.name,
                status: model.status,
                features: [
                    (features.chat_completions, "チャット生成"),
                    (features.embeddings, "埋め込み"),
                    (features.audio_transcriptions, "文字起こし"),
                    (features.audio_speeches, "音声合成"),
                ]
                .into_iter()
                .filter(|(enabled, _)| *enabled)
                .map(|(_, label)| label.to_string())
                .collect(),
                tags: model.tags,
                number: scalar_text(&model.id).unwrap_or_default(),
                styles: model
                    .styles
                    .into_iter()
                    .map(|style| {
                        if style.display_name.is_empty() {
                            style.name
                        } else {
                            style.display_name
                        }
                    })
                    .filter(|style| !style.is_empty())
                    .collect(),
                tos_link: model.tos_link,
            }
        })
        .collect())
}

#[derive(Debug, Default, Deserialize)]
struct RawUsageList {
    #[serde(default)]
    results: Vec<RawUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct RawUsage {
    #[serde(default)]
    time: String,
    requests: Option<RawUsageRequests>,
    #[serde(default)]
    chunk_count: i64,
}

#[derive(Debug, Default, Deserialize)]
struct RawUsageRequests {
    #[serde(default)]
    chat_completions: i64,
    #[serde(default)]
    embeddings: i64,
    #[serde(default)]
    audio_transcriptions: i64,
    #[serde(default)]
    audio_speeches: i64,
}

fn parse_usages(text: &str) -> Result<Vec<CloudUsage>> {
    let raw: RawUsageList = serde_json::from_str(text).context("利用状況の解析に失敗しました")?;
    Ok(raw
        .results
        .into_iter()
        .map(|item| {
            let requests = item.requests.unwrap_or_default();
            let breakdown = [
                ("チャット生成", requests.chat_completions),
                ("埋め込み", requests.embeddings),
                ("文字起こし", requests.audio_transcriptions),
                ("音声合成", requests.audio_speeches),
            ];
            let total = breakdown.iter().map(|(_, count)| count).sum();
            CloudUsage {
                time: item.time,
                total,
                // 0件の内訳まで並べると、使った分が埋もれる。
                details: breakdown
                    .into_iter()
                    .filter(|(_, count)| *count > 0)
                    .map(|(label, count)| CloudField::new(label, count.to_string()))
                    .collect(),
            }
        })
        .collect())
}

fn parse_document_usages(text: &str) -> Result<Vec<CloudDocumentUsage>> {
    let raw: RawUsageList =
        serde_json::from_str(text).context("ドキュメント利用状況の解析に失敗しました")?;
    Ok(raw
        .results
        .into_iter()
        .map(|item| CloudDocumentUsage {
            time: item.time,
            chunk_count: item.chunk_count,
        })
        .collect())
}

#[derive(Debug, Default, Deserialize)]
struct RawBill {
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    bill_close_date: String,
    #[serde(default)]
    details: Vec<RawBillDetail>,
}

#[derive(Debug, Default, Deserialize)]
struct RawBillDetail {
    #[serde(default)]
    no: i64,
    #[serde(default)]
    usage_type: String,
    #[serde(default)]
    usage: f64,
    #[serde(default)]
    amount: f64,
    #[serde(default)]
    description: String,
}

fn parse_bill(year_month: &str, text: &str) -> Result<CloudBill> {
    let raw: RawBill = serde_json::from_str(text).context("請求の解析に失敗しました")?;
    Ok(CloudBill {
        year_month: year_month.to_string(),
        updated_at: raw.updated_at,
        close_date: raw.bill_close_date,
        details: raw
            .details
            .into_iter()
            .map(|detail| CloudBillDetail {
                no: detail.no,
                usage_type: detail.usage_type,
                usage: detail.usage,
                amount: detail.amount,
                description: detail.description,
            })
            .collect(),
    })
}

/// 金額と利用量は整数で返ることが多い。小数点以下が無ければ落として読みやすくする。
pub fn format_amount(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
    }
}

/// APIのエラー本文から人が読む部分だけを取り出す。
///
/// 権限エラーは `error_msg`、入力エラーは項目名ごとの配列で返ってくる。
fn extract_error_detail(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return body.trim().to_string();
    };
    if let Some(detail) = ["error_msg", "message", "detail", "error"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(scalar_text))
    {
        return detail;
    }
    if let Value::Object(fields) = &value {
        let messages: Vec<String> = fields
            .iter()
            .filter_map(|(key, value)| {
                let text = match value {
                    Value::Array(items) => items
                        .iter()
                        .filter_map(scalar_text)
                        .collect::<Vec<_>>()
                        .join(" "),
                    other => scalar_text(other)?,
                };
                (!text.is_empty()).then(|| format!("{key}: {text}"))
            })
            .collect();
        if !messages.is_empty() {
            return messages.join(", ");
        }
    }
    body.trim().to_string()
}

fn format_error(status: StatusCode, body: &str, token: &str, secret: &str) -> String {
    let detail = sanitize_detail(&extract_error_detail(body), token, secret);
    let hint = match status {
        StatusCode::UNAUTHORIZED => "  クラウドAPIキーを確認してください。",
        StatusCode::FORBIDDEN => {
            "  このAPIキーでは参照できません（コントロールパネルの会員ログインが必要な項目です）。"
        }
        _ => "",
    };
    if detail.is_empty() {
        format!("AI Engine コントロールパネルAPIエラー ({status}){hint}")
    } else {
        format!("AI Engine コントロールパネルAPIエラー ({status}): {detail}{hint}")
    }
}

/// エラー本文に資格情報が混ざっていても画面に出さない。
fn sanitize_detail(detail: &str, token: &str, secret: &str) -> String {
    let mut out = detail.to_string();
    for credential in [token, secret] {
        if !credential.is_empty() {
            out = out.replace(credential, "[REDACTED]");
        }
    }
    for header in ["Authorization", "authorization", "AUTHORIZATION"] {
        out = out.replace(header, "[REDACTED-HEADER]");
    }
    for marker in ["Basic ", "basic "] {
        redact_after_marker(&mut out, marker);
    }
    out.chars().take(200).collect()
}

fn redact_after_marker(text: &mut String, marker: &str) {
    let mut start_from = 0usize;
    while let Some(relative) = text[start_from..].find(marker) {
        let marker_pos = start_from + relative;
        let token_start = marker_pos + marker.len();
        let token_end = text[token_start..]
            .char_indices()
            .find_map(|(offset, ch)| {
                if ch.is_whitespace() || matches!(ch, ',' | ';' | '"' | '\'') {
                    Some(token_start + offset)
                } else {
                    None
                }
            })
            .unwrap_or(text.len());
        text.replace_range(token_start..token_end, "[REDACTED]");
        start_from = token_start + "[REDACTED]".len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    /// 本番（crane74）で実際に返ってきた本文を切り詰めたもの。
    const AUTH_BODY: &str = r#"{"user":null,"member":null,"account":{"member":{"member_id":"hyx53656"},"account_id":"113601034306","account_code":"crane74","account_name":"crane74","tos_agreed_at":"2025-08-21T18:11:11.495966+09:00","tos_agreed_version":null,"created_at":"2025-08-21T18:11:06.378402+09:00","plan":{"id":30001,"name":"従量課金プラン","label":"payg","request_limit_chat_completions":3000,"request_limit_embeddings":10000,"request_limit_audio_transcriptions":50,"request_limit_audio_speeches":50,"is_allow_pay_as_you_go":true,"is_new_contract_allowed":true,"price":0},"errors":[]}}"#;

    const MODELS_BODY: &str = r#"{"meta":{"page":1,"page_size":100,"total_pages":1,"count":1,"next":null,"previous":null},"results":[{"id":1,"name":"Qwen3-Coder-30B-A3B-Instruct","display_name":"","features":{"chat_completions":true,"embeddings":false,"audio_transcriptions":false,"audio_speeches":false},"tags":["コーディング"],"status":"deprecated","tos_link":"","styles":[]}]}"#;

    #[test]
    fn parses_account_and_plan_from_auth() {
        let auth = parse_auth(AUTH_BODY).unwrap();
        assert_eq!(auth.account_id, "113601034306");
        assert_eq!(auth.account_code, "crane74");
        assert_eq!(auth.member_id, "hyx53656");
        assert_eq!(auth.plan, "従量課金プラン");
        assert!(auth.created_at.starts_with("2025-08-21"));
        assert!(
            auth.plan_details
                .contains(&CloudField::new("プランID", "payg"))
        );
        assert!(auth.plan_details.contains(&CloudField::new(
            "上限",
            "チャット生成 3000 / 埋め込み 10000 / 文字起こし 50 / 音声合成 50",
        )));
        assert!(
            auth.plan_details
                .contains(&CloudField::new("従量課金", "可"))
        );
        assert!(auth.plan_details.contains(&CloudField::new("月額", "0円")));
    }

    #[test]
    fn rejects_auth_without_account() {
        assert!(parse_auth(r#"{"user":null,"member":null,"account":null}"#).is_err());
    }

    #[test]
    fn parses_models_with_features_and_falls_back_to_name() {
        let models = parse_models(MODELS_BODY).unwrap();
        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.id, "Qwen3-Coder-30B-A3B-Instruct");
        // display_name が空なら name をそのまま見出しにする。
        assert_eq!(model.name, "Qwen3-Coder-30B-A3B-Instruct");
        assert_eq!(model.status, "deprecated");
        assert_eq!(model.status_label(), "提供終了予定");
        assert_eq!(model.features, vec!["チャット生成"]);
        assert_eq!(model.tags, vec!["コーディング"]);
        assert_eq!(model.number, "1");
    }

    /// 音声合成モデルは声色を持つ。表示名が空なら内部名で埋める。
    #[test]
    fn parses_voice_styles_and_display_name() {
        let models = parse_models(
            r#"{"results":[{"id":300001,"name":"tohokuitako","display_name":"東北イタコ","features":{"audio_speeches":true},"tags":[],"status":"tos_agreement_required","tos_link":"https://example.com/tos","styles":[{"name":"normal","display_name":"ノーマル"},{"name":"sasayaki","display_name":""}]}]}"#,
        )
        .unwrap();
        let model = &models[0];
        assert_eq!(model.id, "tohokuitako");
        assert_eq!(model.name, "東北イタコ");
        assert_eq!(model.status_label(), "要規約同意");
        assert_eq!(model.features, vec!["音声合成"]);
        assert_eq!(model.styles, vec!["ノーマル", "sasayaki"]);
        assert_eq!(model.tos_link, "https://example.com/tos");
    }

    /// 絞り込みは表示名・モデルID・状態・用途・タグのどれでも当たる。
    #[test]
    fn model_search_covers_visible_columns() {
        let model = &parse_models(MODELS_BODY).unwrap()[0];
        let haystack = model.searchable();
        for needle in [
            "Qwen3-Coder",
            "提供終了予定",
            "チャット生成",
            "コーディング",
        ] {
            assert!(haystack.contains(needle), "{needle} が引っかからない");
        }
    }

    #[test]
    fn sums_request_usage_and_hides_unused_kinds() {
        let usages = parse_usages(
            r#"{"results":[{"time":"2026-09-01T00:00:00+09:00","type":"request","requests":{"chat_completions":18,"embeddings":3,"audio_transcriptions":0,"audio_speeches":0}}]}"#,
        )
        .unwrap();
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].total, 21);
        assert_eq!(
            usages[0].details,
            vec![
                CloudField::new("チャット生成", "18"),
                CloudField::new("埋め込み", "3"),
            ]
        );
    }

    #[test]
    fn parses_document_usage() {
        let usages = parse_document_usages(
            r#"{"results":[{"time":"2026-09-01T00:00:00+09:00","type":"document","chunk_count":2}]}"#,
        )
        .unwrap();
        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].chunk_count, 2);
    }

    #[test]
    fn parses_bill_details_and_total() {
        let bill = parse_bill(
            "202609",
            r#"{"updated_at":"2026-09-03T01:30:12.697666+09:00","bill_close_date":"2026-09-30","details":[{"no":1,"usage_type":"document_chunk","usage":2,"amount":3,"description":"ドキュメント チャンク利用料"},{"no":2,"usage_type":"chat","usage":10,"amount":7,"description":"チャット生成"}]}"#,
        )
        .unwrap();
        assert_eq!(bill.year_month, "202609");
        assert_eq!(bill.close_date, "2026-09-30");
        assert_eq!(bill.details.len(), 2);
        assert_eq!(bill.total(), 10.0);
        assert_eq!(format_amount(bill.total()), "10");
    }

    #[test]
    fn builds_api_root_without_zone_and_keeps_test_environment() {
        let creds = ApiCredentials {
            token: "t".to_string(),
            secret: "s".to_string(),
            source: crate::config::CredentialSource::Env,
            zone: None,
            api_root: Some("https://secure.sakura.ad.jp/cloud/zone".to_string()),
        };
        assert_eq!(
            api_root(&creds),
            "https://secure.sakura.ad.jp/cloud/api/ai/1.0"
        );
        let test_creds = ApiCredentials {
            api_root: Some("https://secure.sakura.ad.jp/cloud-test/zone".to_string()),
            ..creds
        };
        assert_eq!(
            api_root(&test_creds),
            "https://secure.sakura.ad.jp/cloud-test/api/ai/1.0"
        );
    }

    #[test]
    fn bill_path_requires_yyyymm() {
        assert_eq!(bill_path("202609").unwrap(), "/bills/202609/");
        assert!(bill_path("2026-09").is_err());
        assert!(bill_path("../auth").is_err());
    }

    #[test]
    fn error_messages_keep_credentials_out_and_explain_forbidden() {
        let forbidden = format_error(
            StatusCode::FORBIDDEN,
            r#"{"is_fatal":true,"status":"403 Forbidden","error_code":"forbidden","error_msg":"要求された操作は許可されていません"}"#,
            "secret-token-value",
            "secret-secret-value",
        );
        assert!(forbidden.contains("要求された操作は許可されていません"));
        assert!(forbidden.contains("会員ログイン"));

        let validation = format_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            r#"{"model":["この項目は必須です。"]}"#,
            "secret-token-value",
            "secret-secret-value",
        );
        assert!(validation.contains("model: この項目は必須です。"));

        let leaked = format_error(
            StatusCode::UNAUTHORIZED,
            r#"{"message":"Authorization: Basic secret-token-value is invalid"}"#,
            "secret-token-value",
            "secret-secret-value",
        );
        assert!(!leaked.contains("secret-token-value"));
        assert!(leaked.contains("[REDACTED]"));
    }

    struct ExpectedRequest {
        path: &'static str,
        response_body: &'static str,
    }

    fn spawn_test_server(expected: Vec<ExpectedRequest>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            for expected_request in expected {
                let mut stream = accept_with_timeout(&listener, Duration::from_secs(5));
                let request = read_http_request(&mut stream);
                assert_request(&request, expected_request.path);
                write_json_response(&mut stream, expected_request.response_body);
            }
        });
        (base_url, handle)
    }

    fn accept_with_timeout(listener: &TcpListener, timeout: Duration) -> TcpStream {
        let deadline = Instant::now() + timeout;
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "timed out waiting for request");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("failed to accept request: {err}"),
            }
        }
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut buf = [0u8; 4096];
        let mut request = Vec::new();
        loop {
            let read_size = stream.read(&mut buf).unwrap();
            if read_size == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read_size]);
            if request.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8(request).unwrap()
    }

    fn assert_request(request: &str, path: &str) {
        let mut lines = request.split("\r\n");
        let request_line = lines.next().unwrap_or_default();
        assert_eq!(request_line, format!("GET {path} HTTP/1.1"));
        let headers: Vec<&str> = lines.take_while(|line| !line.is_empty()).collect();
        // Basic base64("test-token:test-secret")
        assert!(headers.contains(&"authorization: Basic dGVzdC10b2tlbjp0ZXN0LXNlY3JldA=="));
        assert!(headers.contains(&"accept: application/json"));
    }

    fn write_json_response(stream: &mut TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    }

    #[tokio::test]
    async fn sends_basic_auth_to_slash_terminated_paths() {
        let (base_url, handle) = spawn_test_server(vec![
            ExpectedRequest {
                path: "/auth/",
                response_body: AUTH_BODY,
            },
            ExpectedRequest {
                path: "/models/?page_size=100",
                response_body: MODELS_BODY,
            },
            ExpectedRequest {
                path: "/usages/?type=request&start_at=2026-09-01&end_at=2026-09-03",
                response_body: r#"{"results":[]}"#,
            },
            ExpectedRequest {
                path: "/bills/202609/",
                response_body: r#"{"updated_at":"","bill_close_date":"2026-09-30","details":[]}"#,
            },
        ]);
        let client = AiEngineCloudClient::with_api_root(base_url).unwrap();
        assert_eq!(client.auth().await.unwrap().account_code, "crane74");
        assert_eq!(client.models().await.unwrap().len(), 1);
        assert!(
            client
                .request_usages("2026-09-01", "2026-09-03")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            client.bill("202609").await.unwrap().close_date,
            "2026-09-30"
        );
        handle.join().unwrap();
    }
}
