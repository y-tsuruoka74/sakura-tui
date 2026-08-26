//! さくらのクラウド NoSQL（Cassandra 互換マネージドDB）の読み取り専用クライアント。
//!
//! 専用APIではなく IaaS API 1.1 のアプライアンス（`/appliance?Filter.Class=nosql`）
//! として提供されているため、認証もページングも [`SacloudClient`] をそのまま使う。
//!
//! レスポンスの命名規則が3系統に分かれているのが最大の注意点。
//! 一覧と状態は PascalCase、健全性は PascalCase だがラッパーが大文字 `Nosql`、
//! バックアップとパラメータは lowerCamelCase でラッパーが小文字 `nosql`。

use anyhow::Result;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use crate::sacloud::{SacloudClient, flexible_number, null_as_default};

/// 1 ページあたりの取得件数。
const PAGE_SIZE: usize = 100;
/// ページングを辿る上限。API が実態と違う総件数を返しても止まるようにする。
const MAX_PAGES: usize = 100;

/// NoSQL の提供ゾーン（本番）。
///
/// 公式マニュアルと OpenAPI の `servers` がどちらも東京第2に固定している。
const PRODUCTION_ZONE: &str = "tk1b";

/// アプライアンス一覧を NoSQL に絞り込むクラス名。
const NOSQL_CLASS: &str = "nosql";

// ---------------------------------------------------------------------------
// 公開型
// ---------------------------------------------------------------------------

/// NoSQL データベース 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlDatabase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// `migrating` / `available` / `failed`。
    pub availability: String,
    /// `Instance.Status`。仕様に enum が無いため受け取った値をそのまま持つ。
    pub status: String,
    pub status_changed_at: String,
    pub created_at: String,
    pub plan: NoSqlPlan,
    pub engine: String,
    pub version: String,
    pub default_user: String,
    pub storage: String,
    pub port: u32,
    pub nodes: u32,
    pub zone: String,
    /// ユーザー側スイッチに接続する IP アドレス。
    pub ip_addresses: Vec<String>,
    pub default_route: String,
    pub network_mask_len: u32,
    /// 接続を許可するネットワーク（CIDR）。
    pub source_networks: Vec<String>,
    pub reserve_ip_address: String,
    pub backup: Option<NoSqlBackupSetting>,
    pub repair: Option<NoSqlRepairSetting>,
    pub encryption_key_id: String,
    pub encryption_algorithm: String,
    pub service_class: String,
}

impl NoSqlDatabase {
    /// 一覧の「状態」列に出す文字列。
    ///
    /// `Availability` と `Instance.Status` は別軸なので、食い違うときだけ併記する。
    pub fn status_label(&self) -> String {
        let availability = availability_label(&self.availability);
        let instance = instance_status_label(&self.status);
        match (availability.is_empty(), instance.is_empty()) {
            (true, true) => String::new(),
            (true, false) => instance,
            (false, true) => availability,
            (false, false) if availability == instance => availability,
            // 「移行中 / 停止」のように、可用性と稼働状態がずれている状況を隠さない。
            (false, false) => format!("{availability} / {instance}"),
        }
    }
}

/// プラン。仕様に載っている諸元を `Plan.ID` から引く。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlPlan {
    pub id: u64,
    pub service_class: String,
    /// 40GB・100GB のような表示名。未知のプランでは空。
    pub name: String,
    pub cores: u32,
    pub memory_mb: u32,
    pub disk_mb: u32,
}

impl NoSqlPlan {
    /// 一覧の「プラン」列に出す文字列。
    ///
    /// 未知の `Plan.ID` でも情報を落とさないよう `ServiceClass` に退避する。
    pub fn label(&self) -> String {
        if !self.name.is_empty() {
            return self.name.clone();
        }
        if !self.service_class.is_empty() {
            return self.service_class.clone();
        }
        if self.id != 0 {
            return self.id.to_string();
        }
        String::new()
    }

    /// 詳細パネル向けの諸元。分からない項目は落とす。
    pub fn spec_label(&self) -> String {
        let mut parts = Vec::new();
        if self.cores != 0 {
            parts.push(format!("{}コア", self.cores));
        }
        if self.memory_mb != 0 {
            parts.push(format!("メモリ{}", format_mb(self.memory_mb)));
        }
        if self.disk_mb != 0 {
            parts.push(format!("ディスク{}", format_mb(self.disk_mb)));
        }
        parts.join(" / ")
    }
}

/// バックアップ設定（`Settings.Backup`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlBackupSetting {
    pub connect: String,
    pub day_of_week: Vec<String>,
    pub time: String,
    pub rotate: u32,
}

/// リペア設定（`Settings.Repair`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlRepairSetting {
    /// 増分リペア。仕様上のキーは複数形の `DaysOfWeek`。
    pub incremental_days: Vec<String>,
    pub incremental_time: String,
    /// 完全リペアの実行間隔（日）。7 / 14 / 21 / 28。
    pub full_interval: u32,
    /// 完全リペア。仕様上のキーは単数形の `DayOfWeek`。
    pub full_day: String,
    pub full_time: String,
}

/// ノード 1 件。`PrimaryNodes` と `AddNodes` を統合したもの。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlNode {
    /// ノードの通番。
    pub index: u32,
    pub ip_address: String,
    /// `0` 正常 / `1` デッドノード / `2` 予備IPサーバ。
    pub node_type: String,
    /// このノードが属するアプライアンスの ID。
    pub appliance_id: String,
    pub availability: String,
    pub zone: String,
    /// プライマリノード側なら真。追加ノードなら偽。
    pub primary: bool,
}

