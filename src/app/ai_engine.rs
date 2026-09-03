//! AI Engine 画面の状態と操作。
//!
//! 2系統のAPIを1つのサービスにまとめてタブで切り替える。
//!
//! - コントロールパネルAPI（IaaSと同じAPIキー）… モデル・利用状況・請求・アカウント
//! - 推論API / RAG API（専用のアカウントトークン）… ドキュメントとチャンク
//!
//! コントロールパネルAPIが使えない資格情報のときだけ、モデル一覧は
//! アカウントトークンで引ける `managed_resources` 側へ落とす。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    App, ConfirmAction, Loadable, ManagedResourceKind, Message, Overlay, Pane, RagEditForm,
    RagUploadForm, StatusKind, child_id_to_load, fmt_error, matches,
};
use crate::ai_engine_cloud::{
    AiEngineCloudClient, CloudAuth, CloudBill, CloudDocumentUsage, CloudModel, CloudUsage,
};
#[cfg(test)]
use crate::ai_engine_cloud::{CloudBillDetail, CloudField};
use crate::ai_rag::{RagChunk, RagDocument, RagUpload};

/// トークン未設定のときの案内。モデル一覧側と同じ文言に揃える。
const TOKEN_REQUIRED: &str = "AI Engineには専用のアカウントトークンが必要です";

/// コントロールパネルAPIはIaaSと同じAPIキーで呼ぶ。アカウントトークンでは通らない。
const CREDENTIALS_REQUIRED: &str = "クラウドAPIキーが設定されていません";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiEngineTab {
    #[default]
    Models,
    Documents,
    Usage,
    Billing,
    Account,
}

impl AiEngineTab {
    pub const ALL: [AiEngineTab; 5] = [
        AiEngineTab::Models,
        AiEngineTab::Documents,
        AiEngineTab::Usage,
        AiEngineTab::Billing,
        AiEngineTab::Account,
    ];

    pub fn title(self) -> &'static str {
        match self {
            AiEngineTab::Models => "モデル",
            AiEngineTab::Documents => "ドキュメント",
            AiEngineTab::Usage => "利用状況",
            AiEngineTab::Billing => "請求",
            AiEngineTab::Account => "アカウント",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug)]
pub struct AiEngineView {
    pub tab: AiEngineTab,
    pub cloud_auth: Loadable<CloudAuth>,
    pub cloud_models: Loadable<Vec<CloudModel>>,
    pub usages: Loadable<Vec<CloudUsage>>,
    pub document_usages: Loadable<Vec<CloudDocumentUsage>>,
    pub bills: HashMap<String, Loadable<CloudBill>>,
    pub billing_month: String,
    pub model_state: TableState,
    pub documents: Loadable<Vec<RagDocument>>,
    pub document_state: TableState,
    /// ドキュメントごとのチャンク。キーはドキュメントの ID。
    pub chunks: HashMap<String, Loadable<Vec<RagChunk>>>,
    /// チャンク本文ペインのスクロール位置（行）。
    pub chunk_scroll: u16,
}

impl Default for AiEngineView {
    fn default() -> Self {
        Self {
            tab: AiEngineTab::default(),
            cloud_auth: Loadable::default(),
            cloud_models: Loadable::default(),
            usages: Loadable::default(),
            document_usages: Loadable::default(),
            bills: HashMap::new(),
            billing_month: current_billing_month(),
            model_state: TableState::default(),
            documents: Loadable::default(),
            document_state: TableState::default(),
            chunks: HashMap::new(),
            chunk_scroll: 0,
        }
    }
}

impl AiEngineView {
    pub(super) fn reset_cloud(&mut self) {
        self.cloud_auth = Loadable::Idle;
        self.cloud_models = Loadable::Idle;
        self.usages = Loadable::Idle;
        self.document_usages = Loadable::Idle;
        self.bills.clear();
        self.model_state.select(None);
    }
}

