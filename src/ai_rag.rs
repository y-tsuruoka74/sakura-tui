//! さくらのAI Engine RAG の読み取り専用クライアント。
//!
//! ホストもトークンも推論API（`src/ai_engine.rs`）と同じ
//! `https://api.ai.sakura.ad.jp` の Bearer 認証なので、
//! [`AiEngineClient`] にメソッドを生やして相乗りする。
//! ただし「同じトークンで通る」ことは仕様に明記されていない。
//!
//! パスは末尾スラッシュが必須（Django REST framework 由来）。
//! ページングは `page` / `page_size` のページ番号方式で、
//! レスポンスの封筒は `meta` と `results`。
//!
//! 仕様に「データセット」という概念は無い。階層はドキュメントとチャンクの2段で、
//! グルーピングはタグで行う。

use anyhow::Result;
use serde::Deserialize;

use crate::ai_engine::AiEngineClient;

/// 1 ページあたりの取得件数。仕様に上限の記載が無いため控えめにする。
const PAGE_SIZE: usize = 100;
/// ページを辿る上限。
const MAX_PAGES: usize = 100;

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// RAG に取り込んだドキュメント 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RagDocument {
    pub id: String,
    pub name: String,
    /// `pending` / `processing` / `available` / `deleted` / `error`。
    pub status: String,
    /// 埋め込みモデル。仕様上 enum ではなく自由文字列。
    pub model: String,
    pub chunk_size: u32,
    pub chunk_count: u32,
    pub tags: Vec<String>,
    /// `status` が `error` のときの詳細。
    pub error_message: String,
    pub created_at: String,
}

impl RagDocument {
    pub fn status_label(&self) -> String {
        match self.status.as_str() {
            "pending" => "待機中".to_string(),
            "processing" => "処理中".to_string(),
            "available" => "利用可能".to_string(),
            "deleted" => "削除済み".to_string(),
            "error" => "エラー".to_string(),
            other => other.to_string(),
        }
    }

    pub fn failed(&self) -> bool {
        self.status == "error"
    }
}

/// アップロードの入力。空の項目はサービス側の既定に任せる。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RagUpload {
    pub path: String,
    pub name: String,
    pub tags: Vec<String>,
    pub model: String,
    pub chunk_size: String,
}

/// ドキュメントを分割したチャンク 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RagChunk {
    /// ドキュメント内の通番。
    pub index: u32,
    pub content: String,
    /// 任意の JSON。整形して 1 行にしてある。
    pub metadata: String,
    /// 属するドキュメントの ID。
    pub document_id: String,
}

// ---------------------------------------------------------------------------
// デシリアライズ用の内部型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Page<T> {
    #[serde(default)]
    meta: Option<PageMeta>,
    #[serde(default = "Vec::new")]
    results: Vec<Option<T>>,
}