impl NoSqlNode {
    pub fn node_type_label(&self) -> String {
        node_type_label(&self.node_type)
    }

    pub fn group_label(&self) -> &'static str {
        if self.primary {
            "プライマリ"
        } else {
            "追加"
        }
    }
}

/// NoSQL の状態（`/appliance/{id}/status`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlStatus {
    pub appliance_id: String,
    pub version: String,
    /// 更新可能な最新バージョン。現在と同じなら更新なし。
    pub upgrade_version: String,
    pub jobs: Vec<NoSqlJob>,
    pub nodes: Vec<NoSqlNode>,
}

impl NoSqlStatus {
    /// 更新可能なバージョンがあるか。
    pub fn upgrade_available(&self) -> bool {
        !self.upgrade_version.is_empty() && self.upgrade_version != self.version
    }
}

/// 実行中・完了したジョブ。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlJob {
    pub job_type: String,
    pub status: String,
}

/// ノード全体の健全性（`/appliance/{id}/nosql/nodes/health`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlNodeHealth {
    /// `healthy` / `healthy-partial` / `unhealthy`。
    pub status: String,
}

impl NoSqlNodeHealth {
    pub fn label(&self) -> String {
        match self.status.as_str() {
            "healthy" => "起動".to_string(),
            "healthy-partial" => "部分起動（1台停止）".to_string(),
            "unhealthy" => "停止（2台以上停止）".to_string(),
            "" => String::new(),
            other => other.to_string(),
        }
    }
}

/// バックアップ 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlBackup {
    pub id: String,
    pub destination: String,
    pub backup_at: String,
    pub restore_at: String,
    /// 仕様に単位の記載が無いため、数値のまま持つ。
    pub size: u64,
    pub delete_status: String,
    pub restore_status: String,
}

impl NoSqlBackup {
    pub fn delete_status_label(&self) -> String {
        progress_label(&self.delete_status, "削除")
    }

    pub fn restore_status_label(&self) -> String {
        progress_label(&self.restore_status, "復元")
    }
}

/// パラメータ 1 件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoSqlParameter {
    pub id: String,
    pub name: String,
    pub value: String,
    pub default_value: String,
    pub description: String,
    pub options: Vec<String>,
}

impl NoSqlParameter {
    /// 既定値から変更されているか。
    ///
    /// `settingValue` が空なら未設定＝既定値のままとみなす。
    pub fn overridden(&self) -> bool {
        !self.value.is_empty() && self.value != self.default_value
    }
}

// ---------------------------------------------------------------------------
// ラベル変換
// ---------------------------------------------------------------------------

fn availability_label(raw: &str) -> String {
    match raw {
        "available" => "稼働".to_string(),
        "migrating" => "移行中".to_string(),
        "failed" => "失敗".to_string(),
        other => other.to_string(),
    }
}

/// `Instance.Status` のラベル。
///
/// 仕様には `up` という example しか無く取りうる値の一覧が書かれていないため、
/// 知らない値は加工せずそのまま返す。
fn instance_status_label(raw: &str) -> String {
    match raw {
        "up" => "起動".to_string(),
        "down" => "停止".to_string(),
        other => other.to_string(),
    }
}

fn node_type_label(raw: &str) -> String {
    match raw {
        "0" => "正常".to_string(),
        "1" => "デッドノード".to_string(),
        "2" => "予備IPサーバ".to_string(),
        "" => String::new(),
        other => other.to_string(),
    }
}

/// `deleteStatus` / `restoreStatus` のラベル。数字だが文字列で返る。
fn progress_label(raw: &str, action: &str) -> String {
    match raw {
        "0" => format!("未{action}"),
        "1" => format!("{action}中"),
        "2" => format!("{action}完了"),
        "9" => format!("{action}失敗"),
        "" => String::new(),
        other => other.to_string(),
    }
}

fn format_mb(mb: u32) -> String {
    if mb >= 1024 && mb.is_multiple_of(1024) {
        format!("{}GB", mb / 1024)
    } else {
        format!("{mb}MB")
    }
}

/// `Plan.ID` から仕様に載っている諸元を引く。
///
/// （プラン名, コア数, メモリMB, ディスクMB）。「/ノード」プランは
/// 追加ノード用でスペックの記載が無いため、名前だけ返す。
fn plan_spec(plan_id: u64) -> Option<(&'static str, u32, u32, u32)> {
    match plan_id {
        51142 => Some(("40GB", 2, 4096, 40960)),
        51143 => Some(("100GB", 3, 8192, 102400)),
        51144 => Some(("250GB", 6, 16384, 256000)),
        51145 => Some(("100GB/ノード", 0, 0, 0)),
        51146 => Some(("250GB/ノード", 0, 0, 0)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// デシリアライズ用の内部型
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApplianceListResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Total")]
    total: usize,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Appliances")]
    appliances: Vec<RawAppliance>,
}

#[derive(Debug, Deserialize)]
struct RawAppliance {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Class")]
    class: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Description")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Tags")]
    tags: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Availability")]
    availability: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ServiceClass")]
    service_class: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "CreatedAt")]
    created_at: String,
    #[serde(rename = "Plan")]
    plan: Option<RawPlan>,
    #[serde(rename = "Instance")]
    instance: Option<RawInstance>,
    #[serde(rename = "Settings")]
    settings: Option<RawSettings>,
    #[serde(rename = "Remark")]
    remark: Option<RawRemark>,
    #[serde(rename = "Disk")]
    disk: Option<RawDisk>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Interfaces")]
    interfaces: Vec<Option<RawInterface>>,
}

