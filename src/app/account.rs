//! 権限画面の状態（閲覧のみ）。
//!
//! `auth-status` の中身を「区分・項目・値・説明」の行に均して見せる。
//! 表にしておくと絞り込みもコピーも他の画面と同じ操作で済む。

use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, fmt_error, matches};
use crate::account::AuthStatus;

/// 権限画面の 1 行。
#[derive(Debug, Clone)]
pub struct AccountRow {
    pub section: &'static str,
    pub label: String,
    pub value: String,
    pub note: String,
    /// 注意を促したい行（権限不足・警告など）。
    pub warn: bool,
}

impl AccountRow {
    fn new(section: &'static str, label: &str, value: impl Into<String>) -> Self {
        AccountRow {
            section,
            label: label.to_string(),
            value: value.into(),
            note: String::new(),
            warn: false,
        }
    }

    fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    fn warn(mut self, warn: bool) -> Self {
        self.warn = warn;
        self
    }
}

#[derive(Debug, Default)]
pub struct AccountView {
    pub status: Loadable<AuthStatus>,
    pub state: TableState,
}

/// 空欄は「(なし)」と出す。空文字のまま出すと項目が消えたように見える。
fn or_none(value: &str) -> String {
    if value.is_empty() {
        "(なし)".to_string()
    } else {
        value.to_string()
    }
}

/// `auth-status` の中身を表示用の行に均す。
pub fn rows(status: &AuthStatus) -> Vec<AccountRow> {
    let mut out = vec![
        AccountRow::new("権限", "操作権限", or_none(&status.permission_raw))
            .note(status.permission.description())
            .warn(!status.permission.can_write()),
        AccountRow::new("権限", "認証方法", or_none(&status.auth_method)).note(
            if status.is_api_key {
                "APIキーで認証しています"
            } else {
                "APIキー以外で認証しています"
            },
        ),
        AccountRow::new("権限", "認証区分", or_none(&status.auth_class)),
        AccountRow::new("権限", "操作制限", or_none(&status.operation_penalty))
            .note(if status.is_penalized() {
                "操作に制限が掛かっています"
            } else {
                ""
            })
            .warn(status.is_penalized()),
        AccountRow::new(
            "権限",
            "アクセス制限",
            status.rest_filter.clone().unwrap_or("なし".to_string()),
        ),
    ];

    if status.access.is_empty() {
        out.push(
            AccountRow::new("アクセス権", "(なし)", or_none(&status.access_raw))
                .note("このキーには追加のサービスアクセス権がありません"),
        );
    } else {
        for access in &status.access {
            out.push(
                AccountRow::new("アクセス権", access.display(), "あり")
                    // 生の値も添える。和名の対応表が古くても中身が分かるように。
                    .note(match access.label {
                        Some(_) => access.token.clone(),
                        None => format!("{}（和名が未対応の値です）", access.token),
                    }),
            );
        }
    }

    let account = &status.account;
    out.extend([
        AccountRow::new("アカウント", "名前", or_none(&account.name)),
        AccountRow::new("アカウント", "コード", or_none(&account.code)),
        AccountRow::new("アカウント", "ID", or_none(&account.id)),
        AccountRow::new("アカウント", "区分", or_none(&account.class)),
        AccountRow::new("アカウント", "支払方法", or_none(&account.payment_method)),
        AccountRow::new("アカウント", "既定ゾーン", or_none(&account.default_zone)),
        AccountRow::new(
            "アカウント",
            "作成日",
            account.created_at.clone().unwrap_or_default(),
        ),
    ]);

    out.push(AccountRow::new(
        "会員",
        "コード",
        or_none(&status.member.code),
    ));
    out.push(AccountRow::new(
        "会員",
        "区分",
        or_none(&status.member.class),
    ));
    for (name, detail) in &status.member.errors {
        out.push(
            AccountRow::new("会員", name, "警告")
                .note(detail.clone())
                .warn(true),
        );
    }

    for (name, used, max) in &account.usage {
        let value = match max {
            Some(max) => format!("{used} / {max}"),
            None => used.to_string(),
        };
        // 上限に近いものは目立たせる。
        let warn = max.is_some_and(|max| max > 0 && *used * 10 >= max * 9);
        out.push(AccountRow::new("使用量", name, value).warn(warn));
    }

    out
}

