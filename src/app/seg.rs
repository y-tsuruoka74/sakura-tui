//! サービスエンドポイントゲートウェイ画面の状態と操作。
//!
//! 接続先サービスのタブは追加のAPIを呼ばず、ゲートウェイ一覧に含まれる
//! `EnabledServices` から導出する。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, fmt_error, matches};
use crate::seg::{Seg, SegService};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegTab {
    #[default]
    Gateways,
    Services,
}

impl SegTab {
    pub const ALL: [SegTab; 2] = [SegTab::Gateways, SegTab::Services];

    pub fn title(self) -> &'static str {
        match self {
            SegTab::Gateways => "ゲートウェイ",
            SegTab::Services => "接続先サービス",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Default)]
pub struct SegView {
    pub tab: SegTab,
    /// ゾーンごとの一覧。キーはゾーン名。
    pub gateways: HashMap<String, Loadable<Vec<Seg>>>,
    pub gateway_state: TableState,
    pub service_state: TableState,
}

impl App {
    /// 現在ゾーンの一覧。SEG はゾーン依存なので `z` の切り替えに追従する。
    fn seg_gateways(&self) -> Loadable<Vec<Seg>> {
        self.seg
            .gateways
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_seg_gateways(&self) -> Loadable<Vec<Seg>> {
        let loadable = self.seg_gateways();
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::SegGateways);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.description,
                            &item.tags.join(","),
                            &item.status_label(),
                            &item.switch_name,
                            &item.switch_id,
                            &item.ip_label(),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_seg_gateway(&self) -> Option<Seg> {
        let index = self.seg.gateway_state.selected()?;
        self.visible_seg_gateways().ready()?.get(index).cloned()
    }

    /// 選択中ゲートウェイの接続先サービス。
    ///
    /// 一覧に含まれる情報なので、ここでの追加取得は無い。
    pub fn visible_seg_services(&self) -> Loadable<Vec<SegService>> {
        let Some(gateway) = self.selected_seg_gateway() else {
            return Loadable::Idle;
        };
        let filter = self.filters.get(Pane::SegServices);
        Loadable::Ready(
            gateway
                .services
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.kind,
                            &item.kind_label(),
                            &item.endpoint,
                            item.mode_label(),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_seg_service(&self) -> Option<SegService> {
        let index = self.seg.service_state.selected()?;
        self.visible_seg_services().ready()?.get(index).cloned()
    }

    pub(super) fn seg_ensure_loaded(&mut self) {
        if self.seg_gateways().is_idle() {
            self.load_seg_gateways();
        } else {
            self.fill_selection(Pane::SegGateways);
            self.fill_selection(Pane::SegServices);
        }
    }

    fn load_seg_gateways(&mut self) {
        let zone = self.zone.clone();
        self.seg.gateways.insert(zone.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_segs(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::SegGateways { zone, result });
        });
    }

    pub(super) fn on_key_seg(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.cycle_seg_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_seg_tab(1),
            KeyCode::Char('1') => self.seg.tab = SegTab::Gateways,
            KeyCode::Char('2') => self.seg.tab = SegTab::Services,
            _ => {}
        }
    }

    fn cycle_seg_tab(&mut self, delta: i32) {
        self.seg.tab = self.seg.tab.cycled(delta);
    }

    /// 選択中のゲートウェイが変わったら、接続先サービスの選択位置を捨てる。
    pub(super) fn seg_reset_child_selection(&mut self) {
        self.seg.service_state.select(None);
    }

    pub(super) fn seg_refresh(&mut self) {
        // 現在ゾーンだけ捨てる。他ゾーンのキャッシュは有効なまま残す。
        self.seg.gateways.remove(&self.zone);
        self.seg.gateway_state.select(None);
        self.seg_reset_child_selection();
        self.seg_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// タブの並びと巡回。2 つしかないので往復する。
    #[test]
    fn tabs_cycle_in_order_and_wrap() {
        let titles: Vec<&str> = SegTab::ALL.iter().map(|tab| tab.title()).collect();
        assert_eq!(titles, vec!["ゲートウェイ", "接続先サービス"]);

        assert_eq!(SegTab::Gateways.cycled(1), SegTab::Services);
        assert_eq!(SegTab::Services.cycled(1), SegTab::Gateways);
        assert_eq!(SegTab::Gateways.cycled(-1), SegTab::Services);
    }

    /// 既定はゲートウェイ一覧。
    #[test]
    fn default_tab_is_the_gateway_list() {
        assert_eq!(SegTab::default(), SegTab::Gateways);
    }
}