#[derive(Debug, Deserialize)]
struct RawPlan {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "ID")]
    id: u64,
}

#[derive(Debug, Deserialize)]
struct RawInstance {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Status")]
    status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "StatusChangedAt")]
    status_changed_at: String,
}

#[derive(Debug, Deserialize)]
struct RawSettings {
    #[serde(rename = "Backup")]
    backup: Option<RawBackupSetting>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "SourceNetwork")]
    source_network: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ReserveIPAddress")]
    reserve_ip_address: String,
    #[serde(rename = "Repair")]
    repair: Option<RawRepairSetting>,
}

#[derive(Debug, Deserialize)]
struct RawBackupSetting {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Connect")]
    connect: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DayOfWeek")]
    day_of_week: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Time")]
    time: String,
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Rotate")]
    rotate: u32,
}

#[derive(Debug, Deserialize)]
struct RawRepairSetting {
    #[serde(rename = "Incremental")]
    incremental: Option<RawIncrementalRepair>,
    #[serde(rename = "Full")]
    full: Option<RawFullRepair>,
}

#[derive(Debug, Deserialize)]
struct RawIncrementalRepair {
    /// 増分リペアだけ複数形の `DaysOfWeek`。
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DaysOfWeek")]
    days_of_week: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Time")]
    time: String,
}

#[derive(Debug, Deserialize)]
struct RawFullRepair {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Interval")]
    interval: u32,
    /// 完全リペアは単数形の `DayOfWeek`。
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DayOfWeek")]
    day_of_week: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Time")]
    time: String,
}

#[derive(Debug, Deserialize)]
struct RawRemark {
    #[serde(rename = "Nosql")]
    nosql: Option<RawRemarkNosql>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Servers")]
    servers: Vec<Option<RawRemarkServer>>,
    #[serde(rename = "Network")]
    network: Option<RawRemarkNetwork>,
}

#[derive(Debug, Deserialize)]
struct RawRemarkNosql {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DatabaseEngine")]
    database_engine: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DatabaseVersion")]
    database_version: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DefaultUser")]
    default_user: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Storage")]
    storage: String,
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Port")]
    port: u32,
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Nodes")]
    nodes: u32,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Zone")]
    zone: String,
}

#[derive(Debug, Deserialize)]
struct RawRemarkServer {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "UserIPAddress")]
    user_ip_address: String,
}

#[derive(Debug, Deserialize)]
struct RawRemarkNetwork {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DefaultRoute")]
    default_route: String,
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "NetworkMaskLen")]
    network_mask_len: u32,
}

#[derive(Debug, Deserialize)]
struct RawDisk {
    #[serde(rename = "EncryptionKey")]
    encryption_key: Option<RawEncryptionKey>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "EncryptionAlgorithm")]
    encryption_algorithm: String,
}

#[derive(Debug, Deserialize)]
struct RawEncryptionKey {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "KMSKeyID")]
    kms_key_id: String,
}

#[derive(Debug, Deserialize)]
struct RawInterface {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "UserIPAddress")]
    user_ip_address: String,
}

/// `/appliance/{id}/status` のレスポンス。
#[derive(Debug, Deserialize)]
struct StatusResponse {
    #[serde(rename = "Appliance")]
    appliance: Option<RawStatusAppliance>,
}

#[derive(Debug, Deserialize)]
struct RawStatusAppliance {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "SettingsResponse")]
    settings_response: Option<RawSettingsResponse>,
}

#[derive(Debug, Deserialize)]
struct RawSettingsResponse {
    #[serde(rename = "Nosql")]
    nosql: Option<RawStatusNosql>,
}

#[derive(Debug, Deserialize)]
struct RawStatusNosql {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "DatabaseVersion")]
    database_version: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "UpgradeVersion")]
    upgrade_version: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Jobs")]
    jobs: Vec<Option<RawJob>>,
    #[serde(rename = "PrimaryNodes")]
    primary_nodes: Option<RawNodeGroup>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "AddNodes")]
    add_nodes: Vec<Option<RawNodeGroup>>,
}

#[derive(Debug, Deserialize)]
struct RawJob {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "JobType")]
    job_type: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "JobStatus")]
    job_status: String,
}

#[derive(Debug, Deserialize)]
struct RawNodeGroup {
    #[serde(rename = "Appliance")]
    appliance: Option<RawNodeAppliance>,
}

#[derive(Debug, Deserialize)]
struct RawNodeAppliance {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "ID")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Availability")]
    availability: String,
    #[serde(rename = "Zone")]
    zone: Option<RawZoneName>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Nodes")]
    nodes: Vec<Option<RawNodeStatus>>,
}

#[derive(Debug, Deserialize)]
struct RawZoneName {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawNodeStatus {
    #[serde(default, deserialize_with = "flexible_number")]
    #[serde(rename = "Index")]
    index: u32,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "UserIPAddress")]
    user_ip_address: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "NodeType")]
    node_type: String,
}

/// 健全性のレスポンス。ラッパーだけ大文字始まりの `Nosql`。
#[derive(Debug, Deserialize)]
struct NodeHealthResponse {
    #[serde(rename = "Nosql")]
    nosql: Option<RawNodeHealth>,
}

