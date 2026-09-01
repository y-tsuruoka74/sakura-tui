//! さくらのクラウド セキュリティコントロールの読み取り専用クライアント。
//!
//! クラウド環境のセキュリティ上の問題を評価ルールで検査し、条件に合ったときに
//! 自動アクション（シンプル通知・ワークフロー）を実行する機能。
//!
//! エンドポイントは `{root}/{zone}/api/securitycontrol/1.0/`。IaaS API とは
//! 別系統で、検索条件も普通のクエリ文字列。ページングはカーソル方式。
//!
//! 評価「結果」を引くAPIは公開されていない。この画面で見られるのは、どのルールが
//! 有効かという設定と、自動アクションの定義まで。検出結果はコントロールパネルの
//! イベントログ側にある。

use anyhow::Result;
use serde::Deserialize;

use crate::sacloud::{SacloudClient, flexible_number, null_as_default};

/// 1 ページあたりの取得件数。仕様上の最大値。
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。カーソルが進まなくなっても止まるようにする。
const MAX_PAGES: usize = 100;

const SUFFIX: &str = "api/securitycontrol/1.0";

/// 本番でのエンドポイント受付ゾーン。仕様の `servers` はこれだけを挙げている。
const PRODUCTION_ZONE: &str = "is1a";

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// プロジェクトでの有効化状態。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecurityControlActivation {
    pub is_active: bool,
    /// 評価対象リソースへのアクセスに使うサービスプリンシパル。
    pub service_principal_id: String,
    /// 登録できる自動アクションの上限。
    pub automated_action_limit: u32,
}

impl SecurityControlActivation {
    pub fn status_label(&self) -> &'static str {
        if self.is_active { "有効" } else { "無効" }
    }
}

/// 評価ルール 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationRule {
    /// `server-no-public-ip` のような識別子。
    pub id: String,
    pub description: String,
    pub is_enabled: bool,
    /// 適用に必要なIAMロール。
    pub iam_roles_required: Vec<String>,
    /// 評価対象のサービスプリンシパル。未指定なら有効化時のものが使われる。
    pub service_principal_id: String,
    /// 評価対象ゾーン。空は「全ゾーン」の意味。
    pub zones: Vec<String>,
    /// 評価対象のオブジェクトストレージサイト。空は「全サイト」の意味。
    pub sites: Vec<String>,
}

impl EvaluationRule {
    pub fn status_label(&self) -> &'static str {
        if self.is_enabled { "有効" } else { "無効" }
    }

    /// 評価対象の絞り込み。仕様上、空配列は「すべて対象」を意味する。
    pub fn scope_label(&self) -> String {
        if !self.zones.is_empty() {
            return self.zones.join(", ");
        }
        if !self.sites.is_empty() {
            return self.sites.join(", ");
        }
        String::new()
    }
}

/// 自動アクション 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomatedAction {
    pub id: String,
    pub name: String,
    pub description: String,
    /// `simpleNotification` / `workflows`。
    pub action_type: String,
    /// 実行条件（CEL式）。
    pub execution_condition: String,
    pub is_enabled: bool,
    pub created_at: String,
    /// アクションの実行に使うサービスプリンシパル。
    pub service_principal_id: String,
    /// `simpleNotification` のときの通知先グループ。
    pub notification_group_id: String,
    /// `workflows` のときのワークフロー。
    pub workflow_id: String,
    pub workflow_revision: String,
}

