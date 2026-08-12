//! サーバー画面の状態と操作。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, ConfirmAction, Loadable, Message, Overlay, Pane, StatusKind, fmt_error, matches};
use crate::iaas::{PowerAction, PowerStatus, Server};

#[derive(Debug, Default)]
pub struct ServerView {
    /// ゾーンごとのサーバー一覧。
    pub servers: HashMap<String, Loadable<Vec<Server>>>,
    pub server_state: TableState,
}

impl App {
    pub fn visible_servers(&self) -> Loadable<Vec<Server>> {
        let loadable = self
            .server
            .servers
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(servers) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::Servers);
        Loadable::Ready(
            servers
                .into_iter()
                .filter(|s| {
                    let ips = s.ip_addresses.join(" ");
                    matches(filter, &[&s.name, &s.host_name, &ips, &s.plan_name])
                })
                .collect(),
        )
    }

    pub fn selected_server(&self) -> Option<Server> {
        let index = self.server.server_state.selected()?;
        self.visible_servers().ready()?.get(index).cloned()
    }

    pub(super) fn load_servers(&mut self, zone: String) {
        self.server.servers.insert(zone.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_servers(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::Servers { zone, result });
        });
    }

    pub(super) fn server_ensure_loaded(&mut self) {
        let zone = self.zone.clone();
        if self.server.servers.get(&zone).is_none_or(Loadable::is_idle) {
            self.load_servers(zone);
            return;
        }
        if !self
            .visible_servers()
            .ready()
            .is_none_or(|servers| servers.is_empty())
            && self.server.server_state.selected().is_none()
        {
            self.server.server_state.select(Some(0));
        }
    }

    pub(super) fn server_refresh(&mut self) {
        let zone = self.zone.clone();
        self.load_servers(zone);
    }

    pub(super) fn on_key_server(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('b') => self.confirm_power(PowerAction::Boot),
            KeyCode::Char('x') => self.confirm_power(PowerAction::Shutdown),
            KeyCode::Char('X') => self.confirm_power(PowerAction::PowerOff),
            KeyCode::Char('B') => self.confirm_power(PowerAction::Reset),
            _ => {}
        }
    }

    fn confirm_power(&mut self, action: PowerAction) {
        if !self.require_write() {
            return;
        }
        let Some(server) = self.selected_server() else {
            return;
        };

        // 現在の電源状態と噛み合わない操作は、API を叩く前に止める。
        let mismatch = match (action, server.power) {
            (PowerAction::Boot, PowerStatus::Up) => Some("すでに起動しています"),
            (PowerAction::Shutdown | PowerAction::PowerOff, PowerStatus::Down) => {
                Some("すでに停止しています")
            }
            (PowerAction::Reset, PowerStatus::Down) => Some("停止中のため再起動できません"),
            _ => None,
        };
        if let Some(reason) = mismatch {
            self.set_status(format!("{}: {reason}", server.name), StatusKind::Info);
            return;
        }

        // 強制停止・強制リセットはデータを失いうるのでサーバー名の入力を求める。
        let verify = action.is_risky().then(|| server.name.clone());
        self.overlay = Some(Overlay::Confirm {
            title: format!("サーバーの{}", action.label()),
            body: format!(
                "サーバー「{}」({}) を{}します。\n{}{}",
                server.name,
                self.zone,
                action.label(),
                action.description(),
                if verify.is_some() {
                    "\n実行するにはサーバー名を入力してください。"
                } else {
                    ""
                }
            ),
            verify,
            typed: String::new(),
            action: ConfirmAction::PowerAction {
                id: server.id,
                zone: self.zone.clone(),
                name: server.name,
                action,
            },
        });
    }

    pub(super) fn run_power_action(
        &mut self,
        id: crate::sacloud::ResourceId,
        zone: String,
        name: String,
        action: PowerAction,
    ) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!("サーバー「{name}」を{}", action.label());
        self.inflight += 1;
        let target_zone = zone.clone();
        tokio::spawn(async move {
            let result = client
                .power_action(&target_zone, id, action)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::ServerAction {
                zone,
                label,
                result,
            });
        });
        self.set_status("送信中…", StatusKind::Info);
    }

    pub(super) fn server_invalidate(&mut self) {
        self.server = ServerView::default();
    }
}