/// 利用状況の既定の期間（直近1か月）。API側で開始・終了の指定が必須。
fn usage_period() -> (String, String) {
    let today = chrono::Local::now().date_naive();
    let start = today - chrono::Duration::days(30);
    (
        start.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    )
}

fn current_billing_month() -> String {
    use chrono::Datelike;
    let now = chrono::Local::now();
    format!("{:04}{:02}", now.year(), now.month())
}

fn parse_yyyymm(value: &str) -> Option<(i32, i32)> {
    if value.len() != 6 || !value.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year = value[0..4].parse::<i32>().ok()?;
    let month = value[4..6].parse::<i32>().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    Some((year, month))
}

fn shift_billing_month(selected: &str, current: &str, delta: i32) -> String {
    let Some((selected_year, selected_month)) = parse_yyyymm(selected) else {
        return selected.to_string();
    };
    let Some((current_year, current_month)) = parse_yyyymm(current) else {
        return selected.to_string();
    };

    let selected_total = selected_year * 12 + (selected_month - 1);
    let current_total = current_year * 12 + (current_month - 1);
    let shifted = selected_total.saturating_add(delta).min(current_total);
    if shifted < 0 {
        return selected.to_string();
    }
    let year = shifted / 12;
    let month = shifted % 12 + 1;
    format!("{year:04}{month:02}")
}

impl App {
    /// モデル一覧をコントロールパネルAPI側で描いているか。
    ///
    /// 取得できないときは推論API側（マネージドリソース）の一覧に落ちる。
    /// 絞り込み・選択・コピーの対象を、実際に描いている方へ合わせるのに使う。
    pub fn ai_engine_shows_cloud_models(&self) -> bool {
        !matches!(self.ai_engine.cloud_models, Loadable::Failed(_))
            && !matches!(self.ai_engine.cloud_auth, Loadable::Failed(_))
    }