#[derive(Debug, Deserialize)]
struct PageMeta {
    #[serde(default)]
    total_pages: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawDocument {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    chunk_size: Option<u32>,
    #[serde(default)]
    chunk_count: Option<u32>,
    /// 仕様上 required ではないので、キーごと欠けることがある。
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawChunk {
    #[serde(default)]
    document: Option<RawDocument>,
    #[serde(default)]
    chunk_index: Option<u32>,
    #[serde(default)]
    content: Option<String>,
    /// 任意の JSON。`null` にもなる。
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// パース
// ---------------------------------------------------------------------------

impl From<RawDocument> for RagDocument {
    fn from(raw: RawDocument) -> Self {
        RagDocument {
            id: raw.id.unwrap_or_default(),
            name: raw.name.unwrap_or_default(),
            status: raw.status.unwrap_or_default(),
            model: raw.model.unwrap_or_default(),
            chunk_size: raw.chunk_size.unwrap_or_default(),
            chunk_count: raw.chunk_count.unwrap_or_default(),
            tags: raw.tags.unwrap_or_default(),
            error_message: raw.error_message.unwrap_or_default(),
            created_at: raw.created_at.unwrap_or_default(),
        }
    }
}

/// メタデータの任意 JSON を 1 行の表示用文字列にする。
fn metadata_label(value: Option<serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::Object(map)) if map.is_empty() => String::new(),
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .map(|(key, value)| match value {
                serde_json::Value::String(text) => format!("{key}={text}"),
                other => format!("{key}={other}"),
            })
            .collect::<Vec<_>>()
            .join(" "),
        Some(serde_json::Value::String(text)) => text,
        Some(other) => other.to_string(),
    }
}

fn parse_documents(body: &str) -> Result<(Vec<RagDocument>, u32)> {
    let page: Page<RawDocument> = parse_json(body)?;
    let total_pages = page.meta.and_then(|m| m.total_pages).unwrap_or(1);
    Ok((
        page.results
            .into_iter()
            .flatten()
            .map(RagDocument::from)
            .collect(),
        total_pages,
    ))
}

fn parse_chunks(body: &str, document_id: &str) -> Result<(Vec<RagChunk>, u32)> {
    let page: Page<RawChunk> = parse_json(body)?;
    let total_pages = page.meta.and_then(|m| m.total_pages).unwrap_or(1);
    Ok((
        page.results
            .into_iter()
            .flatten()
            .map(|raw| RagChunk {
                index: raw.chunk_index.unwrap_or_default(),
                content: raw.content.unwrap_or_default(),
                metadata: metadata_label(raw.metadata),
                // 入れ子のドキュメントに ID があればそちらを信じる。
                document_id: raw
                    .document
                    .and_then(|d| d.id)
                    .unwrap_or_else(|| document_id.to_string()),
            })
            .collect(),
        total_pages,
    ))
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    use anyhow::Context;
    let body = if body.trim().is_empty() { "{}" } else { body };
    serde_json::from_str(body).with_context(|| {
        let head: String = body.chars().take(200).collect();
        format!("AI Engine RAG APIレスポンスの解析に失敗しました: {head}")
    })
}

fn page_query(page: usize) -> Vec<(&'static str, String)> {
    vec![
        ("page", page.to_string()),
        ("page_size", PAGE_SIZE.to_string()),
    ]
}

// ---------------------------------------------------------------------------
// API 呼び出し
// ---------------------------------------------------------------------------

impl AiEngineClient {
    /// ドキュメント一覧。
    ///
    /// 全文（`content`）は個別取得にしか無いが、同じ本文はチャンク側で
    /// 分割されて取れるので、ここでは引かない。
    pub async fn list_rag_documents(&self) -> Result<Vec<RagDocument>> {
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            // パスの末尾スラッシュは必須。省くとリダイレクトか404になる。
            let text = self.get_text("/v1/documents/", &page_query(page)).await?;
            let (items, total_pages) = parse_documents(&text)?;
            let received = items.len();
            out.extend(items);
            if received == 0 || page as u32 >= total_pages {
                break;
            }
        }
        Ok(out)
    }

    /// ドキュメントをアップロードする。
    ///
    /// `name` を空にするとファイル名が使われる。`model` と `chunk_size` も
    /// 空ならサービス側の既定値になるので、送らずに任せる。
    pub async fn upload_rag_document(&self, input: RagUpload) -> Result<RagDocument> {
        use anyhow::Context;
        let bytes = std::fs::read(&input.path)
            .with_context(|| format!("ファイルを読めませんでした: {}", input.path))?;
        let file_name = std::path::Path::new(&input.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "document".to_string());

        let mut form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(file_name),
        );
        if !input.name.is_empty() {
            form = form.text("name", input.name);
        }
        if !input.model.is_empty() {
            form = form.text("model", input.model);
        }
        if !input.chunk_size.is_empty() {
            form = form.text("chunk_size", input.chunk_size);
        }
        // タグは配列なので、同じキーを繰り返して送る。
        for tag in input.tags {
            form = form.text("tags", tag);
        }

