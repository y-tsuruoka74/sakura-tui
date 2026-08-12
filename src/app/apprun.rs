//! AppRun（共用型）画面の状態と操作。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::{ListState, TableState};

use super::{App, ConfirmAction, Loadable, Message, Overlay, Pane, StatusKind, fmt_error, matches};
use crate::apprun::{Application, ApplicationDetail, Traffic, Version};

/// AppRun 画面で選択中のペイン。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppRunPane {
    #[default]
    Applications,
    Versions,
}

#[derive(Debug, Default)]
pub struct AppRunView {
    pub applications: Loadable<Vec<Application>>,
    pub application_state: TableState,

    pub pane: AppRunPane,

    /// アプリ ID をキーにした詳細・バージョン・トラフィック。
    pub details: HashMap<String, Loadable<ApplicationDetail>>,
    pub versions: HashMap<String, Loadable<Vec<Version>>>,
    pub version_state: ListState,
    pub traffics: HashMap<String, Loadable<Vec<Traffic>>>,
}

impl App {
    // --- 表示中の要素 ---

    pub fn visible_applications(&self) -> Vec<&Application> {
        let Some(items) = self.apprun.applications.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Applications);
        items
            .iter()
            .filter(|app| matches(filter, &[&app.name, &app.status, &app.public_url]))
            .collect()
    }

    pub fn selected_application(&self) -> Option<&Application> {
        let index = self.apprun.application_state.selected()?;
        self.visible_applications().into_iter().nth(index)
    }

    pub fn selected_application_detail(&self) -> Loadable<ApplicationDetail> {
        self.selected_application()
            .and_then(|app| self.apprun.details.get(&app.id))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_versions(&self) -> Loadable<Vec<Version>> {
        let loadable = self
            .selected_application()
            .and_then(|app| self.apprun.versions.get(&app.id))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(versions) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            versions
                .into_iter()
                .filter(|v| matches(self.filters.get(Pane::Versions), &[&v.name, &v.status]))
                .collect(),
        )
    }

    pub fn selected_version(&self) -> Option<Version> {
        let index = self.apprun.version_state.selected()?;
        self.visible_versions().ready()?.get(index).cloned()
    }

    pub fn current_traffics(&self) -> Loadable<Vec<Traffic>> {
        self.selected_application()
            .and_then(|app| self.apprun.traffics.get(&app.id))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    /// トラフィック情報上で「最新バージョン」と印が付いているか。
    pub fn is_latest_version(&self, version_name: &str) -> bool {
        self.current_traffics().ready().is_some_and(|traffics| {
            traffics
                .iter()
                .any(|t| t.version_name == version_name && t.is_latest)
        })
    }

    /// トラフィックが向いている割合（バージョン名 → %）。
    pub fn traffic_percent(&self, version_name: &str) -> Option<i32> {
        self.current_traffics()
            .ready()?
            .iter()
            .find(|t| t.version_name == version_name)
            .map(|t| t.percent)
    }

    pub(super) fn apprun_active_pane(&self) -> Pane {
        match self.apprun.pane {
            AppRunPane::Applications => Pane::Applications,
            AppRunPane::Versions => Pane::Versions,
        }
    }

    // --- 読み込み ---

    pub(super) fn load_applications(&mut self) {
        self.apprun.applications = Loadable::Loading;
        self.inflight += 1;
        let client = self.apprun_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_applications().await.map_err(fmt_error);
            let _ = tx.send(Message::Applications(result));
        });
    }

    fn load_application_detail(&mut self, id: String) {
        self.apprun.details.insert(id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.apprun_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.application_detail(&id).await.map_err(fmt_error);
            let _ = tx.send(Message::ApplicationDetail { id, result });
        });
    }

    fn load_versions(&mut self, id: String) {
        self.apprun.versions.insert(id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.apprun_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_versions(&id).await.map_err(fmt_error);
            let _ = tx.send(Message::Versions { id, result });
        });
    }

    fn load_traffics(&mut self, id: String) {
        self.apprun.traffics.insert(id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.apprun_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_traffics(&id).await.map_err(fmt_error);
            let _ = tx.send(Message::Traffics { id, result });
        });
    }

    pub(super) fn apprun_ensure_loaded(&mut self) {
        if self.apprun.applications.is_idle() {
            self.load_applications();
            return;
        }
        // 絞り込みで件数が変わっても選択が空にならないようにする。
        if !self.visible_applications().is_empty()
            && self.apprun.application_state.selected().is_none()
        {
            self.apprun.application_state.select(Some(0));
        }
        let Some(id) = self.selected_application().map(|app| app.id.clone()) else {
            return;
        };
        if self.apprun.details.get(&id).is_none_or(Loadable::is_idle) {
            self.load_application_detail(id.clone());
        }
        if self.apprun.versions.get(&id).is_none_or(Loadable::is_idle) {
            self.load_versions(id.clone());
        }
        if self.apprun.traffics.get(&id).is_none_or(Loadable::is_idle) {
            self.load_traffics(id);
        }
        if self
            .visible_versions()
            .ready()
            .is_some_and(|v| !v.is_empty())
            && self.apprun.version_state.selected().is_none()
        {
            self.apprun.version_state.select(Some(0));
        }
    }

    pub(super) fn apprun_refresh(&mut self) {
        match self.selected_application().map(|app| app.id.clone()) {
            Some(id) if self.apprun.pane == AppRunPane::Versions => {
                self.load_versions(id.clone());
                self.load_traffics(id);
            }
            _ => self.load_applications(),
        }
    }

    // --- キー入力 ---

    pub(super) fn on_key_apprun(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.apprun.pane = AppRunPane::Applications,
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                self.apprun.pane = AppRunPane::Versions
            }
            // トラフィックを選択中のバージョンへ全振りする。
            KeyCode::Char('t') => self.confirm_route_traffic(),
            _ => {}
        }
    }

    fn confirm_route_traffic(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(app) = self.selected_application() else {
            return;
        };
        let (id, app_name) = (app.id.clone(), app.name.clone());
        let Some(version) = self.selected_version() else {
            self.set_status(
                "トラフィックを向けるバージョンを選択してください",
                StatusKind::Info,
            );
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "トラフィックの切り替え".to_string(),
            body: format!(
                "アプリ「{app_name}」のトラフィックを 100% バージョン「{}」に向けます。\n\
                 公開URLへのリクエストは即座にこのバージョンへ流れます。",
                version.name
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::RouteTraffic {
                application: id,
                app_name,
                version: version.name,
            },
        });
    }

    pub(super) fn run_route_traffic(&mut self, id: String, app_name: String, version: String) {
        let client = self.apprun_client.clone();
        let tx = self.tx.clone();
        let label = format!("「{app_name}」のトラフィックを {version} に切り替え");
        self.inflight += 1;
        let target = id.clone();
        tokio::spawn(async move {
            let result = client
                .route_all_traffic(&target, &version)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::AppRunAction { id, label, result });
        });
        self.set_status("送信中…", StatusKind::Info);
    }

    /// AppRun 関連のキャッシュを捨てる（認証情報の切り替え時など）。
    pub(super) fn apprun_invalidate(&mut self) {
        self.apprun = AppRunView::default();
    }
}
