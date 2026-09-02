//! パケットフィルタ画面（フィルタ → ルール）。
//!
//! ルールは配列ごと差し替えるので、1本足す・消す・動かすのどれでも
//! 手元のルール全部を送り直す。取り違えないよう、送る前に必ず読み直して
//! `ExpressionHash` を取る。

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    App, ConfirmAction, ListFocus, Loadable, Message, Overlay, PacketFilterForm,
    PacketFilterFormMode, Pane, RuleForm, RuleFormMode, StatusKind, fmt_error, matches,
};
use crate::packet_filter::{PacketFilter, PacketFilterRule};
use crate::sacloud::ResourceId;

#[derive(Debug, Default)]
pub struct PacketFilterView {
    pub filters: Loadable<Vec<PacketFilter>>,
    pub filter_state: TableState,
    pub rule_state: TableState,
    pub focus: ListFocus,
    /// 更新後の再取得で同じフィルタを選び直すための ID。
    pub reselect: Option<ResourceId>,
}

impl App {
    pub fn visible_packet_filters(&self) -> Loadable<Vec<PacketFilter>> {
        let Loadable::Ready(filters) = self.packet_filter.filters.clone() else {
            return self.packet_filter.filters.clone();
        };
        let filter = self.filters.get(Pane::PacketFilters);
        Loadable::Ready(
            filters
                .into_iter()
                .filter(|f| matches(filter, &[&f.name, &f.description]))
                .collect(),
        )
    }

    pub fn selected_packet_filter(&self) -> Option<PacketFilter> {
        let index = self.packet_filter.filter_state.selected()?;
        self.visible_packet_filters().ready()?.get(index).cloned()
    }

    /// 選択中フィルタのルール。絞り込みはしない（順番に意味があるため）。
    pub fn visible_packet_filter_rules(&self) -> Vec<PacketFilterRule> {
        self.selected_packet_filter()
            .map(|f| f.rules)
            .unwrap_or_default()
    }

    pub fn selected_packet_filter_rule(&self) -> Option<(usize, PacketFilterRule)> {
        let index = self.packet_filter.rule_state.selected()?;
        let rule = self.visible_packet_filter_rules().get(index).cloned()?;
        Some((index, rule))
    }

    pub(super) fn packet_filter_active_pane(&self) -> Pane {
        match self.packet_filter.focus {
            ListFocus::Left => Pane::PacketFilters,
            ListFocus::Right => Pane::PacketFilterRules,
        }
    }

    pub(super) fn packet_filter_ensure_loaded(&mut self) {
        if self.packet_filter.filters.is_idle() {
            self.load_packet_filters();
            return;
        }
        let filters = self
            .visible_packet_filters()
            .ready()
            .cloned()
            .unwrap_or_default();
        if !filters.is_empty() && self.packet_filter.filter_state.selected().is_none() {
            self.packet_filter.filter_state.select(Some(0));
        }
        let rules = self.visible_packet_filter_rules().len();
        if rules == 0 {
            self.packet_filter.rule_state.select(None);
        } else if self.packet_filter.rule_state.selected().is_none() {
            self.packet_filter.rule_state.select(Some(0));
        }
    }

