//! APIキーの権限とアカウント情報（閲覧のみ）。
//!
//! `auth-status` は「今使っている資格情報で何ができるか」をまとめて返す。
//! 権限が足りずに 403 が出たときの原因を、ここを見れば切り分けられる。

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;

use crate::sacloud::{SacloudClient, flexible_number, null_as_default};

/// APIキーに与えられた操作権限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPermission {
    /// 作成・削除まで含めて全部できる。
    Create,
    /// 設定変更まで。作成・削除はできない。
    Arrange,
    /// 電源操作のみ。
    Power,
    /// 閲覧のみ。
    View,
    /// 将来増えた値。
    Unknown,
}

impl KeyPermission {
    pub fn parse(value: &str) -> Self {
        match value {
            "create" => KeyPermission::Create,
            "arrange" => KeyPermission::Arrange,
            "power" => KeyPermission::Power,
            "view" => KeyPermission::View,
            _ => KeyPermission::Unknown,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            KeyPermission::Create => "リソースの作成・削除まで可能",
            KeyPermission::Arrange => "設定変更まで可能（作成・削除は不可）",
            KeyPermission::Power => "電源操作のみ可能",
            KeyPermission::View => "閲覧のみ",
            KeyPermission::Unknown => "不明な権限",
        }
    }

    /// 書き込み操作ができる権限か。
    pub fn can_write(self) -> bool {
        matches!(
            self,
            KeyPermission::Create | KeyPermission::Arrange | KeyPermission::Power
        )
    }
}

/// APIキーに付いた「サービスへのアクセス権」の 1 つ。
///
/// コントロールパネルのチェックボックスに対応する。名前は環境や時期で
/// 増えるので、既知のものだけ和名を当て、知らないものは生の値のまま見せる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceAccess {
    /// API が返した生の値。
    pub token: String,
    /// 和名。対応が分からないときは `None`。
    pub label: Option<&'static str>,
}

impl ServiceAccess {
    fn new(token: &str) -> Self {
        ServiceAccess {
            token: token.to_string(),
            label: service_access_label(token),
        }
    }

    /// 画面に出す名前。和名が分からなければ生の値。
    pub fn display(&self) -> &str {
        self.label.unwrap_or(&self.token)
    }
}

/// 既知のアクセス権の和名。
///
/// コントロールパネルの「サービスへのアクセス権」の並びに合わせている。
/// ここに無い値は不明として生のまま出す。決め打ちで「無い」と判断すると、
/// 対応表が古いだけのときに「使えるのに使えない」と嘘をつくため。
fn service_access_label(token: &str) -> Option<&'static str> {
    Some(match token {
        "bill" => "請求閲覧",
        "dstorage" | "objectstorage" => "オブジェクトストレージ",
        "koukaryokudok" => "高火力 DOK",
        "cdn" | "webaccel" => "ウェブアクセラレータ",
        "phy" => "専用サーバ PHY",
        "apprun" => "AppRun",
        "apigateway" | "apigw" => "APIゲートウェイ",
        "workflow" => "ワークフロー",
        _ => return None,
    })
}

/// `auth-status` の中身。
#[derive(Debug, Clone)]
pub struct AuthStatus {
    pub auth_method: String,
    pub auth_class: String,
    pub is_api_key: bool,
    pub permission: KeyPermission,
    /// 生の権限文字列。未知の値でもそのまま見せられるように持っておく。
    pub permission_raw: String,
    /// サービスへのアクセス権。
    pub access: Vec<ServiceAccess>,
    /// API が返した生の文字列。対応表に無い値でも失われないように持っておく。
    pub access_raw: String,
    pub operation_penalty: String,
    /// アクセス元の制限。無ければ `None`。
    pub rest_filter: Option<String>,
    pub account: Account,
    pub member: Member,
}

impl AuthStatus {
    /// 操作制限が掛かっているか（未払いなどでペナルティが付くことがある）。
    pub fn is_penalized(&self) -> bool {
        !self.operation_penalty.is_empty() && self.operation_penalty != "none"
    }
}

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub code: String,
    pub name: String,
    pub class: String,
    pub payment_method: String,
    pub created_at: Option<String>,
    pub default_zone: String,
    /// `(表示名, 使用数, 上限)`。上限が分からないものは `None`。
    pub usage: Vec<(&'static str, i64, Option<i64>)>,
}