#[derive(Debug, Deserialize)]
struct RawNodeHealth {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "Status")]
    status: String,
}

/// バックアップのレスポンス。ラッパーは小文字 `nosql`、中身は lowerCamelCase。
#[derive(Debug, Deserialize)]
struct BackupResponse {
    nosql: Option<RawBackupList>,
}

#[derive(Debug, Deserialize)]
struct RawBackupList {
    #[serde(default, deserialize_with = "null_as_default")]
    backups: Vec<Option<RawBackup>>,
}

#[derive(Debug, Deserialize)]
struct RawBackup {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "backupId")]
    backup_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "backupDestination")]
    backup_destination: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "backupAt")]
    backup_at: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "restoreAt")]
    restore_at: String,
    #[serde(default, deserialize_with = "flexible_number")]
    size: u64,
    /// 数字だが仕様上は文字列。
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "deleteStatus")]
    delete_status: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "restoreStatus")]
    restore_status: String,
}

/// パラメータのレスポンス。ラッパーは小文字 `nosql`、中身は lowerCamelCase。
#[derive(Debug, Deserialize)]
struct ParameterResponse {
    nosql: Option<RawParameterList>,
}

#[derive(Debug, Deserialize)]
struct RawParameterList {
    #[serde(default, deserialize_with = "null_as_default")]
    parameters: Vec<Option<RawParameter>>,
}

#[derive(Debug, Deserialize)]
struct RawParameter {
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "settingItemId")]
    setting_item_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "settingItem")]
    setting_item: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "defaultValue")]
    default_value: String,
    #[serde(default, deserialize_with = "null_as_default")]
    description: String,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "parameterOptions")]
    parameter_options: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    #[serde(rename = "settingValue")]
    setting_value: String,
}

// ---------------------------------------------------------------------------
// パース
// ---------------------------------------------------------------------------