    fn load_packet_filters(&mut self) {
        self.packet_filter.filters = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client.list_packet_filters(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::PacketFilters { result });
        });
    }

    pub(super) fn packet_filters_arrived(&mut self, result: Result<Vec<PacketFilter>, String>) {
        let rule = self.packet_filter.rule_state.selected();
        self.packet_filter.filter_state.select(None);
        self.packet_filter.filters = self.store_result(result);
        // 更新のあとは、さっきまで見ていたフィルタに戻す。
        if let (Some(id), Some(filters)) = (
            self.packet_filter.reselect.take(),
            self.packet_filter.filters.ready(),
        ) && let Some(index) = filters.iter().position(|f| f.id == id)
        {
            self.packet_filter.filter_state.select(Some(index));
        }
        // 行数が減っていることがあるので、範囲に収めてから戻す。
        let rules = self.visible_packet_filter_rules().len();
        self.packet_filter
            .rule_state
            .select(rule.filter(|i| *i < rules));
        self.ensure_loaded();
    }

    pub(super) fn packet_filter_refresh(&mut self) {
        self.load_packet_filters();
    }

    pub(super) fn packet_filter_invalidate(&mut self) {
        // ルールを1本足すたびに一覧側へ戻されると編集を続けられないので、
        // 見ていた場所は持ち越す。
        let reselect = self.packet_filter.reselect;
        let focus = self.packet_filter.focus;
        let rule = self.packet_filter.rule_state.selected();
        self.packet_filter = PacketFilterView {
            reselect,
            focus,
            ..PacketFilterView::default()
        };
        self.packet_filter.rule_state.select(rule);
    }

    pub(super) fn on_key_packet_filter(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab | KeyCode::Left | KeyCode::Right => self.toggle_packet_filter_focus(),
            KeyCode::Char('n') => self.open_packet_filter_new(),
            KeyCode::Char('E') => self.open_packet_filter_edit(),
            KeyCode::Char('D') => self.confirm_packet_filter_delete(),
            // ルールは上から順に評価されるので、並べ替えられるようにする。
            KeyCode::Char('[') => self.move_rule(false),
            KeyCode::Char(']') => self.move_rule(true),
            _ => {}
        }
    }

    fn toggle_packet_filter_focus(&mut self) {
        self.packet_filter.focus = match self.packet_filter.focus {
            ListFocus::Left => ListFocus::Right,
            ListFocus::Right => ListFocus::Left,
        };
    }

    /// フィルタ側なら新しいフィルタ、ルール側なら新しいルール。
    fn open_packet_filter_new(&mut self) {
        if !self.require_write() {
            return;
        }
        match self.packet_filter.focus {
            ListFocus::Left => {
                self.overlay = Some(Overlay::PacketFilterForm(PacketFilterForm::default()));
            }
            ListFocus::Right => {
                if self.selected_packet_filter().is_none() {
                    return;
                }
                self.overlay = Some(Overlay::RuleForm(RuleForm::add()));
            }
        }
    }

    fn open_packet_filter_edit(&mut self) {
        if !self.require_write() {
            return;
        }
        match self.packet_filter.focus {
            ListFocus::Left => {
                let Some(filter) = self.selected_packet_filter() else {
                    return;
                };
                self.overlay = Some(Overlay::PacketFilterForm(PacketFilterForm {
                    mode: PacketFilterFormMode::Edit,
                    id: Some(filter.id),
                    name: filter.name,
                    description: filter.description,
                    field: 0,
                }));
            }
            ListFocus::Right => {
                let Some((index, rule)) = self.selected_packet_filter_rule() else {
                    return;
                };
                self.overlay = Some(Overlay::RuleForm(RuleForm::edit(index, &rule)));
            }
        }
    }

    pub(super) fn submit_packet_filter_form(&mut self, form: PacketFilterForm) {
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.overlay = Some(Overlay::PacketFilterForm(form));
            self.set_status("名前を入力してください", StatusKind::Error);
            return;
        }
        let description = form.description.trim().to_string();
        self.overlay = None;
        match (form.mode, form.id) {
            (PacketFilterFormMode::Create, _) => self.run_create_packet_filter(name, description),
            (PacketFilterFormMode::Edit, Some(id)) => {
                // 名前だけの変更でもルールは配列ごと送るので、今の中身を添える。
                let rules = self
                    .selected_packet_filter()
                    .map(|f| f.rules)
                    .unwrap_or_default();
                self.run_save_packet_filter(id, name, description, rules, "名前を変更しました");
            }
            (PacketFilterFormMode::Edit, None) => {}
        }
    }

    fn run_create_packet_filter(&mut self, name: String, description: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client
                .create_packet_filter(&zone, &name, &description)
                .await;
            let _ = tx.send(Message::PacketFilterChanged {
                what: format!("パケットフィルタ「{name}」を作成しました"),
                failed: "パケットフィルタの作成に失敗しました".to_string(),
                result: result.map(|_| ()).map_err(fmt_error),
            });
        });
    }

    /// ルールを含めて保存する。送る直前に読み直して最新の hash を取る。
    fn run_save_packet_filter(
        &mut self,
        id: ResourceId,
        name: String,
        description: String,
        rules: Vec<PacketFilterRule>,
        done: &str,
    ) {
        self.packet_filter.reselect = Some(id);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        let what = format!("パケットフィルタ「{name}」の{done}");
        tokio::spawn(async move {
            // 読んでから送るまでの間に他の人が変えていれば、API 側が弾く。
            let result = match client.get_packet_filter(&zone, id).await {
                Ok(current) => {
                    client
                        .update_packet_filter(
                            &zone,
                            id,
                            &name,
                            &description,
                            &rules,
                            &current.expression_hash,
                        )
                        .await
                }
                Err(err) => Err(err),
            };
            let _ = tx.send(Message::PacketFilterChanged {
                what,
                failed: "パケットフィルタの更新に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    pub(super) fn submit_rule_form(&mut self, form: RuleForm) {
        let Some(filter) = self.selected_packet_filter() else {
            return;
        };
        let rule = form.to_rule();
        if let Err(err) = form.validate() {
            self.overlay = Some(Overlay::RuleForm(form));
            self.set_status(err, StatusKind::Error);
            return;
        }
        let mut rules = filter.rules.clone();
        let done = match form.mode {
            RuleFormMode::Add => {
                rules.push(rule);
                "ルールを追加しました"
            }
            RuleFormMode::Edit => {
                let Some(slot) = form.index.and_then(|i| rules.get_mut(i)) else {
                    return;
                };
                *slot = rule;
                "ルールを変更しました"
            }
        };
        self.overlay = None;
        self.run_save_packet_filter(filter.id, filter.name, filter.description, rules, done);
    }

    /// ルールを1つ上か下へ動かす。上にあるものから順に評価される。
    fn move_rule(&mut self, down: bool) {
        if !self.require_write() || self.packet_filter.focus != ListFocus::Right {
            return;
        }
        let Some(filter) = self.selected_packet_filter() else {
            return;
        };
        let Some((index, _)) = self.selected_packet_filter_rule() else {
            return;
        };
        let target = if down {
            index + 1
        } else {
            index.wrapping_sub(1)
        };
        if target >= filter.rules.len() {
            return;
        }
        let mut rules = filter.rules.clone();
        rules.swap(index, target);
        self.packet_filter.rule_state.select(Some(target));
        self.run_save_packet_filter(
            filter.id,
            filter.name,
            filter.description,
            rules,
            "ルールを並べ替えました",
        );
    }

    /// フィルタ側ならフィルタごと、ルール側ならそのルールだけ消す。
    fn confirm_packet_filter_delete(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(filter) = self.selected_packet_filter() else {
            return;
        };
        match self.packet_filter.focus {
            ListFocus::Left => {
                self.overlay = Some(Overlay::Confirm {
                    title: "パケットフィルタの削除".to_string(),
                    body: format!(
                        "パケットフィルタ「{}」({}) をルールごと削除します。\n\
                         元に戻せません。実行するには名前を入力してください。",
                        filter.name, self.zone
                    ),
                    verify: Some(filter.name.clone()),
                    typed: String::new(),
                    action: ConfirmAction::DeletePacketFilter {
                        zone: self.zone.clone(),
                        id: filter.id,
                        name: filter.name,
                    },
                });
            }
            ListFocus::Right => {
                let Some((index, rule)) = self.selected_packet_filter_rule() else {
                    return;
                };
                self.overlay = Some(Overlay::Confirm {
                    title: "ルールの削除".to_string(),
                    body: format!(
                        "「{}」の {} 番目のルールを削除します。\n\
                         {} {} → {} ({})",
                        filter.name,
                        index + 1,
                        rule.protocol,
                        rule.source(),
                        rule.destination(),
                        rule.action,
                    ),
                    verify: None,
                    typed: String::new(),
                    action: ConfirmAction::DeletePacketFilterRule {
                        id: filter.id,
                        index,
                    },
                });
            }
        }
    }

    pub(super) fn run_delete_packet_filter(&mut self, zone: String, id: ResourceId, name: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.delete_packet_filter(&zone, id).await;
            let _ = tx.send(Message::PacketFilterChanged {
                what: format!("パケットフィルタ「{name}」を削除しました"),
                failed: "パケットフィルタの削除に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    pub(super) fn run_delete_packet_filter_rule(&mut self, id: ResourceId, index: usize) {
        let Some(filter) = self.selected_packet_filter() else {
            return;
        };
        if filter.id != id || index >= filter.rules.len() {
            return;
        }
        let mut rules = filter.rules.clone();
        rules.remove(index);
        self.run_save_packet_filter(
            filter.id,
            filter.name,
            filter.description,
            rules,
            "ルールを削除しました",
        );
    }
}
