//! セキュリティコントロール画面の状態と操作。
//!
//! 有効化状態はプロジェクト単位の 1 件なので、評価ルールタブのヘッダに出す。

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, fmt_error, matches};
use crate::security_control::{AutomatedAction, EvaluationRule, SecurityControlActivation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SecurityControlTab {
    #[default]
    Rules,
    Actions,
}

impl SecurityControlTab {
    pub const ALL: [SecurityControlTab; 2] =
        [SecurityControlTab::Rules, SecurityControlTab::Actions];

    pub fn title(self) -> &'static str {
        match self {
            SecurityControlTab::Rules => "評価ルール",
            SecurityControlTab::Actions => "自動アクション",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Default)]
pub struct SecurityControlView {
    pub tab: SecurityControlTab,
    pub activation: Loadable<SecurityControlActivation>,
    pub rules: Loadable<Vec<EvaluationRule>>,
    pub rule_state: TableState,
    pub actions: Loadable<Vec<AutomatedAction>>,
    pub action_state: TableState,
}

impl App {
    pub fn visible_security_control_rules(&self) -> Loadable<Vec<EvaluationRule>> {
        let Loadable::Ready(items) = &self.security_control.rules else {
            return self.security_control.rules.clone();
        };
        let filter = self.filters.get(Pane::SecurityControlRules);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.description,
                            item.status_label(),
                            &item.scope_label(),
                            &item.iam_roles_required.join(","),
                            &item.service_principal_id,
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_security_control_rule(&self) -> Option<EvaluationRule> {
        let index = self.security_control.rule_state.selected()?;
        self.visible_security_control_rules()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_security_control_actions(&self) -> Loadable<Vec<AutomatedAction>> {
        let Loadable::Ready(items) = &self.security_control.actions else {
            return self.security_control.actions.clone();
        };
        let filter = self.filters.get(Pane::SecurityControlActions);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.description,
                            &item.action_type_label(),
                            &item.target_label(),
                            &item.execution_condition,
                            item.status_label(),
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_security_control_action(&self) -> Option<AutomatedAction> {
        let index = self.security_control.action_state.selected()?;
        self.visible_security_control_actions()
            .ready()?
            .get(index)
            .cloned()
    }

    pub(super) fn security_control_ensure_loaded(&mut self) {
        if self.security_control.activation.is_idle() {
            self.load_security_control_activation();
        }
        if self.security_control.rules.is_idle() {
            self.load_security_control_rules();
        } else {
            self.fill_selection(Pane::SecurityControlRules);
        }
        if self.security_control.actions.is_idle() {
            self.load_security_control_actions();
        } else {
            self.fill_selection(Pane::SecurityControlActions);
        }
    }

    fn load_security_control_activation(&mut self) {
        self.security_control.activation = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .security_control_activation()
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::SecurityControlActivation { result });
        });
    }

    fn load_security_control_rules(&mut self) {
        self.security_control.rules = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_evaluation_rules().await.map_err(fmt_error);
            let _ = tx.send(Message::SecurityControlRules { result });
        });
    }

    fn load_security_control_actions(&mut self) {
        self.security_control.actions = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_automated_actions().await.map_err(fmt_error);
            let _ = tx.send(Message::SecurityControlActions { result });
        });
    }

    pub(super) fn on_key_security_control(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.cycle_security_control_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_security_control_tab(1),
            KeyCode::Char('1') => self.security_control.tab = SecurityControlTab::Rules,
            KeyCode::Char('2') => self.security_control.tab = SecurityControlTab::Actions,
            _ => {}
        }
    }

    fn cycle_security_control_tab(&mut self, delta: i32) {
        self.security_control.tab = self.security_control.tab.cycled(delta);
    }

    pub(super) fn security_control_refresh(&mut self) {
        self.security_control.activation = Loadable::Idle;
        self.security_control.rules = Loadable::Idle;
        self.security_control.actions = Loadable::Idle;
        self.security_control.rule_state.select(None);
        self.security_control.action_state.select(None);
        self.security_control_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// タブの並びと巡回。2 つしかないので往復する。
    #[test]
    fn tabs_cycle_in_order_and_wrap() {
        let titles: Vec<&str> = SecurityControlTab::ALL
            .iter()
            .map(|tab| tab.title())
            .collect();
        assert_eq!(titles, vec!["評価ルール", "自動アクション"]);

        assert_eq!(
            SecurityControlTab::Rules.cycled(1),
            SecurityControlTab::Actions
        );
        assert_eq!(
            SecurityControlTab::Actions.cycled(1),
            SecurityControlTab::Rules
        );
        assert_eq!(
            SecurityControlTab::Rules.cycled(-1),
            SecurityControlTab::Actions
        );
    }

    /// 既定は評価ルール。
    #[test]
    fn default_tab_is_the_rule_list() {
        assert_eq!(SecurityControlTab::default(), SecurityControlTab::Rules);
    }
}