#[derive(Debug, Clone)]
pub struct Member {
    pub code: String,
    pub class: String,
    /// 会員情報に関する警告（支払い方法の不備など）。
    pub errors: Vec<(String, String)>,
}

// --- API のレスポンス形状 ---

/// `auth-status` は封筒に包まず、そのまま返ってくる。
#[derive(Debug, Deserialize)]
struct RawAuthStatus {
    #[serde(rename = "AuthMethod", default, deserialize_with = "null_as_default")]
    auth_method: String,
    #[serde(rename = "AuthClass", default, deserialize_with = "null_as_default")]
    auth_class: String,
    #[serde(rename = "IsAPIKey", default)]
    is_api_key: bool,
    #[serde(rename = "Permission", default, deserialize_with = "null_as_default")]
    permission: String,
    #[serde(
        rename = "ExternalPermission",
        default,
        deserialize_with = "null_as_default"
    )]
    external_permission: String,
    #[serde(
        rename = "OperationPenalty",
        default,
        deserialize_with = "null_as_default"
    )]
    operation_penalty: String,
    #[serde(rename = "RESTFilter")]
    rest_filter: Option<serde_json::Value>,
    #[serde(rename = "Account")]
    account: Option<RawAccount>,
    #[serde(rename = "Member")]
    member: Option<RawMember>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAccount {
    #[serde(rename = "ID", default)]
    id: serde_json::Value,
    #[serde(rename = "Code", default, deserialize_with = "null_as_default")]
    code: String,
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(rename = "Class", default, deserialize_with = "null_as_default")]
    class: String,
    #[serde(
        rename = "PaymentMethod",
        default,
        deserialize_with = "null_as_default"
    )]
    payment_method: String,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "DefaultZone")]
    default_zone: Option<RawZone>,
    #[serde(rename = "Limits", default)]
    limits: RawLimits,
    #[serde(rename = "UsedServers", default, deserialize_with = "flexible_number")]
    used_servers: i64,
    #[serde(rename = "UsedDisks", default, deserialize_with = "flexible_number")]
    used_disks: i64,
    #[serde(rename = "UsedSwitches", default, deserialize_with = "flexible_number")]
    used_switches: i64,
    #[serde(rename = "UsedBridges", default, deserialize_with = "flexible_number")]
    used_bridges: i64,
    #[serde(rename = "UsedArchives", default, deserialize_with = "flexible_number")]
    used_archives: i64,
    #[serde(rename = "UsedCDROMs", default, deserialize_with = "flexible_number")]
    used_cdroms: i64,
    #[serde(
        rename = "UsedAppliances",
        default,
        deserialize_with = "flexible_number"
    )]
    used_appliances: i64,
    #[serde(
        rename = "UsedCommonServiceItem",
        default,
        deserialize_with = "flexible_number"
    )]
    used_common_service_item: i64,
    #[serde(rename = "UsedGPU", default, deserialize_with = "flexible_number")]
    used_gpu: i64,
}

#[derive(Debug, Default, Deserialize)]
struct RawLimits {
    #[serde(rename = "MaxServers", default, deserialize_with = "flexible_number")]
    max_servers: i64,
    #[serde(rename = "MaxDiskCount", default, deserialize_with = "flexible_number")]
    max_disks: i64,
    #[serde(rename = "MaxBridges", default, deserialize_with = "flexible_number")]
    max_bridges: i64,
    #[serde(
        rename = "MaxArchiveCount",
        default,
        deserialize_with = "flexible_number"
    )]
    max_archives: i64,
    #[serde(
        rename = "MaxCDROMCount",
        default,
        deserialize_with = "flexible_number"
    )]
    max_cdroms: i64,
    #[serde(rename = "MaxGPUs", default, deserialize_with = "flexible_number")]
    max_gpus: i64,
}

#[derive(Debug, Deserialize)]
struct RawZone {
    #[serde(rename = "Name", default, deserialize_with = "null_as_default")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawMember {
    #[serde(rename = "Code", default, deserialize_with = "null_as_default")]
    code: String,
    #[serde(rename = "Class", default, deserialize_with = "null_as_default")]
    class: String,
    #[serde(rename = "Errors", default, deserialize_with = "null_as_default")]
    errors: std::collections::BTreeMap<String, String>,
}

/// ID は文字列でも数値でも返る。
fn id_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// 上限 0 は「上限なし」ではなく「取得できなかった」とみなして出さない。
fn limit(value: i64) -> Option<i64> {
    (value > 0).then_some(value)
}