    pub fn visible_ai_engine_cloud_models(&self) -> Loadable<Vec<CloudModel>> {
        let Loadable::Ready(items) = &self.ai_engine.cloud_models else {
            return self.ai_engine.cloud_models.clone();
        };
        let filter = self.filters.get(Pane::AiEngineModels);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| matches(filter, &[&item.searchable()]))
                .cloned()
                .collect(),
        )
    }

    pub fn selected_ai_engine_cloud_model(&self) -> Option<CloudModel> {
        let index = self.ai_engine.model_state.selected()?;
        self.visible_ai_engine_cloud_models()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_ai_engine_documents(&self) -> Loadable<Vec<RagDocument>> {
        let Loadable::Ready(items) = &self.ai_engine.documents else {
            return self.ai_engine.documents.clone();
        };
        let filter = self.filters.get(Pane::AiEngineDocuments);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.status_label(),
                            &item.model,
                            &item.tags.join(","),
                            &item.error_message,
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_ai_engine_document(&self) -> Option<RagDocument> {
        let index = self.ai_engine.document_state.selected()?;
        self.visible_ai_engine_documents()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_ai_engine_chunks(&self) -> Loadable<Vec<RagChunk>> {
        let Some(document) = self.selected_ai_engine_document() else {
            return Loadable::Idle;
        };
        let loadable = self
            .ai_engine
            .chunks
            .get(&document.id)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        Loadable::Ready(items)
    }

    /// チャンク本文ペインをスクロールする。
    ///
    /// 上下キーはドキュメント一覧の選択に使っているので、本文側は J / K で送る。
    pub(super) fn scroll_ai_engine_chunks(&mut self, delta: i32) {
        let scroll = &mut self.ai_engine.chunk_scroll;
        *scroll = if delta < 0 {
            scroll.saturating_sub(delta.unsigned_abs() as u16)
        } else {
            scroll.saturating_add(delta as u16)
        };
    }

    pub(super) fn ai_engine_ensure_loaded(&mut self) {
        match self.ai_engine.tab {
            AiEngineTab::Models => {
                self.managed_resources_ensure_loaded();
                if self.ensure_ai_engine_cloud_auth() {
                    self.load_ai_engine_cloud_models();
                    self.fill_selection(Pane::AiEngineModels);
                }
            }
            AiEngineTab::Documents => {
                if self.ai_engine.documents.is_idle() {
                    self.load_ai_engine_documents();
                } else {
                    self.fill_selection(Pane::AiEngineDocuments);
                }
                let document_id = self.selected_ai_engine_document().map(|d| d.id);
                if let Some(id) = child_id_to_load(document_id, &self.ai_engine.chunks) {
                    self.load_ai_engine_chunks(id);
                }
            }
            AiEngineTab::Usage => {
                if self.ensure_ai_engine_cloud_auth() {
                    self.load_ai_engine_cloud_usages();
                }
            }
            AiEngineTab::Billing => {
                if self.ensure_ai_engine_cloud_auth() {
                    self.load_ai_engine_cloud_billing();
                }
            }
            AiEngineTab::Account => {
                self.ensure_ai_engine_cloud_auth();
            }
        }
    }

    fn ai_engine_cloud_client(&self) -> Result<std::sync::Arc<AiEngineCloudClient>, String> {
        if !self.has_credentials {
            return Err(CREDENTIALS_REQUIRED.to_string());
        }
        Ok(self.ai_engine_cloud_client.clone())
    }

    fn ensure_ai_engine_cloud_auth(&mut self) -> bool {
        match &self.ai_engine.cloud_auth {
            Loadable::Ready(_) => return true,
            Loadable::Loading | Loadable::Failed(_) => return false,
            Loadable::Idle => {}
        }
        let client = match self.ai_engine_cloud_client() {
            Ok(client) => client,
            Err(err) => {
                self.ai_engine.cloud_auth = Loadable::Failed(err);
                return false;
            }
        };
        self.ai_engine.cloud_auth = Loadable::Loading;
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.auth().await.map_err(fmt_error);
            let _ = tx.send(Message::AiEngineCloudAuth { result });
        });
        false
    }

    fn load_ai_engine_cloud_models(&mut self) {
        if !self.ai_engine.cloud_models.is_idle() {
            return;
        }
        let Ok(client) = self.ai_engine_cloud_client() else {
            return;
        };
        self.ai_engine.cloud_models = Loadable::Loading;
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.models().await.map_err(fmt_error);
            let _ = tx.send(Message::AiEngineCloudModels { result });
        });
    }

    fn load_ai_engine_cloud_usages(&mut self) {
        let Ok(client) = self.ai_engine_cloud_client() else {
            return;
        };
        // 期間の指定は必須。直近1か月を既定にする。
        let (start, end) = usage_period();
        if self.ai_engine.usages.is_idle() {
            self.ai_engine.usages = Loadable::Loading;
            self.inflight += 1;
            let tx = self.tx.clone();
            let client = client.clone();
            let (start, end) = (start.clone(), end.clone());
            tokio::spawn(async move {
                let result = client.request_usages(&start, &end).await.map_err(fmt_error);
                let _ = tx.send(Message::AiEngineCloudUsages { result });
            });
        }
        if self.ai_engine.document_usages.is_idle() {
            self.ai_engine.document_usages = Loadable::Loading;
            self.inflight += 1;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client
                    .document_usages(&start, &end)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::AiEngineCloudDocumentUsages { result });
            });
        }
    }

    fn load_ai_engine_cloud_billing(&mut self) {
        let Ok(client) = self.ai_engine_cloud_client() else {
            return;
        };
        let month = self.ai_engine.billing_month.clone();
        if self
            .ai_engine
            .bills
            .get(&month)
            .is_none_or(Loadable::is_idle)
        {
            self.ai_engine
                .bills
                .insert(month.clone(), Loadable::Loading);
            self.inflight += 1;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.bill(&month).await.map_err(fmt_error);
                let _ = tx.send(Message::AiEngineCloudBill { month, result });
            });
        }
    }

    fn load_ai_engine_documents(&mut self) {
        let Some(client) = self.ai_engine_client.clone() else {
            self.ai_engine.documents = Loadable::Failed(TOKEN_REQUIRED.to_string());
            return;
        };
        self.ai_engine.documents = Loadable::Loading;
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_rag_documents().await.map_err(fmt_error);
            let _ = tx.send(Message::AiEngineDocuments { result });
        });
    }

    fn load_ai_engine_chunks(&mut self, document_id: String) {
        let Some(client) = self.ai_engine_client.clone() else {
            self.ai_engine
                .chunks
                .insert(document_id, Loadable::Failed(TOKEN_REQUIRED.to_string()));
            return;
        };
        self.ai_engine
            .chunks
            .insert(document_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .list_rag_chunks(&document_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::AiEngineChunks {
                document_id,
                result,
            });
        });
    }

    pub(super) fn on_key_ai_engine(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.cycle_ai_engine_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_ai_engine_tab(1),
            KeyCode::Char('1') => self.ai_engine.tab = AiEngineTab::Models,
            KeyCode::Char('2') => self.ai_engine.tab = AiEngineTab::Documents,
            KeyCode::Char('3') => self.ai_engine.tab = AiEngineTab::Usage,
            KeyCode::Char('4') => self.ai_engine.tab = AiEngineTab::Billing,
            KeyCode::Char('5') => self.ai_engine.tab = AiEngineTab::Account,
            KeyCode::Char('[') if self.ai_engine.tab == AiEngineTab::Billing => {
                self.shift_ai_engine_billing_month(-1)
            }
            KeyCode::Char(']') if self.ai_engine.tab == AiEngineTab::Billing => {
                self.shift_ai_engine_billing_month(1)
            }
            // 本文のスクロール。上下は一覧の選択に使うので大文字を割り当てる。
            KeyCode::Char('J') => self.scroll_ai_engine_chunks(1),
            KeyCode::Char('K') => self.scroll_ai_engine_chunks(-1),
            KeyCode::Char('t') => self.open_ai_engine_token_form(),
            // 書き込み系はドキュメントタブでだけ受ける。
            KeyCode::Char('n') if self.ai_engine.tab == AiEngineTab::Documents => {
                self.open_rag_upload_form()
            }
            KeyCode::Char('d') if self.ai_engine.tab == AiEngineTab::Documents => {
                self.confirm_delete_rag_document()
            }
            KeyCode::Char('e') if self.ai_engine.tab == AiEngineTab::Documents => {
                self.open_rag_edit_form()
            }
            _ => {}
        }
    }

    fn shift_ai_engine_billing_month(&mut self, delta: i32) {
        self.ai_engine.billing_month = shift_billing_month(
            &self.ai_engine.billing_month,
            &current_billing_month(),
            delta,
        );
    }

    fn open_rag_upload_form(&mut self) {
        if !self.require_write() {
            return;
        }
        if self.ai_engine_client.is_none() {
            self.set_status(TOKEN_REQUIRED, StatusKind::Error);
            return;
        }
        self.overlay = Some(Overlay::RagUploadForm(RagUploadForm::default()));
    }

    pub(super) fn submit_rag_upload_form(&mut self, form: RagUploadForm) {
        let path = form.path.trim().to_string();
        if path.is_empty() {
            self.overlay = Some(Overlay::RagUploadForm(form));
            self.set_status("ファイルのパスを入力してください", StatusKind::Error);
            return;
        }
        let Some(client) = self.ai_engine_client.clone() else {
            self.set_status(TOKEN_REQUIRED, StatusKind::Error);
            return;
        };
        let input = RagUpload {
            path,
            name: form.name.trim().to_string(),
            tags: form.tag_list(),
            model: form.model.trim().to_string(),
            chunk_size: form.chunk_size.trim().to_string(),
        };
        self.overlay = None;
        self.set_status("アップロードしています…", StatusKind::Info);
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.upload_rag_document(input).await.map_err(fmt_error);
            let _ = tx.send(Message::RagDocumentUploaded { result });
        });
    }

    fn open_rag_edit_form(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(document) = self.selected_ai_engine_document() else {
            self.set_status("編集するドキュメントを選んでください", StatusKind::Error);
            return;
        };
        self.overlay = Some(Overlay::RagEditForm(RagEditForm {
            id: document.id,
            original_name: document.name.clone(),
            name: document.name,
            tags: document.tags.join(", "),
            field: 0,
        }));
    }

    pub(super) fn submit_rag_edit_form(&mut self, form: RagEditForm) {
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.overlay = Some(Overlay::RagEditForm(form));
            self.set_status("名前を入力してください", StatusKind::Error);
            return;
        }
        let Some(client) = self.ai_engine_client.clone() else {
            self.set_status(TOKEN_REQUIRED, StatusKind::Error);
            return;
        };
        let tags = form.tag_list();
        let id = form.id;
        self.overlay = None;
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .update_rag_document(&id, &name, tags)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::RagDocumentUpdated { result });
        });
    }

    fn confirm_delete_rag_document(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(document) = self.selected_ai_engine_document() else {
            self.set_status("削除するドキュメントを選んでください", StatusKind::Error);
            return;
        };
        // 取り返しがつかないので、名前の入力を要求する。
        self.overlay = Some(Overlay::Confirm {
            title: "ドキュメントの削除".to_string(),
            body: format!(
                "ドキュメント「{}」を削除します。チャンクも一緒に消え、元に戻せません。\n\
                 実行するには名前を入力してください。",
                document.name
            ),
            verify: Some(document.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteRagDocument {
                id: document.id,
                name: document.name,
            },
        });
    }

    pub(super) fn run_delete_rag_document(&mut self, id: String, name: String) {
        let Some(client) = self.ai_engine_client.clone() else {
            self.set_status(TOKEN_REQUIRED, StatusKind::Error);
            return;
        };
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.delete_rag_document(&id).await.map_err(fmt_error);
            let _ = tx.send(Message::RagDocumentDeleted { name, result });
        });
    }

    fn cycle_ai_engine_tab(&mut self, delta: i32) {
        self.ai_engine.tab = self.ai_engine.tab.cycled(delta);
    }

    /// AI Engine トークンが変わったら RAG 側のキャッシュも捨てる。
    ///
    /// モデル一覧と同じトークンを使うため、これを忘れるとトークン登録後も
    /// 「トークンが必要です」という失敗が残り続ける。`ensure_loaded` は
    /// `Idle` のときしか読み直さないので、失敗のままでは復帰できない。
    pub(super) fn ai_engine_reset_rag(&mut self) {
        self.ai_engine.reset_cloud();
        self.ai_engine.documents = Loadable::Idle;
        self.ai_engine.chunks.clear();
        self.ai_engine.document_state.select(None);
        self.ai_engine.chunk_scroll = 0;
        if self.service == super::Service::AiEngine {
            self.ai_engine_ensure_loaded();
        }
    }

    pub(super) fn ai_engine_refresh(&mut self) {
        match self.ai_engine.tab {
            AiEngineTab::Models => {
                self.managed_resources
                    .items
                    .remove(&ManagedResourceKind::AiEngine);
                self.managed_resources.state.select(None);
                self.ai_engine.cloud_auth = Loadable::Idle;
                self.ai_engine.cloud_models = Loadable::Idle;
            }
            AiEngineTab::Documents => {
                self.ai_engine.documents = Loadable::Idle;
                self.ai_engine.chunks.clear();
                self.ai_engine.document_state.select(None);
                self.ai_engine.chunk_scroll = 0;
            }
            AiEngineTab::Usage => {
                self.ai_engine.cloud_auth = Loadable::Idle;
                self.ai_engine.usages = Loadable::Idle;
                self.ai_engine.document_usages = Loadable::Idle;
            }
            AiEngineTab::Billing => {
                self.ai_engine.cloud_auth = Loadable::Idle;
                self.ai_engine.bills.remove(&self.ai_engine.billing_month);
            }
            AiEngineTab::Account => {
                self.ai_engine.cloud_auth = Loadable::Idle;
            }
        }
        self.ai_engine_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// タブの並び・表示名・既定値・巡回を固定する。
    #[test]
    fn tabs_have_fixed_order_titles_default_and_wrap() {
        assert_eq!(
            AiEngineTab::ALL,
            [
                AiEngineTab::Models,
                AiEngineTab::Documents,
                AiEngineTab::Usage,
                AiEngineTab::Billing,
                AiEngineTab::Account,
            ]
        );
        let titles: Vec<&str> = AiEngineTab::ALL.iter().map(|tab| tab.title()).collect();
        assert_eq!(
            titles,
            vec!["モデル", "ドキュメント", "利用状況", "請求", "アカウント"]
        );
        assert_eq!(AiEngineTab::default(), AiEngineTab::Models);
        assert_eq!(AiEngineTab::Models.cycled(1), AiEngineTab::Documents);
        assert_eq!(AiEngineTab::Account.cycled(1), AiEngineTab::Models);
        assert_eq!(AiEngineTab::Models.cycled(-1), AiEngineTab::Account);
    }

    /// タグはカンマ区切りで受け、空要素と前後の空白は捨てる。
    #[test]
    fn upload_form_splits_tags() {
        let form = RagUploadForm {
            tags: " manual , ja ,, ".to_string(),
            ..RagUploadForm::default()
        };
        assert_eq!(form.tag_list(), vec!["manual", "ja"]);

        // 未入力ならタグ無しで送る。
        assert!(RagUploadForm::default().tag_list().is_empty());
    }

    /// 編集フォームは名前とタグの2項目だけ。読み取り専用の項目を送らない。
    #[test]
    fn edit_form_has_only_the_writable_fields() {
        assert_eq!(RagEditForm::LABELS.len(), 2);
        let form = RagEditForm {
            id: "d1".to_string(),
            original_name: "旧名".to_string(),
            name: "新名".to_string(),
            tags: " a , b ,, ".to_string(),
            field: 0,
        };
        assert_eq!(form.value(0), "新名");
        assert_eq!(form.value(1), " a , b ,, ");
        assert_eq!(form.tag_list(), vec!["a", "b"]);
        assert_eq!(form.value(2), "");
        // 変更前の名前は確認の文言に使うので保持する。
        assert_eq!(form.original_name, "旧名");
    }

    /// 入力欄の並びとラベルを固定する。value の対応がずれると別の値を送ってしまう。
    #[test]
    fn upload_form_fields_match_their_labels() {
        let form = RagUploadForm {
            path: "/tmp/a.txt".to_string(),
            name: "名前".to_string(),
            tags: "t".to_string(),
            model: "m".to_string(),
            chunk_size: "512".to_string(),
            field: 0,
        };
        assert_eq!(RagUploadForm::LABELS.len(), 5);
        assert_eq!(form.value(0), "/tmp/a.txt");
        assert_eq!(form.value(1), "名前");
        assert_eq!(form.value(2), "t");
        assert_eq!(form.value(3), "m");
        assert_eq!(form.value(4), "512");
        // 範囲外は空文字。描画側で落ちないようにする。
        assert_eq!(form.value(5), "");
    }

    #[test]
    fn billing_month_shifts_back_and_forward() {
        assert_eq!(shift_billing_month("202405", "202512", -1), "202404");
        assert_eq!(shift_billing_month("202405", "202512", 1), "202406");
    }

    #[test]
    fn billing_month_shift_handles_year_boundaries() {
        assert_eq!(shift_billing_month("202401", "202512", -1), "202312");
        assert_eq!(shift_billing_month("202412", "202512", 1), "202501");
    }

    #[test]
    fn billing_month_shift_caps_future_month() {
        assert_eq!(shift_billing_month("202512", "202512", 1), "202512");
        assert_eq!(shift_billing_month("202511", "202512", 2), "202512");
    }

    #[test]
    fn billing_month_shift_keeps_selected_on_malformed_input() {
        assert_eq!(shift_billing_month("bad", "202512", -1), "bad");
        assert_eq!(shift_billing_month("202501", "bad", 1), "202501");
        assert_eq!(shift_billing_month("202500", "202512", -1), "202500");
    }

    #[test]
    fn ai_engine_view_default_cloud_states_are_idle_and_bills_empty() {
        let view = AiEngineView::default();
        assert!(view.cloud_auth.is_idle());
        assert!(view.cloud_models.is_idle());
        assert!(view.usages.is_idle());
        assert!(view.document_usages.is_idle());
        assert!(view.bills.is_empty());
        assert_eq!(view.model_state.selected(), None);
        assert_eq!(view.billing_month, current_billing_month());
    }

    #[test]
    fn ai_engine_reset_cloud_clears_all_cloud_data_without_touching_billing_month() {
        let mut view = AiEngineView {
            tab: AiEngineTab::Billing,
            cloud_auth: Loadable::Ready(CloudAuth {
                account_id: "113601034306".to_string(),
                account_code: "crane74".to_string(),
                account_name: "crane74".to_string(),
                member_id: "hyx53656".to_string(),
                tos_agreed_at: "2025-08-21T18:11:11+09:00".to_string(),
                created_at: "2025-08-21T18:11:06+09:00".to_string(),
                plan: "従量課金プラン".to_string(),
                plan_details: vec![CloudField {
                    label: "プランID".to_string(),
                    value: "payg".to_string(),
                }],
            }),
            cloud_models: Loadable::Ready(vec![CloudModel {
                id: "gpt-oss-120b".to_string(),
                name: "gpt-oss-120b".to_string(),
                status: "available".to_string(),
                features: vec!["チャット生成".to_string()],
                tags: vec!["国産".to_string()],
                number: "30001".to_string(),
                styles: Vec::new(),
                tos_link: String::new(),
            }]),
            usages: Loadable::Ready(vec![CloudUsage {
                time: "2026-09-01T00:00:00+09:00".to_string(),
                total: 21,
                details: vec![CloudField {
                    label: "チャット生成".to_string(),
                    value: "18".to_string(),
                }],
            }]),
            document_usages: Loadable::Ready(vec![CloudDocumentUsage {
                time: "2026-09-01T00:00:00+09:00".to_string(),
                chunk_count: 2,
            }]),
            bills: HashMap::from([(
                "202401".to_string(),
                Loadable::Ready(CloudBill {
                    year_month: "202401".to_string(),
                    updated_at: "2024-01-31T01:30:00+09:00".to_string(),
                    close_date: "2024-01-31".to_string(),
                    details: vec![CloudBillDetail {
                        no: 1,
                        usage_type: "document_chunk".to_string(),
                        usage: 2.0,
                        amount: 3.0,
                        description: "ドキュメント チャンク利用料".to_string(),
                    }],
                }),
            )]),
            billing_month: "202401".to_string(),
            model_state: {
                let mut state = TableState::default();
                state.select(Some(3));
                state
            },
            documents: Loadable::default(),
            document_state: Default::default(),
            chunks: HashMap::from([(
                "doc1".to_string(),
                Loadable::Ready(vec![RagChunk {
                    index: 0,
                    content: "body".to_string(),
                    metadata: String::new(),
                    document_id: "doc1".to_string(),
                }]),
            )]),
            chunk_scroll: 9,
        };

        view.reset_cloud();

        assert!(view.cloud_auth.is_idle());
        assert!(view.cloud_models.is_idle());
        assert!(view.usages.is_idle());
        assert!(view.document_usages.is_idle());
        assert!(view.bills.is_empty());
        // 別アカウントの一覧に選択位置が残ると、無関係なモデルを指したままになる。
        assert_eq!(view.model_state.selected(), None);
        assert_eq!(view.billing_month, "202401");
    }
}