impl From<RawAppliance> for NoSqlDatabase {
    fn from(raw: RawAppliance) -> Self {
        let instance = raw.instance;
        let remark = raw.remark;
        let nosql = remark.as_ref().and_then(|r| r.nosql.as_ref());
        let network = remark.as_ref().and_then(|r| r.network.as_ref());
        let settings = raw.settings;
        let disk = raw.disk;

        // インターフェースと Remark.Servers の両方に IP が載る。
        // どちらか片方しか無い場合もあるため統合し、重複と空を落とす。
        let mut ip_addresses: Vec<String> = raw
            .interfaces
            .into_iter()
            .flatten()
            .map(|i| i.user_ip_address)
            .chain(
                remark
                    .as_ref()
                    .map(|r| {
                        r.servers
                            .iter()
                            .flatten()
                            .map(|s| s.user_ip_address.clone())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            )
            .filter(|ip| !ip.is_empty())
            .collect();
        ip_addresses.dedup();

        let plan_id = raw.plan.map(|p| p.id).unwrap_or_default();
        let spec = plan_spec(plan_id);

        NoSqlDatabase {
            id: raw.id,
            name: raw.name,
            description: raw.description,
            tags: raw.tags,
            availability: raw.availability,
            status: instance
                .as_ref()
                .map(|i| i.status.clone())
                .unwrap_or_default(),
            status_changed_at: instance
                .as_ref()
                .map(|i| i.status_changed_at.clone())
                .unwrap_or_default(),
            created_at: raw.created_at,
            plan: NoSqlPlan {
                id: plan_id,
                service_class: raw.service_class.clone(),
                name: spec.map(|s| s.0.to_string()).unwrap_or_default(),
                cores: spec.map(|s| s.1).unwrap_or_default(),
                memory_mb: spec.map(|s| s.2).unwrap_or_default(),
                disk_mb: spec.map(|s| s.3).unwrap_or_default(),
            },
            engine: nosql.map(|n| n.database_engine.clone()).unwrap_or_default(),
            version: nosql
                .map(|n| n.database_version.clone())
                .unwrap_or_default(),
            default_user: nosql.map(|n| n.default_user.clone()).unwrap_or_default(),
            storage: nosql.map(|n| n.storage.clone()).unwrap_or_default(),
            port: nosql.map(|n| n.port).unwrap_or_default(),
            nodes: nosql.map(|n| n.nodes).unwrap_or_default(),
            zone: nosql.map(|n| n.zone.clone()).unwrap_or_default(),
            ip_addresses,
            default_route: network.map(|n| n.default_route.clone()).unwrap_or_default(),
            network_mask_len: network.map(|n| n.network_mask_len).unwrap_or_default(),
            source_networks: settings
                .as_ref()
                .map(|s| s.source_network.clone())
                .unwrap_or_default(),
            reserve_ip_address: settings
                .as_ref()
                .map(|s| s.reserve_ip_address.clone())
                .unwrap_or_default(),
            backup: settings
                .as_ref()
                .and_then(|s| s.backup.as_ref())
                .map(|b| NoSqlBackupSetting {
                    connect: b.connect.clone(),
                    day_of_week: b.day_of_week.clone(),
                    time: b.time.clone(),
                    rotate: b.rotate,
                }),
            repair: settings
                .as_ref()
                .and_then(|s| s.repair.as_ref())
                .map(|r| NoSqlRepairSetting {
                    incremental_days: r
                        .incremental
                        .as_ref()
                        .map(|i| i.days_of_week.clone())
                        .unwrap_or_default(),
                    incremental_time: r
                        .incremental
                        .as_ref()
                        .map(|i| i.time.clone())
                        .unwrap_or_default(),
                    full_interval: r.full.as_ref().map(|f| f.interval).unwrap_or_default(),
                    full_day: r
                        .full
                        .as_ref()
                        .map(|f| f.day_of_week.clone())
                        .unwrap_or_default(),
                    full_time: r.full.as_ref().map(|f| f.time.clone()).unwrap_or_default(),
                }),
            encryption_key_id: disk
                .as_ref()
                .and_then(|d| d.encryption_key.as_ref())
                .map(|k| k.kms_key_id.clone())
                .unwrap_or_default(),
            encryption_algorithm: disk
                .as_ref()
                .map(|d| d.encryption_algorithm.clone())
                .unwrap_or_default(),
            service_class: raw.service_class,
        }
    }
}

fn parse_databases(body: &str) -> Result<(Vec<NoSqlDatabase>, usize)> {
    let parsed: ApplianceListResponse = parse_json(body)?;
    let total = parsed.total;
    let items = parsed
        .appliances
        .into_iter()
        // API 側のフィルターを信用しきらず、別のアプライアンスの混入を防ぐ。
        .filter(|raw| raw.class.is_empty() || raw.class == NOSQL_CLASS)
        .map(NoSqlDatabase::from)
        .collect();
    Ok((items, total))
}

fn parse_status(body: &str) -> Result<NoSqlStatus> {
    let parsed: StatusResponse = parse_json(body)?;
    let Some(appliance) = parsed.appliance else {
        return Ok(NoSqlStatus::default());
    };
    let Some(nosql) = appliance.settings_response.and_then(|s| s.nosql) else {
        return Ok(NoSqlStatus {
            appliance_id: appliance.id,
            ..NoSqlStatus::default()
        });
    };

    let mut nodes = Vec::new();
    if let Some(group) = nosql.primary_nodes {
        collect_nodes(group, true, &mut nodes);
    }
    for group in nosql.add_nodes.into_iter().flatten() {
        collect_nodes(group, false, &mut nodes);
    }
    // プライマリを先に、その中は通番順に並べる。
    nodes.sort_by(|a, b| b.primary.cmp(&a.primary).then(a.index.cmp(&b.index)));

    Ok(NoSqlStatus {
        appliance_id: appliance.id,
        version: nosql.database_version,
        upgrade_version: nosql.upgrade_version,
        jobs: nosql
            .jobs
            .into_iter()
            .flatten()
            .map(|j| NoSqlJob {
                job_type: j.job_type,
                status: j.job_status,
            })
            .collect(),
        nodes,
    })
}

fn collect_nodes(group: RawNodeGroup, primary: bool, out: &mut Vec<NoSqlNode>) {
    let Some(appliance) = group.appliance else {
        return;
    };
    let zone = appliance.zone.map(|z| z.name).unwrap_or_default();
    for node in appliance.nodes.into_iter().flatten() {
        out.push(NoSqlNode {
            index: node.index,
            ip_address: node.user_ip_address,
            node_type: node.node_type,
            appliance_id: appliance.id.clone(),
            availability: appliance.availability.clone(),
            zone: zone.clone(),
            primary,
        });
    }
}

fn parse_node_health(body: &str) -> Result<NoSqlNodeHealth> {
    let parsed: NodeHealthResponse = parse_json(body)?;
    Ok(NoSqlNodeHealth {
        status: parsed.nosql.map(|n| n.status).unwrap_or_default(),
    })
}

fn parse_backups(body: &str) -> Result<Vec<NoSqlBackup>> {
    let parsed: BackupResponse = parse_json(body)?;
    let Some(list) = parsed.nosql else {
        return Ok(Vec::new());
    };
    Ok(list
        .backups
        .into_iter()
        .flatten()
        .map(|raw| NoSqlBackup {
            id: raw.backup_id,
            destination: raw.backup_destination,
            backup_at: raw.backup_at,
            restore_at: raw.restore_at,
            size: raw.size,
            delete_status: raw.delete_status,
            restore_status: raw.restore_status,
        })
        .collect())
}

fn parse_parameters(body: &str) -> Result<Vec<NoSqlParameter>> {
    let parsed: ParameterResponse = parse_json(body)?;
    let Some(list) = parsed.nosql else {
        return Ok(Vec::new());
    };
    Ok(list
        .parameters
        .into_iter()
        .flatten()
        .map(|raw| NoSqlParameter {
            id: raw.setting_item_id,
            name: raw.setting_item,
            value: raw.setting_value,
            default_value: raw.default_value,
            description: raw.description,
            options: raw.parameter_options,
        })
        .collect())
}

/// 本文が空でも落ちないようにしてから解析する。
fn parse_json<T: serde::de::DeserializeOwned>(body: &str) -> Result<T> {
    use anyhow::Context;
    let body = if body.trim().is_empty() { "{}" } else { body };
    serde_json::from_str(body).with_context(|| {
        let head: String = body.chars().take(200).collect();
        format!("NoSQL APIレスポンスの解析に失敗しました: {head}")
    })
}

// ---------------------------------------------------------------------------
// API 呼び出し
// ---------------------------------------------------------------------------

/// 接続先に応じた NoSQL の問い合わせ先ゾーン。
///
/// 本番では東京第2に固定されているが、社内テスト環境（cloud-test）に tk1b は
/// 存在しない（is1x / is1y / is1z / tk1s）。決め打ちにすると必ず失敗するため、
/// テスト環境ではプロファイルの既定ゾーンへ回す。
fn nosql_zone<'a>(api_root: &str, default_zone: &'a str) -> &'a str {
    if api_root == crate::config::TEST_API_ROOT {
        default_zone
    } else {
        PRODUCTION_ZONE
    }
}

