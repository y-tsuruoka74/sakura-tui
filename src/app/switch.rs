//! スイッチ画面の状態と読み込み。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    App, ConfirmAction, Loadable, Message, Overlay, Pane, StatusKind, SwitchForm, SwitchFormMode,
    fmt_error, matches,
};
use crate::switch::Switch;

#[derive(Debug, Default)]
pub struct SwitchView {
    /// ゾーンごとのスイッチ一覧。
    pub switches: HashMap<String, Loadable<Vec<Switch>>>,
    pub switch_state: TableState,
}

impl App {
    pub fn visible_switches(&self) -> Loadable<Vec<Switch>> {
        let loadable = self
            .switch
            .switches
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(switches) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::Switches);
        Loadable::Ready(
            switches
                .into_iter()
                .filter(|switch| {
                    let id = switch.id.to_string();
                    let tags = switch.tags.join(" ");
                    matches(filter, &[&switch.name, &switch.description, &id, &tags])
                })
                .collect(),
        )
    }

    pub fn selected_switch(&self) -> Option<Switch> {
        let index = self.switch.switch_state.selected()?;
        self.visible_switches().ready()?.get(index).cloned()
    }

    pub(super) fn load_switches(&mut self, zone: String) {
        self.switch.switches.insert(zone.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_switches(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::Switches { zone, result });
        });
    }

    pub(super) fn switch_ensure_loaded(&mut self) {
        let zone = self.zone.clone();
        if self
            .switch
            .switches
            .get(&zone)
            .is_none_or(Loadable::is_idle)
        {
            self.load_switches(zone);
            return;
        }
        self.fill_selection(Pane::Switches);
    }

    pub(super) fn switch_refresh(&mut self) {
        let zone = self.zone.clone();
        self.load_switches(zone);
    }

    pub(super) fn on_key_switch(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => self.open_create_switch(),
            KeyCode::Char('E') => self.open_edit_switch(),
            KeyCode::Char('D') => self.confirm_delete_switch(),
            _ => {}
        }
    }

    fn open_create_switch(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::SwitchForm(SwitchForm {
            mode: SwitchFormMode::Create,
            target: None,
            name: String::new(),
            description: String::new(),
            field: 0,
        }));
    }

    fn open_edit_switch(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(target) = self.selected_switch() else {
            self.set_status("編集するスイッチを選択してください", StatusKind::Info);
            return;
        };
        self.overlay = Some(Overlay::SwitchForm(SwitchForm {
            mode: SwitchFormMode::Edit,
            name: target.name.clone(),
            description: target.description.clone(),
            target: Some(target),
            field: 0,
        }));
    }

    pub(super) fn submit_switch_form(&mut self, form: SwitchForm) {
        if let Err(message) = validate_switch_form(&form) {
            self.set_status(message, StatusKind::Error);
            self.overlay = Some(Overlay::SwitchForm(form));
            return;
        }

        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);

        match form.mode {
            SwitchFormMode::Create => {
                let label = format!("スイッチ「{}」を作成", form.name);
                let (name, description) = (form.name, form.description);
                let target_zone = zone.clone();
                tokio::spawn(async move {
                    let result = client
                        .create_switch(&target_zone, &name, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::SwitchAction {
                        zone,
                        label,
                        result,
                    });
                });
            }
            SwitchFormMode::Edit => {
                let Some(target) = form.target else {
                    self.inflight = self.inflight.saturating_sub(1);
                    return;
                };
                let label = format!("スイッチ「{}」を更新", form.name);
                let (name, description) = (form.name, form.description);
                let target_zone = zone.clone();
                tokio::spawn(async move {
                    let result = client
                        .update_switch(&target_zone, target.id, &name, &description)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::SwitchAction {
                        zone,
                        label,
                        result,
                    });
                });
            }
        }
    }

    fn confirm_delete_switch(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(target) = self.selected_switch() else {
            self.set_status("削除するスイッチを選択してください", StatusKind::Info);
            return;
        };
        if target.server_count > 0 || target.appliance_count > 0 {
            self.set_status(
                format!(
                    "スイッチ「{}」にはサーバー {} 台、アプライアンス {} 台が接続されているため削除できません",
                    target.name, target.server_count, target.appliance_count
                ),
                StatusKind::Error,
            );
            return;
        }

        self.overlay = Some(Overlay::Confirm {
            title: "スイッチの削除".to_string(),
            body: format!(
                "スイッチ「{}」({}) を削除します。\nこの操作は取り消せません。実行しますか？",
                target.name, self.zone
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteSwitch {
                zone: self.zone.clone(),
                id: target.id,
                name: target.name,
            },
        });
    }

    pub(super) fn run_delete_switch(
        &mut self,
        zone: String,
        id: crate::sacloud::ResourceId,
        name: String,
    ) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!("スイッチ「{name}」を削除");
        let target_zone = zone.clone();
        self.inflight += 1;
        tokio::spawn(async move {
            let result = client
                .delete_switch(&target_zone, id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::SwitchAction {
                zone,
                label,
                result,
            });
        });
        self.set_status("送信中…", StatusKind::Info);
    }

    pub(super) fn switch_invalidate(&mut self) {
        self.switch = SwitchView::default();
    }
}

fn validate_switch_form(form: &SwitchForm) -> Result<(), &'static str> {
    if form.name.trim().is_empty() {
        return Err("名前を入力してください");
    }
    if form.name.chars().count() > 64 {
        return Err("名前は64文字以内で入力してください");
    }
    if form.description.chars().count() > 512 {
        return Err("説明は512文字以内で入力してください");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(name: &str, description: &str) -> SwitchForm {
        SwitchForm {
            mode: SwitchFormMode::Create,
            target: None,
            name: name.to_string(),
            description: description.to_string(),
            field: 0,
        }
    }

    #[test]
    fn validates_switch_form() {
        assert!(validate_switch_form(&form("private", "internal network")).is_ok());
        assert_eq!(
            validate_switch_form(&form("   ", "")),
            Err("名前を入力してください")
        );
        assert_eq!(
            validate_switch_form(&form(&"あ".repeat(65), "")),
            Err("名前は64文字以内で入力してください")
        );
        assert_eq!(
            validate_switch_form(&form("private", &"あ".repeat(513))),
            Err("説明は512文字以内で入力してください")
        );
    }
}
