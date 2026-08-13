//! 請求画面の状態（閲覧のみ）。
//!
//! 請求一覧を選ぶと、その明細と集計を右側のタブで切り替えて見る。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, StatusKind, fmt_error, matches};
use crate::billing::{Bill, BillDetail, BillingIdentity, summarize};

/// 請求画面で操作対象になっている側。
///
/// 左の月一覧と右の明細はどちらもスクロールするので、
/// どちらに ↑↓ が効くのかを持っておく必要がある。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BillingFocus {
    /// 左の月一覧。まず月を選ぶので既定はこちら。
    #[default]
    Bills,
    /// 右の明細・集計。
    Detail,
}

/// 請求画面のタブ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BillingTab {
    #[default]
    Details,
    ByCategory,
    ByZone,
}

impl BillingTab {
    pub const ALL: [BillingTab; 3] = [
        BillingTab::Details,
        BillingTab::ByCategory,
        BillingTab::ByZone,
    ];

    pub fn title(self) -> &'static str {
        match self {
            BillingTab::Details => "明細",
            BillingTab::ByCategory => "種別ごと",
            BillingTab::ByZone => "ゾーンごと",
        }
    }
}

#[derive(Debug)]
pub struct BillingView {
    /// アカウントIDと会員コード。請求を引くのに要る。
    pub identity: Loadable<BillingIdentity>,
    /// 表示中の年。API は年を指定しないと直近しか返さない。
    pub year: i32,
    pub bills: Loadable<Vec<Bill>>,
    pub bill_state: TableState,
    pub focus: BillingFocus,
    pub tab: BillingTab,
    /// 請求IDをキーにした明細。
    pub details: HashMap<String, Loadable<Vec<BillDetail>>>,
    pub detail_state: TableState,
    pub summary_state: TableState,
}

impl Default for BillingView {
    fn default() -> Self {
        Self {
            identity: Loadable::default(),
            year: current_year(),
            bills: Loadable::default(),
            bill_state: TableState::default(),
            focus: BillingFocus::default(),
            tab: BillingTab::default(),
            details: HashMap::new(),
            detail_state: TableState::default(),
            summary_state: TableState::default(),
        }
    }
}

/// ↑↓ の対象になるペイン。
///
/// 月一覧を見ているあいだは ↑↓ で月が変わってほしいので、
/// タブより先にフォーカスを見る。
fn pane_for(focus: BillingFocus, tab: BillingTab) -> Pane {
    match focus {
        BillingFocus::Bills => Pane::Bills,
        BillingFocus::Detail => match tab {
            BillingTab::Details => Pane::BillDetails,
            _ => Pane::BillSummary,
        },
    }
}

/// 今の年。請求の初期表示に使う。
pub(super) fn current_year() -> i32 {
    use chrono::Datelike;
    chrono::Local::now().year()
}