impl SacloudClient {
    /// NoSQL の問い合わせ先ゾーン。画面にも表示する。
    pub fn nosql_zone(&self) -> &str {
        nosql_zone(self.api_root(), self.default_zone())
    }

    pub async fn list_nosql_databases(&self) -> Result<Vec<NoSqlDatabase>> {
        let zone = self.nosql_zone().to_string();
        let mut out = Vec::new();
        let mut from = 0usize;
        for _ in 0..MAX_PAGES {
            let body = json!({
                "Filter": { "Class": NOSQL_CLASS },
                "From": from,
                "Count": PAGE_SIZE,
                "Sort": ["Name"],
            });
            let text: String = self.nosql_get_text(&zone, "appliance", Some(body)).await?;
            let (items, total) = parse_databases(&text)?;
            let received = items.len();
            out.extend(items);
            if received == 0 || from + received >= total {
                break;
            }
            from += received;
        }
        Ok(out)
    }

    pub async fn nosql_status(&self, database_id: &str) -> Result<NoSqlStatus> {
        let zone = self.nosql_zone().to_string();
        let path = format!("appliance/{database_id}/status");
        let text = self.nosql_get_text(&zone, &path, None).await?;
        parse_status(&text)
    }

    pub async fn nosql_node_health(&self, database_id: &str) -> Result<NoSqlNodeHealth> {
        let zone = self.nosql_zone().to_string();
        let path = format!("appliance/{database_id}/nosql/nodes/health");
        let text = self.nosql_get_text(&zone, &path, None).await?;
        parse_node_health(&text)
    }

    pub async fn list_nosql_backups(&self, database_id: &str) -> Result<Vec<NoSqlBackup>> {
        let zone = self.nosql_zone().to_string();
        let path = format!("appliance/{database_id}/nosql/backup");
        let text = self.nosql_get_text(&zone, &path, None).await?;
        parse_backups(&text)
    }

    pub async fn list_nosql_parameters(&self, database_id: &str) -> Result<Vec<NoSqlParameter>> {
        let zone = self.nosql_zone().to_string();
        let path = format!("appliance/{database_id}/nosql/parameter");
        let text = self.nosql_get_text(&zone, &path, None).await?;
        parse_parameters(&text)
    }

    /// 生の本文で受け取る。
    ///
    /// 命名規則が3系統に分かれていて型を出し分ける必要があるため、
    /// 解析はそれぞれの `parse_*` に任せる。
    async fn nosql_get_text(
        &self,
        zone: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<String> {
        let value: serde_json::Value = self.request_in_zone(zone, Method::GET, path, body).await?;
        Ok(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一覧の封筒を剥がし、`Appliance.ID` は文字列・`Plan.ID` は数値という
    /// 型の食い違いを吸収できること。
    #[test]
    fn parses_database_list_with_mixed_id_types() {
        let body = r#"{
            "From": 0, "Count": 1, "Total": 1,
            "Appliances": [{
                "Class": "nosql",
                "Name": "CassandraName",
                "Description": "説明",
                "Tags": ["tag1"],
                "ID": "113600097295",
                "Plan": {"ID": 51143},
                "Availability": "available",
                "ServiceClass": "cloud/nosql/plan/2",
                "CreatedAt": "2021-01-01T00:00:00Z",
                "Instance": {"Status": "up", "StatusChangedAt": "2021-01-02T00:00:00Z"},
                "Remark": {
                    "Nosql": {
                        "DatabaseEngine": "Cassandra",
                        "DatabaseVersion": "4.1.9",
                        "DefaultUser": "defaultuser01",
                        "Storage": "SSD",
                        "Port": 9042,
                        "Nodes": 3,
                        "Zone": "tk1b"
                    },
                    "Servers": [{"UserIPAddress": "192.168.100.11"}],
                    "Network": {"DefaultRoute": "192.168.100.254", "NetworkMaskLen": 24}
                },
                "Interfaces": [{"UserIPAddress": "192.168.100.11"}]
            }],
            "is_ok": true
        }"#;
        let (items, total) = parse_databases(body).unwrap();
        assert_eq!(total, 1);
        assert_eq!(items.len(), 1);
        let db = &items[0];
        assert_eq!(db.id, "113600097295");
        assert_eq!(db.plan.id, 51143);
        assert_eq!(db.plan.name, "100GB");
        assert_eq!(db.plan.cores, 3);
        assert_eq!(db.engine, "Cassandra");
        assert_eq!(db.version, "4.1.9");
        assert_eq!(db.port, 9042);
        assert_eq!(db.nodes, 3);
        assert_eq!(db.zone, "tk1b");
        assert_eq!(db.default_route, "192.168.100.254");
        assert_eq!(db.network_mask_len, 24);
        // Interfaces と Remark.Servers に同じ IP が載っていても重複させない。
        assert_eq!(db.ip_addresses, vec!["192.168.100.11".to_string()]);
        assert_eq!(db.status_label(), "稼働 / 起動");
    }

    /// nullable の項目が軒並み null でも落ちないこと。
    /// `Interfaces` は要素自体が null になりうる。
    #[test]
    fn tolerates_nulls_across_nullable_fields() {
        let body = r#"{
            "From": 0, "Count": 1, "Total": 1,
            "Appliances": [{
                "Class": "nosql",
                "ID": "1",
                "Name": "n",
                "Tags": null,
                "Settings": {"Backup": null, "Repair": null, "SourceNetwork": null},
                "Disk": null,
                "Instance": {"Status": "up", "StatusChangedAt": null},
                "Remark": {"Nosql": null, "Servers": null, "Network": null},
                "Interfaces": [null, {"UserIPAddress": null}]
            }]
        }"#;
        let (items, _) = parse_databases(body).unwrap();
        let db = &items[0];
        assert!(db.tags.is_empty());
        assert!(db.backup.is_none());
        assert!(db.repair.is_none());
        assert!(db.ip_addresses.is_empty());
        assert!(db.encryption_key_id.is_empty());
        assert_eq!(db.status_label(), "起動");
    }

    /// リペア設定は増分だけ複数形 `DaysOfWeek`、完全は単数形 `DayOfWeek`。
    /// 綴りを取り違えると値が黙って落ちるので固定する。
    #[test]
    fn parses_backup_and_repair_settings_with_differing_day_keys() {
        let body = r#"{
            "Total": 1,
            "Appliances": [{
                "Class": "nosql", "ID": "1",
                "Settings": {
                    "Backup": {
                        "Connect": "nfs://192.168.100.250/export",
                        "DayOfWeek": ["sun", "mon"],
                        "Time": "00:00",
                        "Rotate": 3
                    },
                    "SourceNetwork": ["192.168.100.200"],
                    "ReserveIPAddress": "192.168.100.203",
                    "Repair": {
                        "Incremental": {"DaysOfWeek": ["tue", "wed"], "Time": "01:00"},
                        "Full": {"Interval": 14, "DayOfWeek": "sat", "Time": "02:00"}
                    }
                }
            }]
        }"#;
        let (items, _) = parse_databases(body).unwrap();
        let db = &items[0];
        let backup = db.backup.as_ref().unwrap();
        assert_eq!(backup.connect, "nfs://192.168.100.250/export");
        assert_eq!(backup.day_of_week, vec!["sun", "mon"]);
        assert_eq!(backup.rotate, 3);
        let repair = db.repair.as_ref().unwrap();
        assert_eq!(repair.incremental_days, vec!["tue", "wed"]);
        assert_eq!(repair.incremental_time, "01:00");
        assert_eq!(repair.full_interval, 14);
        assert_eq!(repair.full_day, "sat");
        assert_eq!(db.source_networks, vec!["192.168.100.200".to_string()]);
        assert_eq!(db.reserve_ip_address, "192.168.100.203");
    }