impl AutomatedAction {
    pub fn status_label(&self) -> &'static str {
        if self.is_enabled { "有効" } else { "無効" }
    }

    pub fn action_type_label(&self) -> String {
        match self.action_type.as_str() {
            "simpleNotification" => "シンプル通知".to_string(),
            "workflows" => "ワークフロー".to_string(),
            other => other.to_string(),
        }
    }

    /// アクションの宛先。種別によって見る項目が違う。
    pub fn target_label(&self) -> String {
        match self.action_type.as_str() {
            "simpleNotification" => self.notification_group_id.clone(),
            "workflows" => {
                if self.workflow_revision.is_empty() {
                    self.workflow_id.clone()
                } else {
                    format!("{} ({})", self.workflow_id, self.workflow_revision)
                }
            }
            _ => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// デシリアライズ用の内部型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawActivation {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "servicePrincipalId")]
    service_principal_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "isActive")]
    is_active: bool,
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "automatedActionLimit")]
    automated_action_limit: u32,
}

#[derive(Debug, Deserialize)]
struct RulePage {
    #[serde(default, deserialize_with = "null_as_default")]
    items: Vec<Option<RawEvaluationRule>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEvaluationRule {
    #[serde(rename = "rule")]
    rule: Option<RawRule>,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "iamRolesRequired")]
    iam_roles_required: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "isEnabled")]
    is_enabled: bool,
}

/// 評価ルールの中身。
///
/// 仕様では `evaluationRuleId` で判別する 14 個の oneOf だが、違いは
/// `parameter` に載る項目だけなので、まとめて受けて空のものを捨てる。
#[derive(Debug, Deserialize)]
struct RawRule {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "evaluationRuleId")]
    evaluation_rule_id: String,
    #[serde(rename = "parameter")]
    parameter: Option<RawRuleParameter>,
}

#[derive(Debug, Deserialize)]
struct RawRuleParameter {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "servicePrincipalId")]
    service_principal_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    zones: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    sites: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ActionPage {
    #[serde(default, deserialize_with = "null_as_default")]
    items: Vec<Option<RawAutomatedAction>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAutomatedAction {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "automatedActionId")]
    automated_action_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(rename = "action")]
    action: Option<RawAction>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "executionCondition")]
    execution_condition: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "isEnabled")]
    is_enabled: bool,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "createdAt")]
    created_at: String,
}

/// アクション定義。`actionType` で判別する oneOf だが、`actionParameter` の
/// 項目は種別ごとに重ならないため、まとめて受ける。
#[derive(Debug, Deserialize)]
struct RawAction {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "actionType")]
    action_type: String,
    #[serde(rename = "actionParameter")]
    action_parameter: Option<RawActionParameter>,
}

#[derive(Debug, Deserialize)]
struct RawActionParameter {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "servicePrincipalId")]
    service_principal_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "notificationGroupId")]
    notification_group_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "workflowId")]
    workflow_id: String,
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "revisionId")]
    revision_id: u64,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "revisionAlias")]
    revision_alias: String,
}

// ---------------------------------------------------------------------------
// パース
// ---------------------------------------------------------------------------

impl From<RawEvaluationRule> for EvaluationRule {
    fn from(raw: RawEvaluationRule) -> Self {
        let rule = raw.rule;
        let parameter = rule.as_ref().and_then(|r| r.parameter.as_ref());
        EvaluationRule {
            id: rule
                .as_ref()
                .map(|r| r.evaluation_rule_id.clone())
                .unwrap_or_default(),
            description: raw.description,
            is_enabled: raw.is_enabled,
            iam_roles_required: raw.iam_roles_required,
            service_principal_id: parameter
                .map(|p| p.service_principal_id.clone())
                .unwrap_or_default(),
            zones: parameter.map(|p| p.zones.clone()).unwrap_or_default(),
            sites: parameter.map(|p| p.sites.clone()).unwrap_or_default(),
        }
    }
}