impl From<RawAuthStatus> for AuthStatus {
    fn from(raw: RawAuthStatus) -> Self {
        let account = raw.account.unwrap_or_default();
        let limits = &account.limits;
        let usage = vec![
            ("サーバー", account.used_servers, limit(limits.max_servers)),
            ("ディスク", account.used_disks, limit(limits.max_disks)),
            ("スイッチ", account.used_switches, None),
            ("ブリッジ", account.used_bridges, limit(limits.max_bridges)),
            (
                "アーカイブ",
                account.used_archives,
                limit(limits.max_archives),
            ),
            ("ISOイメージ", account.used_cdroms, limit(limits.max_cdroms)),
            ("アプライアンス", account.used_appliances, None),
            ("GPU", account.used_gpu, limit(limits.max_gpus)),
            (
                "共通サービス",
                account.used_common_service_item,
                // DNS・シンプル監視・コンテナレジストリなどの合計。
                None,
            ),
        ];

        // "none" と空は「無し」。実際の区切りは "+"（bill+apprun+... ）だが、
        // 環境で違いうるので他の区切りでも切れるようにしておく。
        let access_raw = raw.external_permission;
        let access = if access_raw.is_empty() || access_raw == "none" {
            Vec::new()
        } else {
            access_raw
                .split(['+', ',', ' ', '/', '|'])
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "none")
                .map(ServiceAccess::new)
                .collect()
        };

        let member = raw.member.map(|m| Member {
            code: m.code,
            class: m.class,
            errors: m.errors.into_iter().collect(),
        });

        AuthStatus {
            auth_method: raw.auth_method,
            auth_class: raw.auth_class,
            is_api_key: raw.is_api_key,
            permission: KeyPermission::parse(&raw.permission),
            permission_raw: raw.permission,
            access,
            access_raw,
            operation_penalty: raw.operation_penalty,
            // null や空オブジェクトは「制限なし」。
            rest_filter: raw.rest_filter.and_then(|value| match value {
                serde_json::Value::Null => None,
                serde_json::Value::String(s) if s.is_empty() => None,
                other => Some(other.to_string()),
            }),
            account: Account {
                id: id_to_string(&account.id),
                code: account.code,
                name: account.name,
                class: account.class,
                payment_method: account.payment_method,
                created_at: account.created_at,
                default_zone: account.default_zone.map(|z| z.name).unwrap_or_default(),
                usage,
            },
            member: member.unwrap_or(Member {
                code: String::new(),
                class: String::new(),
                errors: Vec::new(),
            }),
        }
    }
}