impl App {
    pub fn visible_account_rows(&self) -> Vec<AccountRow> {
        let Some(status) = self.account.status.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Account);
        rows(status)
            .into_iter()
            .filter(|row| matches(filter, &[row.section, &row.label, &row.value, &row.note]))
            .collect()
    }

    // --- 読み込み ---

    pub(super) fn account_ensure_loaded(&mut self) {
        if self.account.status.is_idle() {
            self.load_auth_status();
            return;
        }
        self.fill_selection(Pane::Account);
    }

    pub(super) fn account_refresh(&mut self) {
        self.load_auth_status();
    }

    pub(super) fn load_auth_status(&mut self) {
        self.account.status = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.auth_status().await.map_err(fmt_error);
            let _ = tx.send(Message::AuthStatus(Box::new(result)));
        });
    }

    pub(super) fn account_invalidate(&mut self) {
        self.account = AccountView::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AuthStatus {
        AuthStatus {
            auth_method: "apikey".into(),
            auth_class: "account".into(),
            is_api_key: true,
            permission: crate::account::KeyPermission::Create,
            permission_raw: "create".into(),
            access: Vec::new(),
            access_raw: "none".into(),
            operation_penalty: "none".into(),
            rest_filter: None,
            account: crate::account::Account {
                id: "1".into(),
                code: "code".into(),
                name: "name".into(),
                class: "account".into(),
                payment_method: "banktransfer".into(),
                created_at: None,
                default_zone: "tk1b".into(),
                usage: vec![("サーバー", 95, Some(100)), ("ディスク", 1, Some(100))],
            },
            member: crate::account::Member {
                code: "m".into(),
                class: "sakura".into(),
                errors: Vec::new(),
            },
        }
    }

    /// アクセス権が無いときも、その旨が行として出ること。
    #[test]
    fn shows_absence_of_access_rights() {
        let rows = rows(&sample());
        let row = rows.iter().find(|r| r.section == "アクセス権").unwrap();
        assert_eq!(row.label, "(なし)");
    }

    /// アクセス権は 1 件ずつ行になり、生の値も添えること。
    #[test]
    fn lists_access_rights_one_per_row() {
        let mut status = sample();
        status.access = vec![
            crate::account::ServiceAccess {
                token: "bill".into(),
                label: Some("請求閲覧"),
            },
            crate::account::ServiceAccess {
                token: "mystery".into(),
                label: None,
            },
        ];
        let rows = rows(&status);
        let access: Vec<&AccountRow> = rows.iter().filter(|r| r.section == "アクセス権").collect();
        assert_eq!(access.len(), 2);
        assert_eq!(access[0].label, "請求閲覧");
        assert_eq!(access[0].note, "bill");
        // 和名が分からない値も落とさない。
        assert_eq!(access[1].label, "mystery");
    }

    /// 上限に近い使用量に印が付くこと。
    #[test]
    fn warns_when_close_to_limit() {
        let rows = rows(&sample());
        let servers = rows
            .iter()
            .find(|r| r.section == "使用量" && r.label == "サーバー")
            .unwrap();
        assert_eq!(servers.value, "95 / 100");
        assert!(servers.warn);
        let disks = rows
            .iter()
            .find(|r| r.section == "使用量" && r.label == "ディスク")
            .unwrap();
        assert!(!disks.warn);
    }

    /// 値が空でも空欄にならないこと。
    #[test]
    fn empty_values_show_placeholder() {
        let mut status = sample();
        status.account.name = String::new();
        let rows = rows(&status);
        let name = rows
            .iter()
            .find(|r| r.section == "アカウント" && r.label == "名前")
            .unwrap();
        assert_eq!(name.value, "(なし)");
    }
}