impl From<RawAutomatedAction> for AutomatedAction {
    fn from(raw: RawAutomatedAction) -> Self {
        let action = raw.action;
        let parameter = action.as_ref().and_then(|a| a.action_parameter.as_ref());
        // リビジョンはIDとエイリアスのどちらかで指定される。両方無ければ最新。
        let workflow_revision = parameter
            .map(|p| {
                if !p.revision_alias.is_empty() {
                    p.revision_alias.clone()
                } else if p.revision_id != 0 {
                    p.revision_id.to_string()
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        AutomatedAction {
            id: raw.automated_action_id,
            name: raw.name,
            description: raw.description,
            action_type: action
                .as_ref()
                .map(|a| a.action_type.clone())
                .unwrap_or_default(),
            execution_condition: raw.execution_condition,
            is_enabled: raw.is_enabled,
            created_at: raw.created_at,
            service_principal_id: parameter
                .map(|p| p.service_principal_id.clone())
                .unwrap_or_default(),
            notification_group_id: parameter
                .map(|p| p.notification_group_id.clone())
                .unwrap_or_default(),
            workflow_id: parameter.map(|p| p.workflow_id.clone()).unwrap_or_default(),
            workflow_revision,
        }
    }
}

fn parse_activation(body: &str) -> Result<SecurityControlActivation> {
    let raw: RawActivation = parse_json(body)?;
    Ok(SecurityControlActivation {
        is_active: raw.is_active,
        service_principal_id: raw.service_principal_id,
        automated_action_limit: raw.automated_action_limit,
    })
}

fn parse_rule_page(body: &str) -> Result<(Vec<EvaluationRule>, Option<String>)> {
    let page: RulePage = parse_json(body)?;
    Ok((
        page.items
            .into_iter()
            .flatten()
            .map(EvaluationRule::from)
            .collect(),
        page.next_page_token.filter(|token| !token.is_empty()),
    ))
}

fn parse_action_page(body: &str) -> Result<(Vec<AutomatedAction>, Option<String>)> {
    let page: ActionPage = parse_json(body)?;
    Ok((
        page.items
            .into_iter()
            .flatten()
            .map(AutomatedAction::from)
            .collect(),
        page.next_page_token.filter(|token| !token.is_empty()),
    ))
}

fn parse_json<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    use anyhow::Context;
    let body = if body.trim().is_empty() { "{}" } else { body };
    serde_json::from_str(body).with_context(|| {
        let head: String = body.chars().take(200).collect();
        format!("セキュリティコントロールAPIレスポンスの解析に失敗しました: {head}")
    })
}

/// 1 ページ分の本文を「要素と次カーソル」に解く関数。
type PageParser<T> = fn(&str) -> Result<(Vec<T>, Option<String>)>;

fn page_query(cursor: Option<String>) -> Vec<(&'static str, String)> {
    let mut query = vec![("page_size", PAGE_SIZE.to_string())];
    if let Some(cursor) = cursor {
        query.push(("next", cursor));
    }
    query
}

// ---------------------------------------------------------------------------
// API 呼び出し
// ---------------------------------------------------------------------------

/// 接続先に応じたセキュリティコントロールの受付ゾーン。
///
/// 仕様は `is1a` だけを挙げているが、社内テスト環境（cloud-test）に `is1a` は
/// 存在しない。決め打ちにすると必ず失敗するので、テスト環境では既定ゾーンへ回す。
fn security_control_zone<'a>(api_root: &str, default_zone: &'a str) -> &'a str {
    if api_root == crate::config::TEST_API_ROOT {
        default_zone
    } else {
        PRODUCTION_ZONE
    }
}

impl SacloudClient {
    /// セキュリティコントロールの問い合わせ先ゾーン。
    pub fn security_control_zone(&self) -> &str {
        security_control_zone(self.api_root(), self.default_zone())
    }

    pub async fn security_control_activation(&self) -> Result<SecurityControlActivation> {
        let zone = self.security_control_zone().to_string();
        let value: serde_json::Value = self
            .request_zoned_service(&zone, SUFFIX, "activation", &[])
            .await?;
        parse_activation(&value.to_string())
    }

    pub async fn list_evaluation_rules(&self) -> Result<Vec<EvaluationRule>> {
        let zone = self.security_control_zone().to_string();
        self.collect_pages(&zone, "evaluation_rules", parse_rule_page)
            .await
    }