impl App {
    pub fn visible_bills(&self) -> Vec<&Bill> {
        let Some(items) = self.billing.bills.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Bills);
        items
            .iter()
            .filter(|b| {
                matches(
                    filter,
                    &[b.date.as_deref().unwrap_or(""), &b.amount.to_string()],
                )
            })
            .collect()
    }

    pub fn selected_bill(&self) -> Option<&Bill> {
        let index = self.billing.bill_state.selected()?;
        self.visible_bills().into_iter().nth(index)
    }

    pub fn current_bill_details(&self) -> Loadable<Vec<BillDetail>> {
        self.selected_bill()
            .and_then(|bill| self.billing.details.get(&bill.id))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_bill_details(&self) -> Loadable<Vec<BillDetail>> {
        let loadable = self.current_bill_details();
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::BillDetails);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|d| {
                    let label = d.service_label();
                    matches(filter, &[&d.description, &d.zone, &label])
                })
                .collect(),
        )
    }

    /// 現在のタブに応じた集計。`(名前, 金額, 件数)` を金額の大きい順に返す。
    pub fn current_summary(&self) -> Vec<(String, i64, usize)> {
        let Some(details) = self.current_bill_details().ready().cloned() else {
            return Vec::new();
        };
        match self.billing.tab {
            BillingTab::ByCategory => summarize(&details, BillDetail::category),
            BillingTab::ByZone => summarize(&details, |d| {
                if d.zone.is_empty() {
                    "(ゾーンなし)".to_string()
                } else {
                    d.zone.clone()
                }
            }),
            BillingTab::Details => Vec::new(),
        }
    }

    pub(super) fn billing_active_pane(&self) -> Pane {
        pane_for(self.billing.focus, self.billing.tab)
    }

    // --- 読み込み ---

    pub(super) fn billing_ensure_loaded(&mut self) {
        // まずアカウントIDと会員コードを引く。
        if self.billing.identity.is_idle() {
            self.billing.identity = Loadable::Loading;
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.billing_identity().await.map_err(fmt_error);
                let _ = tx.send(Message::BillingIdentity(Box::new(result)));
            });
            return;
        }
        let Some(identity) = self.billing.identity.ready().cloned() else {
            return;
        };

        if self.billing.bills.is_idle() {
            self.billing.bills = Loadable::Loading;
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            let year = self.billing.year;
            tokio::spawn(async move {
                let result = client
                    .list_bills(&identity.account_id, year)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::Bills(result));
            });
            return;
        }

        self.fill_selection(Pane::Bills);
        let Some(bill) = self.selected_bill().cloned() else {
            return;
        };
        if self
            .billing
            .details
            .get(&bill.id)
            .is_none_or(Loadable::is_idle)
        {
            self.billing
                .details
                .insert(bill.id.clone(), Loadable::Loading);
            self.inflight += 1;
            let client = self.sacloud.clone();
            let tx = self.tx.clone();
            let member_code = identity.member_code.clone();
            tokio::spawn(async move {
                let result = client
                    .bill_details(&member_code, &bill.id)
                    .await
                    .map_err(fmt_error);
                let _ = tx.send(Message::BillDetails {
                    id: bill.id,
                    result,
                });
            });
        } else {
            self.fill_selection(self.billing_active_pane());
        }
    }

    pub(super) fn billing_refresh(&mut self) {
        // 請求は締め後に確定するので、明細ごと取り直す。
        self.billing.details.clear();
        self.billing.bills = Loadable::Idle;
        self.billing_ensure_loaded();
    }

    /// 表示中の年。
    pub fn billing_year(&self) -> i32 {
        self.billing.year
    }

    // --- キー入力 ---

    /// 表示する年を変える。
    fn change_year(&mut self, delta: i32) {
        let year = self.billing.year + delta;
        // 未来の請求は無いので、今年より先には進まない。
        if year > current_year() {
            self.set_status("これ以降の請求はありません", StatusKind::Info);
            return;
        }
        self.billing.year = year;
        self.billing.bills = Loadable::Idle;
        self.billing.bill_state.select(None);
        self.billing.details.clear();
        self.billing.focus = BillingFocus::Bills;
        self.billing_ensure_loaded();
    }

    pub(super) fn on_key_billing(&mut self, key: KeyEvent) {
        match key.code {
            // 年の移動。左右は常に年に割り当てる（月は ↑↓ で選ぶ）。
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('[') => self.change_year(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(']') => self.change_year(1),
            // 明細へ入る / 月一覧へ戻る。
            KeyCode::Enter => self.billing.focus = BillingFocus::Detail,
            KeyCode::Esc => self.billing.focus = BillingFocus::Bills,
            KeyCode::Tab => self.cycle_billing_tab(1),
            KeyCode::BackTab => self.cycle_billing_tab(-1),
            KeyCode::Char('1') => self.set_billing_tab(BillingTab::Details),
            KeyCode::Char('2') => self.set_billing_tab(BillingTab::ByCategory),
            KeyCode::Char('3') => self.set_billing_tab(BillingTab::ByZone),
            _ => {}
        }
    }

    fn set_billing_tab(&mut self, tab: BillingTab) {
        self.billing.tab = tab;
        self.billing.focus = BillingFocus::Detail;
    }

    fn cycle_billing_tab(&mut self, delta: i32) {
        let current = BillingTab::ALL
            .iter()
            .position(|t| *t == self.billing.tab)
            .unwrap_or(0) as i32;
        let len = BillingTab::ALL.len() as i32;
        self.billing.tab = BillingTab::ALL[(current + delta).rem_euclid(len) as usize];
        self.billing.focus = BillingFocus::Detail;
    }

    pub(super) fn billing_invalidate(&mut self) {
        self.billing = BillingView::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 既定では月一覧が操作対象であること（ここが明細だと月を選べない）。
    #[test]
    fn month_list_is_focused_by_default() {
        let view = BillingView::default();
        assert_eq!(view.focus, BillingFocus::Bills);
        assert_eq!(pane_for(view.focus, view.tab), Pane::Bills);
    }

    /// 明細に入ると ↑↓ の対象がタブごとの表に移ること。
    #[test]
    fn detail_focus_follows_tab() {
        assert_eq!(
            pane_for(BillingFocus::Detail, BillingTab::Details),
            Pane::BillDetails
        );
        assert_eq!(
            pane_for(BillingFocus::Detail, BillingTab::ByCategory),
            Pane::BillSummary
        );
        assert_eq!(
            pane_for(BillingFocus::Detail, BillingTab::ByZone),
            Pane::BillSummary
        );
    }

    /// タブを見ているあいだに月一覧へ戻っても、↑↓ は月に効くこと。
    #[test]
    fn returning_to_month_list_ignores_tab() {
        assert_eq!(
            pane_for(BillingFocus::Bills, BillingTab::ByZone),
            Pane::Bills
        );
    }
}