        let text = self.post_multipart("/v1/documents/upload/", form).await?;
        let raw: RawDocument = parse_json(&text)?;
        Ok(RagDocument::from(raw))
    }

    /// ドキュメントを削除する。取り返しがつかない。
    pub async fn delete_rag_document(&self, document_id: &str) -> Result<()> {
        self.delete(&format!("/v1/documents/{document_id}/")).await
    }

    /// 指定ドキュメントのチャンク一覧。
    pub async fn list_rag_chunks(&self, document_id: &str) -> Result<Vec<RagChunk>> {
        let mut out = Vec::new();
        for page in 1..=MAX_PAGES {
            let path = format!("/v1/documents/{document_id}/chunks/");
            let text = self.get_text(&path, &page_query(page)).await?;
            let (items, total_pages) = parse_chunks(&text, document_id)?;
            let received = items.len();
            out.extend(items);
            if received == 0 || page as u32 >= total_pages {
                break;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 封筒は `meta` と `results`。DRF 標準の count/next/previous ではない。
    #[test]
    fn parses_documents_from_the_meta_results_envelope() {
        let body = r#"{
            "meta": {"page": 1, "page_size": 20, "total_pages": 2,
                     "count": 42, "next": "https://example/?page=2", "previous": null},
            "results": [{
                "id": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
                "created_at": "2026-06-29T12:34:56Z",
                "status": "available",
                "name": "manual.pdf",
                "model": "multilingual-e5-large",
                "chunk_size": 512,
                "chunk_count": 12,
                "tags": ["manual", "ja"],
                "error_message": ""
            }]
        }"#;
        let (items, total_pages) = parse_documents(body).unwrap();
        assert_eq!(total_pages, 2);
        let doc = &items[0];
        assert_eq!(doc.id, "3fa85f64-5717-4562-b3fc-2c963f66afa6");
        assert_eq!(doc.name, "manual.pdf");
        assert_eq!(doc.chunk_count, 12);
        assert_eq!(doc.tags, vec!["manual", "ja"]);
        assert_eq!(doc.status_label(), "利用可能");
        assert!(!doc.failed());
    }

    /// 処理状態は5種類。エラーは判別できるようにする。
    #[test]
    fn maps_every_processing_status() {
        let label = |status: &str| {
            RagDocument {
                status: status.to_string(),
                ..RagDocument::default()
            }
            .status_label()
        };
        assert_eq!(label("pending"), "待機中");
        assert_eq!(label("processing"), "処理中");
        assert_eq!(label("available"), "利用可能");
        assert_eq!(label("deleted"), "削除済み");
        assert_eq!(label("error"), "エラー");
        // 仕様に無い値でも落とさずそのまま出す。
        assert_eq!(label("unknown"), "unknown");

        let failed = RagDocument {
            status: "error".to_string(),
            error_message: "変換に失敗しました".to_string(),
            ..RagDocument::default()
        };
        assert!(failed.failed());
    }

    /// チャンクにはドキュメントが丸ごと入れ子になる。
    /// `metadata` は任意 JSON で null にもなる。
    #[test]
    fn parses_chunks_with_the_nested_document() {
        let body = r#"{
            "meta": {"page": 1, "total_pages": 1},
            "results": [
                {
                    "document": {"id": "doc-1", "name": "manual.pdf",
                                 "status": "available", "chunk_count": 2},
                    "chunk_index": 0,
                    "content": "チャンク本文",
                    "metadata": {"page": "3", "section": "intro"}
                },
                {
                    "document": {"id": "doc-1"},
                    "chunk_index": 1,
                    "content": "つづき",
                    "metadata": null
                }
            ]
        }"#;
        let (chunks, total_pages) = parse_chunks(body, "fallback").unwrap();
        assert_eq!(total_pages, 1);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].content, "チャンク本文");
        assert_eq!(chunks[0].document_id, "doc-1");
        // キー順は BTreeMap 準拠で安定する。
        assert_eq!(chunks[0].metadata, "page=3 section=intro");
        assert_eq!(chunks[1].metadata, "");
    }

    /// 入れ子のドキュメントに ID が無ければ、呼び出し側の ID で補う。
    #[test]
    fn chunk_falls_back_to_the_requested_document_id() {
        let body = r#"{"results": [{"chunk_index": 0, "content": "x", "document": null}]}"#;
        let (chunks, _) = parse_chunks(body, "doc-9").unwrap();
        assert_eq!(chunks[0].document_id, "doc-9");
    }

    /// `tags` は required ではないのでキーごと欠けることがある。
    /// null や欠けた項目でも落ちないこと。
    #[test]
    fn tolerates_missing_and_null_fields() {
        let body = r#"{"results": [null, {"id": "a", "name": null, "tags": null,
                                           "chunk_size": null}]}"#;
        let (items, total_pages) = parse_documents(body).unwrap();
        // meta が無ければ 1 ページとみなす。
        assert_eq!(total_pages, 1);
        assert_eq!(items.len(), 1);
        assert!(items[0].tags.is_empty());
        assert_eq!(items[0].chunk_size, 0);
        assert_eq!(items[0].status_label(), "");

        assert!(parse_documents("{}").unwrap().0.is_empty());
        assert!(parse_chunks("{}", "d").unwrap().0.is_empty());
    }

    /// メタデータは形がまちまちなので、1行の表示用に潰す。
    #[test]
    fn metadata_is_flattened_for_display() {
        assert_eq!(metadata_label(None), "");
        assert_eq!(metadata_label(Some(serde_json::json!({}))), "");
        assert_eq!(
            metadata_label(Some(serde_json::json!({"a": "1", "b": 2}))),
            "a=1 b=2"
        );
        assert_eq!(metadata_label(Some(serde_json::json!("plain"))), "plain");
    }

    /// ページングはページ番号方式。
    #[test]
    fn page_query_uses_page_numbers() {
        assert_eq!(
            page_query(2),
            vec![("page", "2".to_string()), ("page_size", "100".to_string())]
        );
    }
}