    pub async fn list_automated_actions(&self) -> Result<Vec<AutomatedAction>> {
        let zone = self.security_control_zone().to_string();
        self.collect_pages(&zone, "automated_actions", parse_action_page)
            .await
    }

    /// カーソルを辿って全ページ集める。
    async fn collect_pages<T>(
        &self,
        zone: &str,
        path: &str,
        parse: PageParser<T>,
    ) -> Result<Vec<T>> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let value: serde_json::Value = self
                .request_zoned_service(zone, SUFFIX, path, &page_query(cursor.clone()))
                .await?;
            let (items, next) = parse(&value.to_string())?;
            let received = items.len();
            out.extend(items);
            match next {
                // 同じカーソルが返ってきたら進んでいないので打ち切る。
                Some(next) if received > 0 && Some(&next) != cursor.as_ref() => cursor = Some(next),
                _ => break,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_activation() {
        let body = r#"{
            "servicePrincipalId": "sp-1",
            "isActive": true,
            "automatedActionLimit": 10
        }"#;
        let activation = parse_activation(body).unwrap();
        assert!(activation.is_active);
        assert_eq!(activation.service_principal_id, "sp-1");
        assert_eq!(activation.automated_action_limit, 10);
        assert_eq!(activation.status_label(), "有効");

        // 未有効化でも落ちない。
        let empty = parse_activation("{}").unwrap();
        assert!(!empty.is_active);
        assert_eq!(empty.status_label(), "無効");
    }

    /// 評価ルールは `rule` の下に識別子とパラメータが入る二段構造。
    /// 種別ごとに `parameter` の項目が違うのでまとめて受ける。
    #[test]
    fn parses_evaluation_rules_with_varying_parameters() {
        let body = r#"{
            "items": [
                {
                    "rule": {
                        "evaluationRuleId": "addon-threat-detection-enabled",
                        "parameter": {"servicePrincipalId": "1",
                                      "zones": ["tk1a", "is1a"]}
                    },
                    "description": "脅威検知が有効か",
                    "iamRolesRequired": ["閲覧"],
                    "isEnabled": true
                },
                {
                    "rule": {
                        "evaluationRuleId": "objectstorage-bucket-acl-changed",
                        "parameter": {"servicePrincipalId": "2", "sites": ["isk01"]}
                    },
                    "iamRolesRequired": [],
                    "isEnabled": false
                },
                {
                    "rule": {"evaluationRuleId": "addon-threat-detections"},
                    "isEnabled": true
                }
            ]
        }"#;
        let (rules, next) = parse_rule_page(body).unwrap();
        assert!(next.is_none());
        assert_eq!(rules.len(), 3);

        assert_eq!(rules[0].id, "addon-threat-detection-enabled");
        assert_eq!(rules[0].zones, vec!["tk1a", "is1a"]);
        assert_eq!(rules[0].scope_label(), "tk1a, is1a");
        assert_eq!(rules[0].status_label(), "有効");
        assert_eq!(rules[0].iam_roles_required, vec!["閲覧"]);

        assert_eq!(rules[1].sites, vec!["isk01"]);
        assert_eq!(rules[1].scope_label(), "isk01");
        assert_eq!(rules[1].status_label(), "無効");