impl SacloudClient {
    /// 今の資格情報で何ができるかを引く。
    pub async fn auth_status(&self) -> Result<AuthStatus> {
        let raw: RawAuthStatus = self
            .request_common(Method::GET, "auth-status", None)
            .await?;
        Ok(AuthStatus::from(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 実際のレスポンスから抜いたもの。
    const SAMPLE: &str = r#"{
        "AuthMethod": "apikey", "AuthClass": "account", "Permission": "create",
        "ExternalPermission": "none", "RESTFilter": null, "IsAPIKey": true,
        "OperationPenalty": "none",
        "Account": {
            "ID": "113701923763", "Class": "account", "Code": "aipf-dev",
            "Name": "AI Engine", "PaymentMethod": "banktransfer",
            "Limits": {"MaxServers": 100, "MaxGPUs": 2, "MaxBridges": 4,
                       "MaxDiskCount": 100, "MaxArchiveCount": 200, "MaxCDROMCount": 50},
            "UsedServers": 18, "UsedSwitches": 2, "UsedBridges": 0, "UsedDisks": 18,
            "UsedArchives": 0, "UsedCDROMs": 0, "UsedAppliances": 0,
            "UsedCommonServiceItem": 34, "UsedGPU": 0,
            "CreatedAt": "2025-07-23T09:58:17+09:00",
            "DefaultZone": {"ID": 21002, "Name": "tk1b"}
        },
        "User": null,
        "Member": {"Class": "sakura", "Code": "ixt15226",
                   "Errors": {"CreditCardError": "https://example.invalid/"}}
    }"#;

    fn parse(body: &str) -> AuthStatus {
        AuthStatus::from(serde_json::from_str::<RawAuthStatus>(body).unwrap())
    }

    #[test]
    fn parses_permission_and_account() {
        let status = parse(SAMPLE);
        assert_eq!(status.permission, KeyPermission::Create);
        assert!(status.permission.can_write());
        assert!(status.is_api_key);
        assert_eq!(status.account.code, "aipf-dev");
        assert_eq!(status.account.id, "113701923763");
        assert_eq!(status.account.default_zone, "tk1b");
        assert_eq!(status.member.code, "ixt15226");
    }

    /// "none" は権限なしとして扱うこと（"none" という権限があるわけではない）。
    #[test]
    fn none_means_no_access() {
        let status = parse(SAMPLE);
        assert!(status.access.is_empty());
    }

    #[test]
    fn parses_multiple_access_rights() {
        let body = SAMPLE.replace(
            r#""ExternalPermission": "none""#,
            r#""ExternalPermission": "bill,apprun""#,
        );
        let status = parse(&body);
        let names: Vec<&str> = status.access.iter().map(ServiceAccess::display).collect();
        assert_eq!(names, ["請求閲覧", "AppRun"]);
    }

    /// 実際に返ってくる形（"+" 区切り）を切れること。
    #[test]
    fn splits_on_plus() {
        let body = SAMPLE.replace(
            r#""ExternalPermission": "none""#,
            r#""ExternalPermission": "bill+koukaryokudok+apprun+dstorage""#,
        );
        let status = parse(&body);
        let names: Vec<&str> = status.access.iter().map(ServiceAccess::display).collect();
        assert_eq!(
            names,
            ["請求閲覧", "高火力 DOK", "AppRun", "オブジェクトストレージ"]
        );
    }

    /// 区切りが変わっても切れること。
    #[test]
    fn splits_on_other_separators() {
        let body = SAMPLE.replace(
            r#""ExternalPermission": "none""#,
            r#""ExternalPermission": "bill apprun/dstorage""#,
        );
        assert_eq!(parse(&body).access.len(), 3);
    }

    /// 知らない値でも捨てず、生のまま見せること。
    ///
    /// 対応表に無いからと落とすと、権限があるのに無いように見えてしまう。
    #[test]
    fn keeps_unknown_access_raw() {
        let body = SAMPLE.replace(
            r#""ExternalPermission": "none""#,
            r#""ExternalPermission": "bill,futureservice""#,
        );
        let status = parse(&body);
        assert_eq!(status.access.len(), 2);
        let unknown = &status.access[1];
        assert_eq!(unknown.label, None);
        assert_eq!(unknown.display(), "futureservice");
        // 生の文字列も丸ごと残す。
        assert_eq!(status.access_raw, "bill,futureservice");
    }

    #[test]
    fn usage_pairs_with_limits() {
        let status = parse(SAMPLE);
        let servers = status
            .account
            .usage
            .iter()
            .find(|u| u.0 == "サーバー")
            .unwrap();
        assert_eq!(servers.1, 18);
        assert_eq!(servers.2, Some(100));
        // 上限が返らないものは None のままにする。
        let switches = status
            .account
            .usage
            .iter()
            .find(|u| u.0 == "スイッチ")
            .unwrap();
        assert_eq!(switches.2, None);
    }

    /// 未払いなどの警告を拾えること。
    #[test]
    fn collects_member_errors() {
        let status = parse(SAMPLE);
        assert_eq!(status.member.errors.len(), 1);
        assert_eq!(status.member.errors[0].0, "CreditCardError");
    }

    #[test]
    fn penalty_none_is_not_penalized() {
        assert!(!parse(SAMPLE).is_penalized());
        let body = SAMPLE.replace(
            r#""OperationPenalty": "none""#,
            r#""OperationPenalty": "locked""#,
        );
        assert!(parse(&body).is_penalized());
    }

    /// 権限が未知の値でも落ちないこと。
    #[test]
    fn unknown_permission_is_kept_raw() {
        let body = SAMPLE.replace(r#""Permission": "create""#, r#""Permission": "future""#);
        let status = parse(&body);
        assert_eq!(status.permission, KeyPermission::Unknown);
        assert_eq!(status.permission_raw, "future");
        assert!(!status.permission.can_write());
    }

    /// 項目が丸ごと欠けても落ちないこと。
    #[test]
    fn survives_missing_fields() {
        let status = parse("{}");
        assert_eq!(status.permission, KeyPermission::Unknown);
        assert!(status.account.code.is_empty());
        assert!(status.member.errors.is_empty());
    }
}