    /// API 側のフィルターが効かず別クラスが混ざっても取り除くこと。
    #[test]
    fn drops_appliances_of_other_classes() {
        let body = r#"{
            "Total": 2,
            "Appliances": [
                {"Class": "nosql", "ID": "1", "Name": "keep"},
                {"Class": "database", "ID": "2", "Name": "drop"}
            ]
        }"#;
        let (items, _) = parse_databases(body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "keep");
    }

    /// 状態APIの `PrimaryNodes` と `AddNodes` を 1 つの表に統合し、
    /// プライマリを先頭・通番順に並べること。
    #[test]
    fn merges_primary_and_additional_nodes_in_order() {
        let body = r#"{
            "Appliance": {
                "ID": "113600097295",
                "SettingsResponse": {
                    "Nosql": {
                        "DatabaseVersion": "4.1.7",
                        "UpgradeVersion": "4.1.9",
                        "Jobs": [{"JobType": "Create", "JobStatus": "Done"}],
                        "PrimaryNodes": {
                            "Appliance": {
                                "ID": "113700352689",
                                "Availability": "available",
                                "Zone": {"Name": "tk1b"},
                                "Nodes": [
                                    {"Index": 1, "UserIPAddress": "192.168.100.12", "NodeType": "1"},
                                    {"Index": 0, "UserIPAddress": "192.168.100.11", "NodeType": "0"}
                                ]
                            }
                        },
                        "AddNodes": [{
                            "Appliance": {
                                "ID": "113700352690",
                                "Availability": "available",
                                "Zone": {"Name": "tk1b"},
                                "Nodes": [
                                    {"Index": 2, "UserIPAddress": "192.168.100.13", "NodeType": "2"}
                                ]
                            }
                        }]
                    }
                }
            },
            "is_ok": true
        }"#;
        let status = parse_status(body).unwrap();
        assert_eq!(status.appliance_id, "113600097295");
        assert_eq!(status.version, "4.1.7");
        assert_eq!(status.upgrade_version, "4.1.9");
        assert!(status.upgrade_available());
        assert_eq!(status.jobs.len(), 1);
        assert_eq!(status.jobs[0].job_type, "Create");

        let indices: Vec<u32> = status.nodes.iter().map(|n| n.index).collect();
        assert_eq!(indices, vec![0, 1, 2]);
        assert!(status.nodes[0].primary);
        assert!(!status.nodes[2].primary);
        assert_eq!(status.nodes[0].node_type_label(), "正常");
        assert_eq!(status.nodes[1].node_type_label(), "デッドノード");
        assert_eq!(status.nodes[2].node_type_label(), "予備IPサーバ");
        assert_eq!(status.nodes[2].group_label(), "追加");
        assert_eq!(status.nodes[2].appliance_id, "113700352690");
    }

    /// 現在と更新可能が同じバージョンなら「更新あり」と誤表示しないこと。
    #[test]
    fn same_version_is_not_an_available_upgrade() {
        let status = NoSqlStatus {
            version: "4.1.9".to_string(),
            upgrade_version: "4.1.9".to_string(),
            ..NoSqlStatus::default()
        };
        assert!(!status.upgrade_available());
    }

    /// 健全性のラッパーは大文字始まりの `Nosql`。
    #[test]
    fn parses_node_health_with_capitalized_wrapper() {
        let body = r#"{"Success": true, "is_ok": true, "Nosql": {"Status": "healthy-partial"}}"#;
        let health = parse_node_health(body).unwrap();
        assert_eq!(health.status, "healthy-partial");
        assert_eq!(health.label(), "部分起動（1台停止）");
    }

    /// バックアップのラッパーは小文字 `nosql`、中身は lowerCamelCase。
    /// 状態コードは数字だが文字列で返る。
    #[test]
    fn parses_backups_with_lowercase_wrapper_and_string_status_codes() {
        let body = r#"{
            "nosql": {
                "backups": [{
                    "backupId": "123e4567-e89b-12d3-a456-426614174000",
                    "backupDestination": "nfs://192.168.100.250/export",
                    "backupAt": "2024-11-18T16:03:48.844+09:00",
                    "restoreAt": "2024-11-18T16:03:51.223+09:00",
                    "size": 2,
                    "deleteStatus": "2",
                    "restoreStatus": "1"
                }]
            },
            "is_ok": true
        }"#;
        let backups = parse_backups(body).unwrap();
        assert_eq!(backups.len(), 1);
        let backup = &backups[0];
        assert_eq!(backup.id, "123e4567-e89b-12d3-a456-426614174000");
        assert_eq!(backup.size, 2);
        assert_eq!(backup.delete_status_label(), "削除完了");
        assert_eq!(backup.restore_status_label(), "復元中");
    }

    /// パラメータも小文字 `nosql` ラッパー。既定値との差分を判定できること。
    #[test]
    fn parses_parameters_and_detects_overrides() {
        let body = r#"{
            "nosql": {
                "parameters": [
                    {
                        "settingItemId": "id-1",
                        "settingItem": "settingItem1",
                        "defaultValue": "16",
                        "description": "説明1",
                        "parameterOptions": ["16", "32"],
                        "settingValue": "32"
                    },
                    {
                        "settingItemId": "id-2",
                        "settingItem": "settingItem2",
                        "defaultValue": "on",
                        "description": "説明2"
                    }
                ]
            }
        }"#;
        let params = parse_parameters(body).unwrap();
        assert_eq!(params.len(), 2);
        assert!(params[0].overridden());
        assert_eq!(params[0].options, vec!["16", "32"]);
        // settingValue 未設定は既定値のままとみなす。
        assert!(!params[1].overridden());
    }

    /// 空のリストや欠けたラッパーでも空配列を返すこと。
    #[test]
    fn missing_wrappers_yield_empty_lists() {
        assert!(parse_backups("{}").unwrap().is_empty());
        assert!(parse_parameters("{}").unwrap().is_empty());
        assert_eq!(parse_node_health("{}").unwrap().status, "");
        assert_eq!(parse_status("{}").unwrap(), NoSqlStatus::default());
        let (items, total) = parse_databases("{}").unwrap();
        assert!(items.is_empty());
        assert_eq!(total, 0);
    }

    /// 未知のプランIDでも情報を落とさず `ServiceClass` に退避すること。
    #[test]
    fn unknown_plan_falls_back_to_service_class() {
        let known = NoSqlPlan {
            id: 51142,
            service_class: "cloud/nosql/plan/1".to_string(),
            name: "40GB".to_string(),
            cores: 2,
            memory_mb: 4096,
            disk_mb: 40960,
        };
        assert_eq!(known.label(), "40GB");
        assert_eq!(known.spec_label(), "2コア / メモリ4GB / ディスク40GB");

        let unknown = NoSqlPlan {
            id: 99999,
            service_class: "cloud/nosql/plan/9".to_string(),
            ..NoSqlPlan::default()
        };
        assert_eq!(unknown.label(), "cloud/nosql/plan/9");
        assert_eq!(unknown.spec_label(), "");
    }

    /// 接続先ごとにゾーンを解決する。
    /// cloud-test に tk1b は無いので決め打ちにしない。
    #[test]
    fn zone_follows_the_environment() {
        assert_eq!(
            nosql_zone("https://secure.sakura.ad.jp/cloud/zone", "is1b"),
            "tk1b"
        );
        assert_eq!(nosql_zone(crate::config::TEST_API_ROOT, "is1x"), "is1x");
    }

    /// 状態は可用性と稼働状態の食い違いを隠さない。
    #[test]
    fn status_label_keeps_both_axes_when_they_disagree() {
        let migrating = NoSqlDatabase {
            availability: "migrating".to_string(),
            status: "down".to_string(),
            ..NoSqlDatabase::default()
        };
        assert_eq!(migrating.status_label(), "移行中 / 停止");

        // 仕様に enum の無い Instance.Status は未知の値もそのまま出す。
        let unknown = NoSqlDatabase {
            availability: String::new(),
            status: "restarting".to_string(),
            ..NoSqlDatabase::default()
        };
        assert_eq!(unknown.status_label(), "restarting");
    }
}