        // parameter を持たない種別でも落ちない。空は「すべて対象」の意味。
        assert_eq!(rules[2].id, "addon-threat-detections");
        assert_eq!(rules[2].scope_label(), "");
    }

    /// 自動アクションは `action.actionType` で中身が変わる。
    #[test]
    fn parses_automated_actions_for_both_action_types() {
        let body = r#"{
            "items": [
                {
                    "automatedActionId": "act-1",
                    "name": "公開IP検知で通知",
                    "description": "説明",
                    "action": {
                        "actionType": "simpleNotification",
                        "actionParameter": {"servicePrincipalId": "sp-1",
                                            "notificationGroupId": "grp-9"}
                    },
                    "executionCondition": "rule == 'server-no-public-ip'",
                    "isEnabled": true,
                    "createdAt": "2026-01-01T00:00:00Z"
                },
                {
                    "automatedActionId": "act-2",
                    "name": "ワークフロー実行",
                    "action": {
                        "actionType": "workflows",
                        "actionParameter": {"servicePrincipalId": "sp-2",
                                            "workflowId": "wf-3", "revisionId": 7}
                    },
                    "executionCondition": "true",
                    "isEnabled": false,
                    "createdAt": "2026-01-02T00:00:00Z"
                }
            ],
            "nextPageToken": "tok-2"
        }"#;
        let (actions, next) = parse_action_page(body).unwrap();
        assert_eq!(next, Some("tok-2".to_string()));

        assert_eq!(actions[0].action_type_label(), "シンプル通知");
        assert_eq!(actions[0].target_label(), "grp-9");
        assert_eq!(actions[0].status_label(), "有効");
        assert_eq!(
            actions[0].execution_condition,
            "rule == 'server-no-public-ip'"
        );

        assert_eq!(actions[1].action_type_label(), "ワークフロー");
        assert_eq!(actions[1].target_label(), "wf-3 (7)");
        assert_eq!(actions[1].status_label(), "無効");
    }

    /// リビジョンはエイリアスが優先。どちらも無ければ最新なので空にする。
    #[test]
    fn workflow_revision_prefers_the_alias() {
        let body = r#"{"items": [
            {"automatedActionId": "a", "name": "n",
             "action": {"actionType": "workflows",
                        "actionParameter": {"workflowId": "wf", "revisionId": 7,
                                            "revisionAlias": "stable"}}},
            {"automatedActionId": "b", "name": "n",
             "action": {"actionType": "workflows",
                        "actionParameter": {"workflowId": "wf"}}}
        ]}"#;
        let (actions, _) = parse_action_page(body).unwrap();
        assert_eq!(actions[0].target_label(), "wf (stable)");
        assert_eq!(actions[1].target_label(), "wf");
    }

    /// null や欠けた項目でも落ちないこと。
    #[test]
    fn tolerates_nulls_and_missing_fields() {
        let body = r#"{"items": [null, {"automatedActionId": "a", "name": null,
                                        "action": null, "isEnabled": null}],
                       "nextPageToken": null}"#;
        let (actions, next) = parse_action_page(body).unwrap();
        assert!(next.is_none());
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type_label(), "");
        assert_eq!(actions[0].target_label(), "");
        assert!(!actions[0].is_enabled);

        let (rules, _) = parse_rule_page(r#"{"items": [null, {"rule": null}]}"#).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "");
    }

    /// 空文字のページトークンは「次ページなし」として扱う。
    #[test]
    fn blank_page_token_stops_pagination() {
        let (_, next) = parse_rule_page(r#"{"items": [], "nextPageToken": ""}"#).unwrap();
        assert!(next.is_none());
    }

    /// ページングのクエリは page_size と next。
    #[test]
    fn page_query_carries_the_cursor() {
        assert_eq!(page_query(None), vec![("page_size", "100".to_string())]);
        assert_eq!(
            page_query(Some("tok".to_string())),
            vec![
                ("page_size", "100".to_string()),
                ("next", "tok".to_string())
            ]
        );
    }

    /// 接続先ごとにゾーンを解決する。cloud-test に is1a は無い。
    #[test]
    fn zone_follows_the_environment() {
        assert_eq!(
            security_control_zone("https://secure.sakura.ad.jp/cloud/zone", "is1b"),
            "is1a"
        );
        assert_eq!(
            security_control_zone(crate::config::TEST_API_ROOT, "is1x"),
            "is1x"
        );
    }
}
