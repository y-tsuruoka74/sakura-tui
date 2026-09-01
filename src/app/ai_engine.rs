//! AI Engine 画面の状態と操作。
//!
//! 推論API（モデル一覧）と RAG は同じホスト・同じアカウントトークンなので、
//! 1つのサービスにまとめてタブで切り替える。
//!
//! モデル一覧だけは `managed_resources` 経由のまま扱う。トークンから
//! クライアントを組み直す処理がそちらにあり、二重に持ちたくないため。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    App, Loadable, ManagedResourceKind, Message, Pane, child_id_to_load, fmt_error, matches,
};
use crate::ai_rag::{RagChunk, RagDocument};

/// トークン未設定のときの案内。モデル一覧側と同じ文言に揃える。
const TOKEN_REQUIRED: &str = "AI Engineには専用のアカウントトークンが必要です";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiEngineTab {
    #[default]
    Models,
    Documents,
}

impl AiEngineTab {
    pub const ALL: [AiEngineTab; 2] = [AiEngineTab::Models, AiEngineTab::Documents];

    pub fn title(self) -> &'static str {
        match self {
            AiEngineTab::Models => "モデル",
            AiEngineTab::Documents => "ドキュメント",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Default)]
pub struct AiEngineView {
    pub tab: AiEngineTab,
    pub documents: Loadable<Vec<RagDocument>>,
    pub document_state: TableState,
    /// ドキュメントごとのチャンク。キーはドキュメントの ID。
    pub chunks: HashMap<String, Loadable<Vec<RagChunk>>>,
    /// チャンク本文ペインのスクロール位置（行）。
    pub chunk_scroll: u16,
}

impl App {
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
        // モデル一覧はトークンからクライアントを組む処理を含むので、
        // 既存の managed_resources 側の入口をそのまま使う。
        self.managed_resources_ensure_loaded();

        if self.ai_engine.documents.is_idle() {
            self.load_ai_engine_documents();
        } else {
            self.fill_selection(Pane::AiEngineDocuments);
        }

        let document_id = self.selected_ai_engine_document().map(|d| d.id);
        if let Some(id) = child_id_to_load(document_id.clone(), &self.ai_engine.chunks) {
            self.load_ai_engine_chunks(id);
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
            // 本文のスクロール。上下は一覧の選択に使うので大文字を割り当てる。
            KeyCode::Char('J') => self.scroll_ai_engine_chunks(1),
            KeyCode::Char('K') => self.scroll_ai_engine_chunks(-1),
            KeyCode::Char('t') => self.open_ai_engine_token_form(),
            _ => {}
        }
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
        self.ai_engine.documents = Loadable::Idle;
        self.ai_engine.chunks.clear();
        self.ai_engine.document_state.select(None);
        self.ai_engine.chunk_scroll = 0;
        if self.service == super::Service::AiEngine {
            self.ai_engine_ensure_loaded();
        }
    }

    pub(super) fn ai_engine_refresh(&mut self) {
        self.managed_resources
            .items
            .remove(&ManagedResourceKind::AiEngine);
        self.managed_resources.state.select(None);
        self.ai_engine.documents = Loadable::Idle;
        self.ai_engine.chunks.clear();
        self.ai_engine.document_state.select(None);
        self.ai_engine.chunk_scroll = 0;
        self.ai_engine_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// タブの並びと巡回。2 つしかないので往復する。
    #[test]
    fn tabs_cycle_in_order_and_wrap() {
        let titles: Vec<&str> = AiEngineTab::ALL.iter().map(|tab| tab.title()).collect();
        assert_eq!(titles, vec!["モデル", "ドキュメント"]);

        assert_eq!(AiEngineTab::Models.cycled(1), AiEngineTab::Documents);
        assert_eq!(AiEngineTab::Documents.cycled(1), AiEngineTab::Models);
        assert_eq!(AiEngineTab::Models.cycled(-1), AiEngineTab::Documents);
    }

    /// 既定はモデル一覧。統合前に AI Engine を開いたときと同じ画面から始める。
    #[test]
    fn default_tab_is_the_model_list() {
        assert_eq!(AiEngineTab::default(), AiEngineTab::Models);
    }
}
