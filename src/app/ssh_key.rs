//! SSH 公開鍵の画面。
//!
//! 鍵はアカウント全体で共通で、ゾーンをまたいで同じものが見える。
//! API のパスにゾーンが要るので、表示中のゾーンで問い合わせる。

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    App, ConfirmAction, Loadable, Message, Overlay, Pane, SshKeyForm, SshKeyFormMode, SshKeyReturn,
    StatusKind, fmt_error, matches,
};
use crate::iaas::SshKey;
use crate::sacloud::ResourceId;

#[derive(Debug, Default)]
pub struct SshKeyView {
    pub keys: Loadable<Vec<SshKey>>,
    pub state: TableState,
}

impl App {
    pub fn visible_ssh_keys(&self) -> Loadable<Vec<SshKey>> {
        let Loadable::Ready(keys) = self.ssh_key.keys.clone() else {
            return self.ssh_key.keys.clone();
        };
        let filter = self.filters.get(Pane::SshKeys);
        Loadable::Ready(
            keys.into_iter()
                .filter(|k| matches(filter, &[&k.name, &k.description, &k.fingerprint]))
                .collect(),
        )
    }

    pub fn selected_ssh_key(&self) -> Option<SshKey> {
        let index = self.ssh_key.state.selected()?;
        self.visible_ssh_keys().ready()?.get(index).cloned()
    }

    pub(super) fn ssh_key_ensure_loaded(&mut self) {
        if self.ssh_key.keys.is_idle() {
            self.load_ssh_keys();
            return;
        }
        if !self
            .visible_ssh_keys()
            .ready()
            .is_none_or(|keys| keys.is_empty())
            && self.ssh_key.state.selected().is_none()
        {
            self.ssh_key.state.select(Some(0));
        }
    }

    fn load_ssh_keys(&mut self) {
        self.ssh_key.keys = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client.list_ssh_keys(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::SshKeyList { result });
        });
    }

    pub(super) fn ssh_key_refresh(&mut self) {
        self.load_ssh_keys();
    }

    pub(super) fn ssh_key_invalidate(&mut self) {
        self.ssh_key = SshKeyView::default();
    }

    pub(super) fn on_key_ssh_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => self.open_ssh_key_form(SshKeyFormMode::Add),
            KeyCode::Char('E') => self.open_ssh_key_form(SshKeyFormMode::Edit),
            KeyCode::Char('D') => self.confirm_delete_ssh_key(),
            _ => {}
        }
    }

    fn open_ssh_key_form(&mut self, mode: SshKeyFormMode) {
        if !self.require_write() {
            return;
        }
        let form = match mode {
            SshKeyFormMode::Add => SshKeyForm::default(),
            SshKeyFormMode::Edit => {
                let Some(key) = self.selected_ssh_key() else {
                    return;
                };
                SshKeyForm {
                    mode,
                    id: Some(key.id),
                    name: key.name,
                    description: key.description,
                    public_key: key.public_key,
                    field: 0,
                }
            }
        };
        self.overlay = Some(Overlay::SshKeyForm(form));
    }

    /// 公開鍵の欄から取得元の一覧を開く。
    pub(super) fn open_ssh_key_source_from_form(&mut self, form: SshKeyForm) {
        self.open_ssh_key_picker(SshKeyReturn::Register(form));
    }

    pub(super) fn submit_ssh_key_form(&mut self, form: SshKeyForm) {
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.overlay = Some(Overlay::SshKeyForm(form));
            self.set_status("名前を入力してください", StatusKind::Error);
            return;
        }
        let description = form.description.trim().to_string();
        match form.mode {
            SshKeyFormMode::Edit => {
                let Some(id) = form.id else {
                    return;
                };
                self.overlay = None;
                self.run_update_ssh_key(id, name, description);
            }
            SshKeyFormMode::Add => {
                let public_key = form.public_key.trim().to_string();
                if !crate::pubkey::looks_like_public_key(&public_key) {
                    self.overlay = Some(Overlay::SshKeyForm(form));
                    self.set_status(
                        "公開鍵の形式ではありません（Ctrl+K で選べます）",
                        StatusKind::Error,
                    );
                    return;
                }
                self.overlay = None;
                self.run_create_ssh_key(name, description, public_key);
            }
        }
    }

    fn run_create_ssh_key(&mut self, name: String, description: String, public_key: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client
                .create_ssh_key(&zone, &name, &description, &public_key)
                .await;
            let _ = tx.send(Message::SshKeyChanged {
                what: format!("公開鍵「{name}」を登録しました"),
                failed: "公開鍵の登録に失敗しました".to_string(),
                result: result.map(|_| ()).map_err(fmt_error),
            });
        });
    }

    fn run_update_ssh_key(&mut self, id: ResourceId, name: String, description: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client.update_ssh_key(&zone, id, &name, &description).await;
            let _ = tx.send(Message::SshKeyChanged {
                what: format!("公開鍵「{name}」を更新しました"),
                failed: "公開鍵の更新に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    fn confirm_delete_ssh_key(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(key) = self.selected_ssh_key() else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "公開鍵の削除".to_string(),
            body: format!(
                "公開鍵「{}」を削除します。\n\
                 {}\n\n\
                 すでにこの鍵で作ったサーバーには影響しません。\
                 元に戻せないので、実行するには名前を入力してください。",
                key.name, key.fingerprint
            ),
            verify: Some(key.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteSshKey {
                id: key.id,
                name: key.name,
            },
        });
    }

    pub(super) fn run_delete_ssh_key(&mut self, id: ResourceId, name: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client.delete_ssh_key(&zone, id).await;
            let _ = tx.send(Message::SshKeyChanged {
                what: format!("公開鍵「{name}」を削除しました"),
                failed: "公開鍵の削除に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }
}
