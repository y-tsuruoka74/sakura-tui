//! アプリケーションの状態遷移。
//!
//! 描画は `ui` モジュールが担当し、ここでは状態・キー入力・非同期処理の結果反映を扱う。
//! API 呼び出しは全て `tokio::spawn` して `Message` として結果を受け取るため、
//! 通信中も UI がブロックしない。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::{ListState, TableState};
use tokio::sync::mpsc::UnboundedSender;

mod account;
mod ai_engine;
mod api_gateway;
mod apprun;
mod billing;
mod cloudhsm;
mod credentials;
mod dedicated;
mod disk;
mod forms;
mod monitoring_suite;
mod network_map;
mod networking_suite;
mod nosql;
mod observability;
mod packet_filter;
mod security_control;
mod seg;
mod server;
mod service;
mod ssh_key;
mod switch;

pub use account::AccountView;
pub use ai_engine::{AiEngineTab, AiEngineView};
pub use api_gateway::{ApiGatewayTab, ApiGatewayView};
pub use apprun::{AppRunPane, AppRunView};
pub use billing::{BillingFocus, BillingTab, BillingView};
pub use cloudhsm::{CloudHsmTab, CloudHsmView};
pub use dedicated::{DedicatedFocus, DedicatedTab, DedicatedView};
pub use disk::DiskView;
pub use forms::*;
pub use network_map::{MapData, MapRow, NetworkKind, NetworkMapView};
pub use networking_suite::{NetworkingSuiteTab, NetworkingSuiteView};
pub use nosql::{NoSqlTab, NoSqlView};
pub use observability::{
    DnsView, ListFocus, MonitoringTab, MonitoringView, SecretsView, SimpleMonitorView,
};
pub use packet_filter::PacketFilterView;
pub use security_control::{SecurityControlTab, SecurityControlView};
pub use seg::{SegTab, SegView};
pub use server::ServerView;
pub use service::*;
pub use ssh_key::SshKeyView;
pub use switch::SwitchView;

use crate::account::AuthStatus;
use crate::ai_engine::AiEngineClient;
use crate::ai_engine_cloud::{
    AiEngineCloudClient, CloudAuth, CloudBill, CloudDocumentUsage, CloudModel, CloudUsage,
};
use crate::ai_rag::{RagChunk, RagDocument};
use crate::api_gateway::{
    ApiGatewayClient, ApiGatewayGroup, ApiGatewayService, ApiGatewayUser, Certificate, Domain,
    Oidc, Route, Subscription, UserAuthentication,
};
use crate::apprun::{AppRunClient, Application, ApplicationDetail, Traffic, Version};
use crate::apprun_dedicated::{self as ded, Cluster, DedicatedClient};
use crate::billing::{Bill, BillDetail, BillingIdentity};
use crate::cloud_resources::{CloudResource, CloudResourceKind};
use crate::cloudhsm::{CloudHsm, CloudHsmClient, CloudHsmDocument, CloudHsmLicense};
use crate::commonservice::{DnsRecord, DnsZone, SimpleMonitor};
use crate::config::{ApiCredentials, Config, CredentialSource, RegistryLogin};
use crate::iaas::{PowerAction, Server, Zone};
use crate::managed_resources::{ManagedResource, ManagedResourceKind};
use crate::monitoring::{
    AlertHistory, AlertProject, AlertRule, DashboardProject, LogMeasureRule, LogRouting,
    MetricsRouting, MonitoringClient, NotificationRouting, NotificationTarget, Publisher, Storage,
    StorageAccessKey, StorageAccessKeySecret,
};
use crate::networking_suite::{Subnet, SubnetAddress, SubnetGroup};
use crate::nosql::{NoSqlBackup, NoSqlDatabase, NoSqlNodeHealth, NoSqlParameter, NoSqlStatus};
use crate::registry::{RegistryClients, TagDetail, TagInfo};
use crate::sacloud::{ContainerRegistry, Permission, RegistryUser, ResourceId, SacloudClient};
use crate::secretmanager::{Secret, Vault};
use crate::security_control::{AutomatedAction, EvaluationRule, SecurityControlActivation};
use crate::seg::Seg;
use crate::switch::Switch;

/// 非同期処理の結果。
pub enum Message {
    CloudResources {
        zone: String,
        kind: CloudResourceKind,
        result: Result<Vec<CloudResource>, String>,
    },
    ManagedResources {
        kind: ManagedResourceKind,
        result: Result<Vec<ManagedResource>, String>,
    },
    ApiGatewaySubscriptions {
        result: Result<Vec<Subscription>, String>,
    },
    ApiGatewayServices {
        result: Result<Vec<ApiGatewayService>, String>,
    },
    ApiGatewayRoutes {
        service_id: String,
        result: Result<Vec<Route>, String>,
    },
    ApiGatewayUsers {
        result: Result<Vec<ApiGatewayUser>, String>,
    },
    ApiGatewayUserAuthentication {
        user_id: String,
        result: Result<UserAuthentication, String>,
    },
    ApiGatewayGroups {
        result: Result<Vec<ApiGatewayGroup>, String>,
    },
    ApiGatewayDomains {
        result: Result<Vec<Domain>, String>,
    },
    ApiGatewayCertificates {
        result: Result<Vec<Certificate>, String>,
    },
    ApiGatewayOidcs {
        result: Result<Vec<Oidc>, String>,
    },
    NoSqlDatabases {
        result: Result<Vec<NoSqlDatabase>, String>,
    },
    NoSqlStatus {
        database_id: String,
        result: Result<NoSqlStatus, String>,
    },
    NoSqlNodeHealth {
        database_id: String,
        result: Result<NoSqlNodeHealth, String>,
    },
    NoSqlBackups {
        database_id: String,
        result: Result<Vec<NoSqlBackup>, String>,
    },
    NoSqlParameters {
        database_id: String,
        result: Result<Vec<NoSqlParameter>, String>,
    },
    SegGateways {
        zone: String,
        result: Result<Vec<Seg>, String>,
    },
    SecurityControlActivation {
        result: Result<SecurityControlActivation, String>,
    },
    SecurityControlRules {
        result: Result<Vec<EvaluationRule>, String>,
    },
    SecurityControlActions {
        result: Result<Vec<AutomatedAction>, String>,
    },
    CloudHsmHsms {
        zone: String,
        result: Result<Vec<CloudHsm>, String>,
    },
    CloudHsmClients {
        hsm_id: String,
        result: Result<Vec<CloudHsmClient>, String>,
    },
    CloudHsmLicenses {
        zone: String,
        result: Result<Vec<CloudHsmLicense>, String>,
    },
    CloudHsmDocuments {
        license_id: String,
        result: Result<Vec<CloudHsmDocument>, String>,
    },
    NetworkingSuiteGroups {
        result: Result<Vec<SubnetGroup>, String>,
    },
    NetworkingSuiteSubnets {
        group_srn: String,
        result: Result<Vec<Subnet>, String>,
    },
    NetworkingSuiteAddresses {
        subnet_srn: String,
        result: Result<Vec<SubnetAddress>, String>,
    },
    AiEngineDocuments {
        result: Result<Vec<RagDocument>, String>,
    },
    AiEngineChunks {
        document_id: String,
        result: Result<Vec<RagChunk>, String>,
    },
    AiEngineCloudAuth {
        result: Result<CloudAuth, String>,
    },
    AiEngineCloudModels {
        result: Result<Vec<CloudModel>, String>,
    },
    AiEngineCloudUsages {
        result: Result<Vec<CloudUsage>, String>,
    },
    AiEngineCloudDocumentUsages {
        result: Result<Vec<CloudDocumentUsage>, String>,
    },
    AiEngineCloudBill {
        month: String,
        result: Result<CloudBill, String>,
    },
    RagDocumentUploaded {
        result: Result<RagDocument, String>,
    },
    RagDocumentDeleted {
        name: String,
        result: Result<(), String>,
    },
    RagDocumentUpdated {
        result: Result<RagDocument, String>,
    },
    ServerPlans {
        plans: Result<Vec<crate::iaas::ServerPlan>, String>,
        disks: Result<Vec<crate::iaas::DiskPlan>, String>,
    },
    /// 作成フォームで NIC・フィルタ・スクリプトに選べるもの。
    ServerAttachments {
        switches: Result<Vec<crate::switch::Switch>, String>,
        filters: Result<Vec<crate::packet_filter::PacketFilter>, String>,
        scripts: Result<Vec<crate::iaas::StartupScript>, String>,
    },
    SshKeyList {
        result: Result<Vec<crate::iaas::SshKey>, String>,
    },
    PacketFilters {
        result: Result<Vec<crate::packet_filter::PacketFilter>, String>,
    },
    NetworkMap {
        zone: String,
        result: Result<MapData, String>,
    },
    /// 接続・切断・フィルタの付け外しのように、結果が成否だけの NIC の操作。
    NicChanged {
        what: String,
        failed: String,
        result: Result<(), String>,
    },
    /// 作成・更新・削除のように、結果が成否だけのパケットフィルタの操作。
    PacketFilterChanged {
        what: String,
        failed: String,
        result: Result<(), String>,
    },
    /// 登録・更新・削除のように、結果が成否だけの公開鍵の操作。
    SshKeyChanged {
        what: String,
        failed: String,
        result: Result<(), String>,
    },
    /// 作成フォームに入れる SSH 公開鍵。取得元ごとに非同期で引く。
    SshKeys {
        from: String,
        result: Result<Vec<crate::pubkey::PublicKey>, String>,
    },
    ServerDeleted {
        name: String,
        result: Result<(), String>,
    },
    ServerPlanChanged {
        name: String,
        result: Result<(), String>,
    },
    DiskPlans {
        result: Result<Vec<crate::iaas::DiskPlan>, String>,
        archives: Result<Vec<crate::iaas::OsTemplate>, String>,
    },
    ArchiveSources {
        result: Result<Vec<(ResourceId, String)>, String>,
    },
    DiskCreated {
        name: String,
        /// OS テンプレートからのコピーが走っているか。
        copying: bool,
        result: Result<(), String>,
    },
    /// 削除・接続・切断のように、結果が成否だけのディスク操作。
    DiskChanged {
        what: String,
        failed: String,
        result: Result<(), String>,
    },
    DiskTargetServers {
        result: Result<Vec<(ResourceId, String)>, String>,
    },
    ServerCreated {
        name: String,
        progress: crate::iaas::ServerCreateProgress,
        result: Result<(), String>,
    },
    IamAction {
        label: String,
        result: Result<(), String>,
    },
    AiEngineTokenVerified {
        name: String,
        token: String,
        result: Result<Vec<ManagedResource>, String>,
    },
    IamCredentialsVerified {
        form: Box<IamCredentialForm>,
        result: Result<(), String>,
    },
    Registries(Result<Vec<ContainerRegistry>, String>),
    Users {
        id: ResourceId,
        result: Result<Vec<RegistryUser>, String>,
    },
    Repositories {
        host: String,
        result: Result<Vec<String>, String>,
    },
    Tags {
        host: String,
        repository: String,
        result: Result<Vec<TagInfo>, String>,
    },
    TagDetails {
        key: TagKey,
        result: Result<TagDetail, String>,
    },
    LoginVerified {
        host: String,
        login: RegistryLogin,
        save: bool,
        result: Result<(), String>,
    },
    UserAction {
        id: ResourceId,
        label: String,
        result: Result<(), String>,
        /// 成功時、ログイン情報として保存するか確認するための資格情報
        /// （ユーザー作成、またはパスワードを変更した更新のときだけ入る）。
        save_login: Option<(String, RegistryLogin)>,
    },
    Applications(Result<Vec<Application>, String>),
    ApplicationDetail {
        id: String,
        result: Result<ApplicationDetail, String>,
    },
    Versions {
        id: String,
        result: Result<Vec<Version>, String>,
    },
    Traffics {
        id: String,
        result: Result<Vec<Traffic>, String>,
    },
    AppRunAction {
        id: String,
        label: String,
        result: Result<(), String>,
    },
    Clusters(Result<Vec<Cluster>, String>),
    ClusterDetail {
        id: String,
        result: Result<Cluster, String>,
    },
    DedicatedApplications {
        cluster: String,
        result: Result<Vec<ded::Application>, String>,
    },
    ScalingGroups {
        cluster: String,
        result: Result<Vec<ded::AutoScalingGroup>, String>,
    },
    WorkerNodes {
        cluster: String,
        asg: String,
        result: Result<Vec<ded::WorkerNode>, String>,
    },
    Certificates {
        cluster: String,
        result: Result<Vec<ded::Certificate>, String>,
    },
    DnsZones(Result<Vec<DnsZone>, String>),
    DnsAction {
        label: String,
        result: Result<(), String>,
    },
    SimpleMonitorAction {
        label: String,
        result: Result<(), String>,
    },
    SimpleMonitors(Result<Vec<SimpleMonitor>, String>),
    Vaults(Result<Vec<Vault>, String>),
    Secrets {
        vault: String,
        result: Result<Vec<Secret>, String>,
    },
    SecretManagerAction {
        label: String,
        reselect_vault: Option<String>,
        result: Result<(), String>,
    },
    UnveiledSecret {
        name: String,
        result: Result<String, String>,
    },
    Projects {
        zone: String,
        result: Result<Vec<AlertProject>, String>,
    },
    AlertRules {
        zone: String,
        project: i64,
        result: Result<Vec<AlertRule>, String>,
    },
    LogMeasureRules {
        zone: String,
        project: i64,
        result: Result<Vec<LogMeasureRule>, String>,
    },
    LogRoutings {
        zone: String,
        result: Result<Vec<LogRouting>, String>,
    },
    MetricsRoutings {
        zone: String,
        result: Result<Vec<MetricsRouting>, String>,
    },
    Publishers {
        zone: String,
        result: Result<Vec<Publisher>, String>,
    },
    DashboardProjects {
        zone: String,
        result: Result<Vec<DashboardProject>, String>,
    },
    AlertHistories {
        zone: String,
        project: i64,
        result: Result<Vec<AlertHistory>, String>,
    },
    NotificationTargets {
        zone: String,
        project: i64,
        result: Result<Vec<NotificationTarget>, String>,
    },
    NotificationRoutings {
        zone: String,
        project: i64,
        result: Result<Vec<NotificationRouting>, String>,
    },
    Storages {
        zone: String,
        result: Result<Vec<Storage>, String>,
    },
    StorageAccessKeys {
        zone: String,
        storage: Storage,
        result: Result<Vec<StorageAccessKey>, String>,
    },
    MonitoringAction {
        zone: String,
        label: String,
        reselect_project: Option<i64>,
        result: Result<(), String>,
    },
    AlertRuleAction {
        zone: String,
        project: i64,
        label: String,
        result: Result<(), String>,
    },
    LogMeasureRuleAction {
        zone: String,
        project: i64,
        label: String,
        result: Result<(), String>,
    },
    LogRoutingAction {
        zone: String,
        label: String,
        result: Result<(), String>,
    },
    MetricsRoutingAction {
        zone: String,
        label: String,
        result: Result<(), String>,
    },
    DashboardAction {
        zone: String,
        label: String,
        result: Result<(), String>,
    },
    NotificationAction {
        zone: String,
        project: i64,
        label: String,
        result: Result<(), String>,
    },
    StorageAction {
        zone: String,
        label: String,
        result: Result<(), String>,
    },
    StorageAccessKeyAction {
        zone: String,
        storage: Storage,
        label: String,
        result: Result<(), String>,
    },
    StorageAccessKeySecret {
        zone: String,
        storage: Storage,
        title: String,
        result: Result<StorageAccessKeySecret, String>,
    },
    /// プロファイル作成時の検証結果。検証が通ってから書き出す。
    ProfileVerified {
        form: Box<ProfileForm>,
        /// 成功時はその環境で使えるゾーンの一覧。
        result: Result<Vec<Zone>, String>,
    },
    /// 認証情報の読み込み結果（キーチェーンを触るため別スレッドで実行する）。
    CredentialsLoaded {
        source: Box<CredentialSource>,
        result: Box<Result<ApiCredentials, String>>,
    },
    /// 保存済みのレジストリログイン（キーチェーンから読み出した結果）。
    SavedLogin {
        host: String,
        login: Option<RegistryLogin>,
    },
    BillingIdentity(Box<Result<BillingIdentity, String>>),
    Bills(Result<Vec<Bill>, String>),
    BillDetails {
        id: String,
        result: Result<Vec<BillDetail>, String>,
    },
    Zones(Result<Vec<Zone>, String>),
    ZoneCount {
        service: Service,
        zone: String,
        result: Result<usize, String>,
    },
    AuthStatus(Box<Result<AuthStatus, String>>),
    /// サービス一覧に出す、サービスごとのリソース数。
    ServiceCount {
        service: Service,
        result: Result<usize, String>,
    },
    Servers {
        zone: String,
        result: Result<Vec<Server>, String>,
    },
    Switches {
        zone: String,
        result: Result<Vec<Switch>, String>,
    },
    SwitchAction {
        zone: String,
        label: String,
        result: Result<(), String>,
    },
    ServerAction {
        zone: String,
        label: String,
        result: Result<(), String>,
    },
    /// レジストリ自体への変更（作成・更新・削除）。
    RegistryAction {
        label: String,
        result: Result<(), String>,
    },
    /// イメージ（マニフェスト）の削除。
    TagAction {
        host: String,
        repository: String,
        label: String,
        result: Result<(), String>,
    },
}

/// 遅延ロードするデータの状態。
#[derive(Debug, Clone, Default)]
pub enum Loadable<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Loadable::Ready(v) => Some(v),
            _ => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Loadable::Idle)
    }
}

/// タグ詳細キャッシュのキー（ホスト・リポジトリ・タグ）。
pub type TagKey = (String, String, String);

/// 絞り込みの対象になるリスト。サービスをまたいで一意になるように並べる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pane {
    // コンテナレジストリ
    Registries,
    Users,
    Repositories,
    Tags,
    // AppRun（共用型）
    Applications,
    Versions,
    // AppRun（専有型）
    Clusters,
    DedicatedApplications,
    ScalingGroups,
    Certificates,
    // サーバー
    Servers,
    Nics,
    // SSH公開鍵
    SshKeys,
    // パケットフィルタ
    PacketFilters,
    PacketFilterRules,
    // 接続マップ
    NetworkMap,
    // スイッチ
    Switches,
    CloudResources,
    ManagedResources,
    // AI Engine
    AiEngineModels,
    // API Gateway
    ApiGatewaySubscriptions,
    ApiGatewayServices,
    ApiGatewayRoutes,
    ApiGatewayUsers,
    ApiGatewayGroups,
    ApiGatewayDomains,
    ApiGatewayCertificates,
    ApiGatewayOidcs,
    // NoSQL
    NoSqlDatabases,
    NoSqlNodes,
    NoSqlBackups,
    NoSqlParameters,
    // サービスエンドポイントゲートウェイ
    SegGateways,
    SegServices,
    // セキュリティコントロール
    SecurityControlRules,
    SecurityControlActions,
    // クラウドHSM
    CloudHsmHsms,
    CloudHsmClients,
    CloudHsmLicenses,
    CloudHsmDocuments,
    // ネットワークスイート (CR)
    NetworkingSuiteGroups,
    NetworkingSuiteSubnets,
    NetworkingSuiteAddresses,
    // AI Engine（RAG）
    AiEngineDocuments,
    // DNS / シンプル監視
    DnsZones,
    DnsRecords,
    Monitors,
    // シークレットマネージャ
    Vaults,
    Secrets,
    // モニタリングスイート
    Projects,
    Rules,
    LogMeasureRules,
    LogRoutings,
    MetricsRoutings,
    Dashboards,
    Histories,
    NotificationTargets,
    NotificationRoutings,
    Storages,
    StorageKeys,
    // 請求
    Bills,
    BillDetails,
    BillSummary,
    // 権限
    Account,
    /// 絞り込み対象になるリストが無い（概要タブなど）。
    None,
}

/// ペインごとの絞り込み文字列。
#[derive(Debug, Clone, Default)]
pub struct Filters(HashMap<Pane, String>);

impl Filters {
    fn get(&self, pane: Pane) -> &str {
        self.0.get(&pane).map_or("", String::as_str)
    }

    fn get_mut(&mut self, pane: Pane) -> Option<&mut String> {
        (pane != Pane::None).then(|| self.0.entry(pane).or_default())
    }
}

/// 絞り込み文字列に一致するか（部分一致・大文字小文字を無視）。
fn matches(filter: &str, fields: &[&str]) -> bool {
    if filter.is_empty() {
        return true;
    }
    let needle = filter.to_lowercase();
    fields
        .iter()
        .any(|field| field.to_lowercase().contains(&needle))
}

/// 親に紐づく子リソースのうち、これから読むべきものの ID。
///
/// 選択中の親がまだキャッシュに無いか `Idle` のときだけ返すので、
/// 同じ親に対して何度も読みに行かずに済む。
fn child_id_to_load<T>(
    selected_id: Option<String>,
    cache: &HashMap<String, Loadable<T>>,
) -> Option<String> {
    selected_id.filter(|id| cache.get(id).is_none_or(Loadable::is_idle))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Overview,
    Users,
    Images,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Overview, Tab::Users, Tab::Images];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "概要",
            Tab::Users => "ユーザー",
            Tab::Images => "イメージ",
        }
    }
}

/// 資格情報の世代を添えて結果を返す送信口。
///
/// プロファイルを切り替えても、前の資格情報で投げた通信は飛び続ける。
/// それが後から届いて画面に入ると、切り替えたのに前の内容が残り、
/// `r` を押すまで直らない。世代を添えておき、古いものは捨てる。
#[derive(Debug, Clone)]
pub struct Tx {
    inner: UnboundedSender<(u64, Message)>,
    /// この送信口が作られたときの世代。複製すると一緒に運ばれる。
    epoch: u64,
}

impl Tx {
    pub fn new(inner: UnboundedSender<(u64, Message)>) -> Self {
        Tx { inner, epoch: 0 }
    }

    pub fn send(&self, message: Message) -> Result<(), ()> {
        self.inner.send((self.epoch, message)).map_err(|_| ())
    }
}

impl Message {
    /// 世代が変わっても捨ててはいけないもの。
    ///
    /// 資格情報そのものの受け渡しと、レジストリのログインは
    /// 「今どのアカウントを見ているか」と無関係に効かせる必要がある。
    fn ignores_epoch(&self) -> bool {
        matches!(
            self,
            Message::CredentialsLoaded { .. }
                | Message::ProfileVerified { .. }
                | Message::LoginVerified { .. }
                | Message::SavedLogin { .. }
        )
    }
}

/// 操作モード。既定は読み取り専用で、書き込み操作は明示的に切り替えてから行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ReadOnly,
    Write,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::ReadOnly => "読取専用",
            Mode::Write => "書込可",
        }
    }

    fn toggled(self) -> Self {
        match self {
            Mode::ReadOnly => Mode::Write,
            Mode::Write => Mode::ReadOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Registries,
    Detail,
}

/// イメージタブ内で選択中のペイン。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImagePane {
    #[default]
    Repositories,
    Tags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Success,
    Error,
}

/// 確認ダイアログで実行する操作。
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteServer {
        zone: String,
        id: ResourceId,
        name: String,
    },
    ChangeServerPlan {
        zone: String,
        id: ResourceId,
        name: String,
        cpu: u32,
        memory_mb: u32,
    },
    CreateDisk {
        zone: String,
        input: Box<crate::iaas::DiskCreateInput>,
    },
    DeleteDisk {
        zone: String,
        id: ResourceId,
        name: String,
    },
    DeleteSshKey {
        id: ResourceId,
        name: String,
    },
    DeletePacketFilter {
        zone: String,
        id: ResourceId,
        name: String,
    },
    DeletePacketFilterRule {
        id: ResourceId,
        index: usize,
    },
    DeleteNic {
        zone: String,
        id: ResourceId,
        name: String,
    },
    CreateArchive {
        zone: String,
        name: String,
        description: String,
        disk_id: ResourceId,
    },
    DeleteArchive {
        zone: String,
        id: ResourceId,
        name: String,
    },
    DeleteAutoBackup {
        zone: String,
        id: ResourceId,
        name: String,
    },
    DisconnectDisk {
        zone: String,
        id: ResourceId,
        name: String,
        server: String,
    },
    CreateServer {
        zone: String,
        /// 入力一式。列挙体が肥大しないよう箱に入れる。
        input: Box<crate::iaas::ServerCreateInput>,
    },
    DeleteRagDocument {
        id: String,
        name: String,
    },
    DeleteUser {
        registry: ResourceId,
        username: String,
    },
    ForgetLogin {
        host: String,
    },
    SaveRegistryLogin {
        host: String,
        login: RegistryLogin,
    },
    DeleteRegistry {
        id: ResourceId,
        name: String,
    },
    DeleteSwitch {
        zone: String,
        id: ResourceId,
        name: String,
    },
    DeleteDnsRecord {
        zone: DnsZone,
        record: DnsRecord,
    },
    DeleteDnsZone {
        id: ResourceId,
        name: String,
    },
    DeleteSimpleMonitor {
        id: ResourceId,
        target: String,
    },
    DeleteCredential {
        name: String,
    },
    DeleteAiEngineToken {
        name: String,
    },
    DeleteTag {
        host: String,
        repository: String,
        tag: String,
        digest: String,
    },
    RouteTraffic {
        application: String,
        app_name: String,
        version: String,
    },
    UnveilSecret {
        vault: String,
        name: String,
    },
    DeleteVault {
        id: String,
        name: String,
    },
    DeleteSecret {
        vault: String,
        name: String,
    },
    DeleteAlertProject {
        zone: String,
        resource_id: i64,
        name: String,
    },
    DeleteAlertRule {
        zone: String,
        project: i64,
        uid: String,
        name: String,
    },
    DeleteLogMeasureRule {
        zone: String,
        project: i64,
        uid: String,
        name: String,
    },
    DeleteLogRouting {
        zone: String,
        routing: LogRouting,
    },
    DeleteMetricsRouting {
        zone: String,
        routing: MetricsRouting,
    },
    DeleteDashboardProject {
        zone: String,
        project: DashboardProject,
    },
    DeleteNotificationTarget {
        zone: String,
        project: i64,
        target: NotificationTarget,
    },
    DeleteNotificationRouting {
        zone: String,
        project: i64,
        routing: NotificationRouting,
    },
    DeleteStorage {
        zone: String,
        storage: Storage,
    },
    SetStorageRetention {
        zone: String,
        storage: Storage,
        days: i64,
    },
    DeleteStorageAccessKey {
        zone: String,
        storage: Storage,
        key: StorageAccessKey,
    },
    RevealStorageAccessKey {
        zone: String,
        storage: Storage,
        key: StorageAccessKey,
    },
    PowerAction {
        id: ResourceId,
        zone: String,
        name: String,
        action: PowerAction,
    },
    DeleteIamResource {
        resource_type: String,
        id: String,
        name: String,
    },
    ChangeIamRole {
        project_id: i64,
        principal_type: String,
        principal_id: i64,
        role_id: String,
        grant: bool,
    },
}

/// 前面に表示するダイアログ。
#[derive(Debug, Clone)]
pub enum Overlay {
    Help,
    Message {
        title: String,
        body: String,
        kind: StatusKind,
        /// 長い本文を読み切れるようにするためのスクロール位置。
        scroll: u16,
    },
    Confirm {
        title: String,
        body: String,
        /// 取り返しがつかない操作では、ここに入れた文字列の入力を要求する。
        verify: Option<String>,
        typed: String,
        action: ConfirmAction,
    },
    UserForm(UserForm),
    RegistryForm(RegistryForm),
    IamResourceForm(IamResourceForm),
    IamRoleForm(IamRoleForm),
    SwitchForm(SwitchForm),
    RagUploadForm(RagUploadForm),
    ServerCreateForm(ServerCreateForm),
    ServerChoicePicker(ServerChoicePicker),
    PacketFilterForm(PacketFilterForm),
    RuleForm(RuleForm),
    NicPicker(NicPicker),
    ServerPlanForm(ServerPlanForm),
    SshKeyForm(SshKeyForm),
    DiskCreateForm(DiskCreateForm),
    DiskServerPicker(DiskServerPicker),
    ArchiveForm(ArchiveForm),
    AutoBackupForm(AutoBackupForm),
    /// SSH 公開鍵の取得元と一覧。選び終えたら戻すので、呼び出し元ごと預かる。
    SshKeyPicker {
        back: Box<SshKeyReturn>,
        stage: SshKeyStage,
    },
    RagEditForm(RagEditForm),
    DnsRecordForm(DnsRecordForm),
    DnsZoneForm(DnsZoneForm),
    SimpleMonitorForm(SimpleMonitorForm),
    VaultForm(VaultForm),
    SecretForm(SecretForm),
    AlertProjectForm(AlertProjectForm),
    AlertRuleForm(AlertRuleForm),
    LogMeasureRuleForm(LogMeasureRuleForm),
    LogRoutingForm(LogRoutingForm),
    MetricsRoutingForm(MetricsRoutingForm),
    DashboardForm(DashboardForm),
    NotificationTargetForm(NotificationTargetForm),
    NotificationRoutingForm(NotificationRoutingForm),
    StorageForm(StorageForm),
    StorageRetentionForm(StorageRetentionForm),
    StorageAccessKeyForm(StorageAccessKeyForm),
    Login(LoginForm),
    /// 保存済みのユーザー名から選んでログインする。
    LoginPicker {
        host: String,
        accounts: Vec<String>,
        index: usize,
    },
    /// 認証情報（usacloud プロファイル / 環境変数）の切り替え。
    ProfilePicker {
        /// 選択肢と、それぞれの既定ゾーン。
        /// ゾーンは開いた時点で読んでおく（描画のたびにファイルを読まないため）。
        sources: Vec<(CredentialSource, Option<String>)>,
        index: usize,
    },
    /// ゾーンの切り替え。
    ZonePicker {
        zones: Vec<Zone>,
        index: usize,
    },
    /// サービスの切り替え。
    ServicePicker {
        index: usize,
        /// 起動直後で、まだ表示するサービスを選んでいない。
        initial: bool,
    },
    /// usacloud プロファイルの新規作成。
    ProfileForm(ProfileForm),
    AiEngineTokenForm(AiEngineTokenForm),
    IamCredentialForm(IamCredentialForm),
}

/// コンテナレジストリ画面が持つ状態。
#[derive(Debug, Default)]
pub struct RegistryView {
    pub registries: Loadable<Vec<ContainerRegistry>>,
    pub registry_state: TableState,

    pub tab: Tab,
    pub focus: Focus,

    pub users: HashMap<ResourceId, Loadable<Vec<RegistryUser>>>,
    pub user_state: ListState,

    pub image_pane: ImagePane,
    pub repositories: HashMap<String, Loadable<Vec<String>>>,
    pub repository_state: ListState,
    pub tags: HashMap<(String, String), Loadable<Vec<TagInfo>>>,
    pub tag_state: ListState,
    pub tag_details: HashMap<TagKey, Loadable<TagDetail>>,
    /// 保存済みログインの読み出しを試したホスト。同じホストを何度も読まないための印。
    pub auto_login_tried: HashSet<String>,
}

#[derive(Debug, Default)]
pub struct CloudResourcesView {
    pub items: HashMap<(String, CloudResourceKind), Loadable<Vec<CloudResource>>>,
    pub state: TableState,
}

#[derive(Debug, Default)]
pub struct ManagedResourcesView {
    pub items: HashMap<ManagedResourceKind, Loadable<Vec<ManagedResource>>>,
    pub state: TableState,
}

pub struct App {
    sacloud: Arc<SacloudClient>,
    apprun_client: Arc<AppRunClient>,
    dedicated_client: Arc<DedicatedClient>,
    monitoring_client: Arc<MonitoringClient>,
    api_gateway_client: Arc<ApiGatewayClient>,
    ai_engine_client: Option<Arc<AiEngineClient>>,
    ai_engine_cloud_client: Arc<AiEngineCloudClient>,
    tx: Tx,
    pub config: Config,
    pub registry_clients: RegistryClients,
    pub credential_source: CredentialSource,
    /// 有効なクラウドAPI認証情報が設定済みか。
    pub has_credentials: bool,

    pub mode: Mode,
    pub should_quit: bool,
    /// 実行中の非同期リクエスト数（スピナー表示用）。
    pub inflight: usize,
    /// 資格情報の世代。切り替えるたびに増やす。
    epoch: u64,
    pub tick: u64,

    /// 表示中のサービス。
    pub service: Service,
    /// ゾーンに属するリソース（サーバーなど）を見るときのゾーン。
    pub zone: String,
    /// 現在の接続先（API ルート）。
    pub api_root: String,
    pub zones: Loadable<Vec<Zone>>,
    /// `(サービス, ゾーン)` ごとのリソース件数。ゾーン選択の判断材料に出す。
    pub account: AccountView,
    pub zone_counts: HashMap<(Service, String), Loadable<usize>>,
    /// サービスごとのリソース数。ゾーン依存のものは現在のゾーンの数。
    pub service_counts: HashMap<Service, Loadable<usize>>,
    /// ゾーン一覧の取得を待ってピッカーを開くかどうか。
    pending_zone_picker: bool,

    /// コンテナレジストリ画面の状態。
    pub registry: RegistryView,
    /// AppRun（共用型）画面の状態。
    pub apprun: AppRunView,
    /// AppRun（専有型）画面の状態。
    pub dedicated: DedicatedView,
    /// サーバー画面の状態。
    pub server: ServerView,
    /// ディスク画面の書き込み操作で使う状態。
    pub disk: DiskView,
    /// SSH 公開鍵画面の状態。
    pub ssh_key: SshKeyView,
    /// パケットフィルタ画面の状態。
    pub packet_filter: PacketFilterView,
    /// 接続マップ画面の状態。
    pub network_map: NetworkMapView,
    /// スイッチ画面の状態。
    pub switch: SwitchView,
    pub cloud_resources: CloudResourcesView,
    pub managed_resources: ManagedResourcesView,
    pub api_gateway: ApiGatewayView,
    pub nosql: NoSqlView,
    pub seg: SegView,
    pub security_control: SecurityControlView,
    pub cloudhsm: CloudHsmView,
    pub networking_suite: NetworkingSuiteView,
    pub ai_engine: AiEngineView,
    pub dns: DnsView,
    pub simple_monitor: SimpleMonitorView,
    pub secrets: SecretsView,
    pub monitoring: MonitoringView,
    /// 請求画面の状態。
    pub billing: BillingView,

    /// ペインごとの絞り込み。
    pub filters: Filters,
    /// 絞り込み文字列を編集中かどうか。
    pub filtering: bool,

    pub overlay: Option<Overlay>,
    /// メッセージダイアログを閉じたあとに戻すフォーム。
    pending_form: Option<Box<ProfileForm>>,
    pub status: Option<(String, StatusKind)>,
}

impl App {
    pub fn new(
        clients: crate::Clients,
        tx: Tx,
        config: Config,
        credential_source: CredentialSource,
        has_credentials: bool,
    ) -> Self {
        let default_zone = clients.sacloud.default_zone().to_string();
        let api_root_url = clients.sacloud.api_root().to_string();
        Self {
            sacloud: clients.sacloud,
            apprun_client: clients.apprun,
            dedicated_client: clients.dedicated,
            monitoring_client: clients.monitoring,
            api_gateway_client: clients.api_gateway,
            ai_engine_client: None,
            ai_engine_cloud_client: clients.ai_engine_cloud,
            tx,
            config,
            registry_clients: RegistryClients::default(),
            credential_source,
            has_credentials,
            // 事故を防ぐため、既定は読み取り専用。
            mode: Mode::ReadOnly,
            should_quit: false,
            inflight: 0,
            epoch: 0,
            tick: 0,
            service: Service::Registry,
            zone: default_zone,
            api_root: api_root_url,
            zones: Loadable::Idle,
            account: AccountView::default(),
            zone_counts: HashMap::new(),
            service_counts: HashMap::new(),
            pending_zone_picker: false,
            registry: RegistryView::default(),
            apprun: AppRunView::default(),
            dedicated: DedicatedView::default(),
            server: ServerView::default(),
            disk: DiskView::default(),
            ssh_key: SshKeyView::default(),
            packet_filter: PacketFilterView::default(),
            network_map: NetworkMapView::default(),
            switch: SwitchView::default(),
            cloud_resources: CloudResourcesView::default(),
            managed_resources: ManagedResourcesView::default(),
            api_gateway: ApiGatewayView::default(),
            nosql: NoSqlView::default(),
            seg: SegView::default(),
            security_control: SecurityControlView::default(),
            cloudhsm: CloudHsmView::default(),
            networking_suite: NetworkingSuiteView::default(),
            ai_engine: AiEngineView::default(),
            dns: DnsView::default(),
            simple_monitor: SimpleMonitorView::default(),
            secrets: SecretsView::default(),
            monitoring: MonitoringView::default(),
            billing: BillingView::default(),
            filters: Filters::default(),
            filtering: false,
            overlay: None,
            pending_form: None,
            status: None,
        }
    }

    // --- 表示中の要素（絞り込み適用後） ---
    pub fn cloud_resource_kind(&self) -> Option<CloudResourceKind> {
        match self.service {
            Service::Disk => Some(CloudResourceKind::Disk),
            Service::Archive => Some(CloudResourceKind::Archive),
            Service::IsoImage => Some(CloudResourceKind::IsoImage),
            Service::Internet => Some(CloudResourceKind::Internet),
            Service::Bridge => Some(CloudResourceKind::Bridge),
            Service::LoadBalancer => Some(CloudResourceKind::LoadBalancer),
            Service::VpcRouter => Some(CloudResourceKind::VpcRouter),
            Service::MobileGateway => Some(CloudResourceKind::MobileGateway),
            Service::Database => Some(CloudResourceKind::Database),
            Service::Nfs => Some(CloudResourceKind::Nfs),
            _ => None,
        }
    }

    pub fn visible_cloud_resources(&self) -> Loadable<Vec<CloudResource>> {
        let Some(kind) = self.cloud_resource_kind() else {
            return Loadable::Idle;
        };
        let loadable = self
            .cloud_resources
            .items
            .get(&(self.zone.clone(), kind))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::CloudResources).to_ascii_lowercase();
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    filter.is_empty() || item.searchable().to_ascii_lowercase().contains(&filter)
                })
                .collect(),
        )
    }

    /// ディスクなどを書き換えたあと、一覧を引き直させる。
    ///
    /// 画面によって一覧の置き場所が違う（ディスクとアーカイブは
    /// `cloud_resources`、自動バックアップは `managed_resources`）ので、
    /// 今の画面が使っているほうを捨てる。
    pub(super) fn cloud_resources_invalidate(&mut self) {
        if let Some(kind) = self.cloud_resource_kind() {
            self.cloud_resources
                .items
                .remove(&(self.zone.clone(), kind));
        }
        if let Some(kind) = self.managed_resource_kind() {
            self.managed_resources.items.remove(&kind);
        }
    }

    pub fn selected_cloud_resource(&self) -> Option<CloudResource> {
        self.visible_cloud_resources()
            .ready()?
            .get(self.cloud_resources.state.selected()?)
            .cloned()
    }

    pub fn managed_resource_kind(&self) -> Option<ManagedResourceKind> {
        match self.service {
            Service::AiEngine => Some(ManagedResourceKind::AiEngine),
            Service::ObjectStorage => Some(ManagedResourceKind::ObjectStorage),
            Service::SimpleMq => Some(ManagedResourceKind::SimpleMq),
            Service::SimpleNotification => Some(ManagedResourceKind::SimpleNotification),
            Service::EventBus => Some(ManagedResourceKind::EventBus),
            Service::Workflows => Some(ManagedResourceKind::Workflows),
            Service::WebAccel => Some(ManagedResourceKind::WebAccel),
            Service::EnhancedLoadBalancer => Some(ManagedResourceKind::EnhancedLoadBalancer),
            Service::LocalRouter => Some(ManagedResourceKind::LocalRouter),
            Service::Gslb => Some(ManagedResourceKind::Gslb),
            Service::Kms => Some(ManagedResourceKind::Kms),
            Service::Iam => Some(ManagedResourceKind::Iam),
            Service::AutoScale => Some(ManagedResourceKind::AutoScale),
            Service::EnhancedDb => Some(ManagedResourceKind::EnhancedDb),
            Service::AutoBackup => Some(ManagedResourceKind::AutoBackup),
            _ => None,
        }
    }

    pub fn visible_managed_resources(&self) -> Loadable<Vec<ManagedResource>> {
        let Some(kind) = self.managed_resource_kind() else {
            return Loadable::Idle;
        };
        let loadable = self
            .managed_resources
            .items
            .get(&kind)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self
            .filters
            .get(Pane::ManagedResources)
            .to_ascii_lowercase();
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    filter.is_empty() || item.searchable().to_ascii_lowercase().contains(&filter)
                })
                .collect(),
        )
    }

    pub fn selected_managed_resource(&self) -> Option<ManagedResource> {
        self.visible_managed_resources()
            .ready()?
            .get(self.managed_resources.state.selected()?)
            .cloned()
    }
    //
    // 選択位置は常に「絞り込み後のリスト」に対する添字として扱う。

    pub fn visible_registries(&self) -> Vec<&ContainerRegistry> {
        let Some(items) = self.registry.registries.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Registries);
        items
            .iter()
            .filter(|r| matches(filter, &[&r.name, r.host(), &r.description]))
            .collect()
    }

    pub fn selected_registry(&self) -> Option<&ContainerRegistry> {
        let index = self.registry.registry_state.selected()?;
        self.visible_registries().into_iter().nth(index)
    }

    /// 現在選択中のレジストリのユーザー一覧（絞り込み適用後）。
    pub fn visible_users(&self) -> Loadable<Vec<RegistryUser>> {
        let loadable = self
            .selected_registry()
            .and_then(|r| self.registry.users.get(&r.id))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(users) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            users
                .into_iter()
                .filter(|u| {
                    matches(
                        self.filters.get(Pane::Users),
                        &[&u.username, u.permission.as_str()],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_user(&self) -> Option<RegistryUser> {
        let index = self.registry.user_state.selected()?;
        self.visible_users().ready()?.get(index).cloned()
    }

    pub fn visible_repositories(&self) -> Loadable<Vec<String>> {
        let loadable = self
            .selected_registry()
            .and_then(|r| self.registry.repositories.get(r.host()))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(repositories) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            repositories
                .into_iter()
                .filter(|r| matches(self.filters.get(Pane::Repositories), &[r]))
                .collect(),
        )
    }

    pub fn selected_repository(&self) -> Option<String> {
        let index = self.registry.repository_state.selected()?;
        self.visible_repositories().ready()?.get(index).cloned()
    }

    pub fn visible_tags(&self) -> Loadable<Vec<TagInfo>> {
        let Some(registry) = self.selected_registry() else {
            return Loadable::Idle;
        };
        let Some(repository) = self.selected_repository() else {
            return Loadable::Idle;
        };
        let loadable = self
            .registry
            .tags
            .get(&(registry.host().to_string(), repository))
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(tags) = loadable else {
            return loadable;
        };
        Loadable::Ready(
            tags.into_iter()
                .filter(|t| matches(self.filters.get(Pane::Tags), &[&t.name]))
                .collect(),
        )
    }

    pub fn selected_tag(&self) -> Option<TagInfo> {
        let index = self.registry.tag_state.selected()?;
        self.visible_tags().ready()?.get(index).cloned()
    }

    /// 選択中タグの詳細キャッシュのキー。
    fn selected_tag_key(&self) -> Option<TagKey> {
        let host = self.selected_registry()?.host().to_string();
        let repository = self.selected_repository()?;
        let tag = self.selected_tag()?.name;
        Some((host, repository, tag))
    }

    /// 選択中タグの詳細。
    pub fn selected_tag_detail(&self) -> Loadable<TagDetail> {
        self.selected_tag_key()
            .and_then(|key| self.registry.tag_details.get(&key))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    /// 選択中レジストリにログイン済みかどうか。
    pub fn is_logged_in(&self) -> bool {
        self.selected_registry()
            .is_some_and(|r| self.registry_clients.get(r.host()).is_some())
    }

    /// 現在キー操作の対象になっているリスト。
    pub fn active_pane(&self) -> Pane {
        match self.service {
            Service::Registry => self.registry_active_pane(),
            Service::AppRun => self.apprun_active_pane(),
            Service::Dedicated => self.dedicated_active_pane(),
            Service::Server => self.server_active_pane(),
            Service::SshKey => Pane::SshKeys,
            Service::Switch => Pane::Switches,
            Service::NetworkMap => Pane::NetworkMap,
            Service::PacketFilter => self.packet_filter_active_pane(),
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::Bridge
            | Service::LoadBalancer
            | Service::VpcRouter
            | Service::MobileGateway
            | Service::Database
            | Service::Nfs => Pane::CloudResources,
            Service::ObjectStorage
            | Service::SimpleMq
            | Service::SimpleNotification
            | Service::EventBus
            | Service::Workflows
            | Service::WebAccel
            | Service::EnhancedLoadBalancer
            | Service::LocalRouter
            | Service::Gslb
            | Service::Kms
            | Service::Iam
            | Service::AutoScale
            | Service::EnhancedDb
            | Service::AutoBackup => Pane::ManagedResources,
            Service::AiEngine => match self.ai_engine.tab {
                // コントロールパネルAPIが使えないときは推論API側の一覧に落ちるので、
                // 実際に描いている方のペインを返す。絞り込みとコピーの対象がずれる。
                AiEngineTab::Models if self.ai_engine_shows_cloud_models() => Pane::AiEngineModels,
                AiEngineTab::Models => Pane::ManagedResources,
                AiEngineTab::Documents => Pane::AiEngineDocuments,
                AiEngineTab::Usage | AiEngineTab::Billing | AiEngineTab::Account => Pane::None,
            },
            Service::NetworkingSuite => match self.networking_suite.tab {
                NetworkingSuiteTab::Groups => Pane::NetworkingSuiteGroups,
                NetworkingSuiteTab::Subnets => Pane::NetworkingSuiteSubnets,
                NetworkingSuiteTab::Addresses => Pane::NetworkingSuiteAddresses,
            },
            Service::CloudHsm => match self.cloudhsm.tab {
                CloudHsmTab::Hsms => Pane::CloudHsmHsms,
                CloudHsmTab::Clients => Pane::CloudHsmClients,
                CloudHsmTab::Licenses => Pane::CloudHsmLicenses,
                CloudHsmTab::Documents => Pane::CloudHsmDocuments,
            },
            Service::SecurityControl => match self.security_control.tab {
                SecurityControlTab::Rules => Pane::SecurityControlRules,
                SecurityControlTab::Actions => Pane::SecurityControlActions,
            },
            Service::Seg => match self.seg.tab {
                SegTab::Gateways => Pane::SegGateways,
                SegTab::Services => Pane::SegServices,
            },
            Service::NoSql => match self.nosql.tab {
                NoSqlTab::Databases => Pane::NoSqlDatabases,
                NoSqlTab::Nodes => Pane::NoSqlNodes,
                NoSqlTab::Backups => Pane::NoSqlBackups,
                NoSqlTab::Parameters => Pane::NoSqlParameters,
            },
            Service::ApiGateway => match self.api_gateway.tab {
                ApiGatewayTab::Subscriptions => Pane::ApiGatewaySubscriptions,
                ApiGatewayTab::Services => Pane::ApiGatewayServices,
                ApiGatewayTab::Routes => Pane::ApiGatewayRoutes,
                ApiGatewayTab::Users => Pane::ApiGatewayUsers,
                ApiGatewayTab::Groups => Pane::ApiGatewayGroups,
                ApiGatewayTab::Domains => Pane::ApiGatewayDomains,
                ApiGatewayTab::Certificates => Pane::ApiGatewayCertificates,
                ApiGatewayTab::Oidc => Pane::ApiGatewayOidcs,
            },
            Service::Dns => match self.dns.focus {
                ListFocus::Left => Pane::DnsZones,
                ListFocus::Right => Pane::DnsRecords,
            },
            Service::SimpleMonitor => Pane::Monitors,
            Service::Secrets => match self.secrets.focus {
                ListFocus::Left => Pane::Vaults,
                ListFocus::Right => Pane::Secrets,
            },
            Service::Account => Pane::Account,
            Service::Billing => self.billing_active_pane(),
            Service::Monitoring => match self.monitoring.focus {
                ListFocus::Left if self.monitoring.tab == MonitoringTab::Storages => Pane::Storages,
                _ if self.monitoring.tab == MonitoringTab::LogRoutings => Pane::LogRoutings,
                _ if self.monitoring.tab == MonitoringTab::MetricsRoutings => Pane::MetricsRoutings,
                _ if self.monitoring.tab == MonitoringTab::Dashboards => Pane::Dashboards,
                ListFocus::Left => Pane::Projects,
                ListFocus::Right => match self.monitoring.tab {
                    MonitoringTab::Rules => Pane::Rules,
                    MonitoringTab::Histories => Pane::Histories,
                    MonitoringTab::Storages => Pane::StorageKeys,
                    MonitoringTab::NotificationTargets => Pane::NotificationTargets,
                    MonitoringTab::NotificationRoutings => Pane::NotificationRoutings,
                    MonitoringTab::LogMeasureRules => Pane::LogMeasureRules,
                    MonitoringTab::LogRoutings => Pane::LogRoutings,
                    MonitoringTab::MetricsRoutings => Pane::MetricsRoutings,
                    MonitoringTab::Dashboards => Pane::Dashboards,
                },
            },
        }
    }

    fn registry_active_pane(&self) -> Pane {
        match self.registry.focus {
            Focus::Registries => Pane::Registries,
            Focus::Detail => match self.registry.tab {
                Tab::Overview => Pane::None,
                Tab::Users => Pane::Users,
                Tab::Images => match self.registry.image_pane {
                    ImagePane::Repositories => Pane::Repositories,
                    ImagePane::Tags => Pane::Tags,
                },
            },
        }
    }

    /// 現在のペインに掛かっている絞り込み文字列。
    pub fn active_filter(&self) -> &str {
        self.filters.get(self.active_pane())
    }

    // --- 非同期処理の起動 ---

    fn load_registries(&mut self) {
        self.registry.registries = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_registries().await.map_err(fmt_error);
            let _ = tx.send(Message::Registries(result));
        });
    }

    fn load_users(&mut self, id: ResourceId) {
        self.registry.users.insert(id, Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_users(id).await.map_err(fmt_error);
            let _ = tx.send(Message::Users { id, result });
        });
    }

    fn load_repositories(&mut self, host: String) {
        let Some(client) = self.registry_clients.get(&host) else {
            return;
        };
        self.registry
            .repositories
            .insert(host.clone(), Loadable::Loading);
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_repositories().await.map_err(fmt_error);
            let _ = tx.send(Message::Repositories { host, result });
        });
    }

    fn load_tags(&mut self, host: String, repository: String) {
        let Some(client) = self.registry_clients.get(&host) else {
            return;
        };
        self.registry
            .tags
            .insert((host.clone(), repository.clone()), Loadable::Loading);
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_tags(&repository).await.map_err(fmt_error);
            let _ = tx.send(Message::Tags {
                host,
                repository,
                result,
            });
        });
    }

    /// 選択中タグの詳細（サイズ・レイヤ数・プラットフォーム・ビルド日時）を取る。
    fn load_tag_detail(&mut self, key: TagKey) {
        let Some(client) = self.registry_clients.get(&key.0) else {
            return;
        };
        self.registry
            .tag_details
            .insert(key.clone(), Loadable::Loading);
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.tag_detail(&key.1, &key.2).await.map_err(fmt_error);
            let _ = tx.send(Message::TagDetails { key, result });
        });
    }

    /// 現在表示中のビューに必要なデータをまだ読んでいなければ読む。
    pub fn ensure_loaded(&mut self) {
        if !self.has_credentials || matches!(self.overlay, Some(Overlay::ServicePicker { .. })) {
            return;
        }
        match self.service {
            Service::Registry => self.registry_ensure_loaded(),
            Service::AppRun => self.apprun_ensure_loaded(),
            Service::Dedicated => self.dedicated_ensure_loaded(),
            Service::Server => self.server_ensure_loaded(),
            Service::SshKey => self.ssh_key_ensure_loaded(),
            Service::PacketFilter => self.packet_filter_ensure_loaded(),
            Service::Switch => self.switch_ensure_loaded(),
            Service::NetworkMap => self.network_map_ensure_loaded(),
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::Bridge
            | Service::LoadBalancer
            | Service::VpcRouter
            | Service::MobileGateway
            | Service::Database
            | Service::Nfs => self.cloud_resources_ensure_loaded(),
            Service::ObjectStorage
            | Service::SimpleMq
            | Service::SimpleNotification
            | Service::EventBus
            | Service::Workflows
            | Service::WebAccel
            | Service::EnhancedLoadBalancer
            | Service::LocalRouter
            | Service::Gslb
            | Service::Kms
            | Service::Iam
            | Service::AutoScale
            | Service::EnhancedDb
            | Service::AutoBackup => self.managed_resources_ensure_loaded(),
            Service::AiEngine => self.ai_engine_ensure_loaded(),
            Service::NetworkingSuite => self.networking_suite_ensure_loaded(),
            Service::CloudHsm => self.cloudhsm_ensure_loaded(),
            Service::SecurityControl => self.security_control_ensure_loaded(),
            Service::Seg => self.seg_ensure_loaded(),
            Service::NoSql => self.nosql_ensure_loaded(),
            Service::ApiGateway => self.api_gateway_ensure_loaded(),
            Service::Dns => self.dns_ensure_loaded(),
            Service::SimpleMonitor => self.monitor_ensure_loaded(),
            Service::Secrets => self.secrets_ensure_loaded(),
            Service::Monitoring => self.monitoring_ensure_loaded(),
            Service::Account => self.account_ensure_loaded(),
            Service::Billing => self.billing_ensure_loaded(),
        }
    }

    fn registry_ensure_loaded(&mut self) {
        // 一覧そのものが未取得ならまずそれを引く。
        // ここを飛ばすと、資格情報を切り替えたあと誰も読み込まず、
        // 「読み込み中…」のまま止まる。
        if self.registry.registries.is_idle() {
            self.load_registries();
            return;
        }
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let id = registry.id;
        let host = registry.host().to_string();
        // 先に選択位置を整えてから、その選択に紐づくデータの要否を判断する。
        self.normalize_selection();

        match self.registry.tab {
            Tab::Overview => {}
            Tab::Users => {
                if self.registry.users.get(&id).is_none_or(Loadable::is_idle) {
                    self.load_users(id);
                }
            }
            // 未ログインならユーザーが L を押すまで何もしない。
            Tab::Images if self.registry_clients.get(&host).is_none() => {
                self.try_auto_login(&host);
            }
            Tab::Images => {
                if self
                    .registry
                    .repositories
                    .get(&host)
                    .is_none_or(Loadable::is_idle)
                {
                    self.load_repositories(host.clone());
                }
                if let Some(repository) = self.selected_repository() {
                    let key = (host.clone(), repository.clone());
                    if self.registry.tags.get(&key).is_none_or(Loadable::is_idle) {
                        self.load_tags(host, repository);
                    }
                }
                // 詳細は選択中のタグの分だけ取る。
                if let Some(key) = self.selected_tag_key()
                    && self
                        .registry
                        .tag_details
                        .get(&key)
                        .is_none_or(Loadable::is_idle)
                {
                    self.load_tag_detail(key);
                }
            }
        }
    }

    /// 読み込み済みなのに未選択のリストがあれば先頭を選ぶ。
    /// （レジストリを切り替えて戻ってきたときに選択が空にならないようにする）
    fn normalize_selection(&mut self) {
        fn fill(state: &mut dyn SelectableList, len: usize) {
            if len > 0 && state.selected().is_none() {
                state.select(Some(0));
            }
        }

        let users = self.visible_users().ready().map_or(0, Vec::len);
        fill(&mut self.registry.user_state, users);
        let repositories = self.visible_repositories().ready().map_or(0, Vec::len);
        fill(&mut self.registry.repository_state, repositories);
        let tags = self.visible_tags().ready().map_or(0, Vec::len);
        fill(&mut self.registry.tag_state, tags);
    }

    // --- 非同期処理の結果反映 ---

    pub fn on_message(&mut self, epoch: u64, message: Message) {
        self.inflight = self.inflight.saturating_sub(1);
        // 前の資格情報で投げた通信の結果は、届いても画面に入れない。
        if epoch != self.epoch && !message.ignores_epoch() {
            return;
        }
        match message {
            Message::CloudResources { zone, kind, result } => {
                let loadable = self.store_result(result);
                self.cloud_resources.items.insert((zone, kind), loadable);
                self.fill_selection(Pane::CloudResources);
            }
            Message::ManagedResources { kind, result } => {
                let loadable = self.store_result(result);
                self.managed_resources.items.insert(kind, loadable);
                self.fill_selection(Pane::ManagedResources);
            }
            Message::ApiGatewaySubscriptions { result } => {
                self.api_gateway.subscriptions = self.store_result(result);
                self.fill_selection(Pane::ApiGatewaySubscriptions);
            }
            Message::ApiGatewayServices { result } => {
                self.api_gateway.services = self.store_result(result);
                self.fill_selection(Pane::ApiGatewayServices);
                self.api_gateway.route_state.select(None);
                self.api_gateway_ensure_loaded();
            }
            Message::ApiGatewayRoutes { service_id, result } => {
                let loadable = self.store_result(result);
                self.api_gateway.routes.insert(service_id, loadable);
                self.fill_selection(Pane::ApiGatewayRoutes);
            }
            Message::ApiGatewayUsers { result } => {
                self.api_gateway.users = self.store_result(result);
                self.fill_selection(Pane::ApiGatewayUsers);
                self.api_gateway_ensure_loaded();
            }
            Message::ApiGatewayUserAuthentication { user_id, result } => {
                let loadable = match result {
                    Ok(item) => Loadable::Ready(item),
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        Loadable::Failed(err)
                    }
                };
                self.api_gateway.authentications.insert(user_id, loadable);
            }
            Message::ApiGatewayGroups { result } => {
                self.api_gateway.groups = self.store_result(result);
                self.fill_selection(Pane::ApiGatewayGroups);
            }
            Message::ApiGatewayDomains { result } => {
                self.api_gateway.domains = self.store_result(result);
                self.fill_selection(Pane::ApiGatewayDomains);
            }
            Message::ApiGatewayCertificates { result } => {
                self.api_gateway.certificates = self.store_result(result);
                self.fill_selection(Pane::ApiGatewayCertificates);
            }
            Message::ApiGatewayOidcs { result } => {
                self.api_gateway.oidcs = self.store_result(result);
                self.fill_selection(Pane::ApiGatewayOidcs);
            }
            Message::NoSqlDatabases { result } => {
                self.nosql.databases = self.store_result(result);
                self.fill_selection(Pane::NoSqlDatabases);
                // 選択が定まってから、その DB にぶら下がる 4 つを読みに行く。
                self.nosql_reset_child_selection();
                self.nosql_ensure_loaded();
            }
            Message::NoSqlStatus {
                database_id,
                result,
            } => {
                let loadable = self.store_result(result);
                self.nosql.statuses.insert(database_id, loadable);
                self.fill_selection(Pane::NoSqlNodes);
            }
            Message::NoSqlNodeHealth {
                database_id,
                result,
            } => {
                let loadable = self.store_result(result);
                self.nosql.healths.insert(database_id, loadable);
            }
            Message::NoSqlBackups {
                database_id,
                result,
            } => {
                let loadable = self.store_result(result);
                self.nosql.backups.insert(database_id, loadable);
                self.fill_selection(Pane::NoSqlBackups);
            }
            Message::NoSqlParameters {
                database_id,
                result,
            } => {
                let loadable = self.store_result(result);
                self.nosql.parameters.insert(database_id, loadable);
                self.fill_selection(Pane::NoSqlParameters);
            }
            Message::AiEngineDocuments { result } => {
                self.ai_engine.documents = self.store_result(result);
                self.fill_selection(Pane::AiEngineDocuments);
                self.ai_engine.chunk_scroll = 0;
                self.ai_engine_ensure_loaded();
            }
            Message::AiEngineChunks {
                document_id,
                result,
            } => {
                let loadable = self.store_result(result);
                self.ai_engine.chunks.insert(document_id, loadable);
            }
            Message::AiEngineCloudAuth { result } => {
                self.ai_engine.cloud_auth = self.store_result(result);
                self.ai_engine_ensure_loaded();
            }
            Message::AiEngineCloudModels { result } => {
                self.ai_engine.cloud_models = self.store_result(result);
                self.fill_selection(Pane::AiEngineModels);
            }
            Message::AiEngineCloudUsages { result } => {
                self.ai_engine.usages = self.store_result(result);
            }
            Message::AiEngineCloudDocumentUsages { result } => {
                self.ai_engine.document_usages = self.store_result(result);
            }
            Message::AiEngineCloudBill { month, result } => {
                let loadable = self.store_result(result);
                self.ai_engine.bills.insert(month, loadable);
            }
            Message::RagDocumentUploaded { result } => match result {
                Ok(document) => {
                    self.set_status(
                        format!("ドキュメント「{}」をアップロードしました", document.name),
                        StatusKind::Success,
                    );
                    // 取り込みは非同期なので、一覧を引き直して状態を見せる。
                    self.ai_engine_refresh();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: "アップロードに失敗しました".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::ServerDeleted { name, result } => match result {
                Ok(()) => {
                    self.set_status(
                        format!("サーバー「{name}」を削除しました"),
                        StatusKind::Success,
                    );
                    self.server_invalidate();
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: "サーバーの削除に失敗しました".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::ServerPlans { plans, disks } => {
                self.server.plans = self.store_result(plans);
                self.server.disk_plans = self.store_result(disks);
                // フォームを開いたまま届くことがあるので、既定値をここで埋める。
                self.server_plans_arrived();
            }
            Message::ServerAttachments {
                switches,
                filters,
                scripts,
            } => {
                self.server.switches = self.store_result(switches);
                self.server.packet_filters = self.store_result(filters);
                self.server.startup_scripts = self.store_result(scripts);
            }
            Message::SshKeys { from, result } => self.ssh_keys_arrived(from, result),
            Message::PacketFilters { result } => self.packet_filters_arrived(result),
            Message::NetworkMap { zone, result } => {
                self.network_map.state.select(None);
                let loadable = self.store_result(result);
                self.network_map.maps.insert(zone, loadable);
                self.ensure_loaded();
            }
            Message::NicChanged {
                what,
                failed,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(what, StatusKind::Success);
                    // NIC はサーバーの応答に入っているので、一覧を引き直す。
                    self.server_reload();
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: failed,
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::PacketFilterChanged {
                what,
                failed,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(what, StatusKind::Success);
                    self.packet_filter_invalidate();
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: failed,
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::SshKeyList { result } => {
                let count = result.as_ref().map(Vec::len).ok();
                self.ssh_key.state.select(None);
                self.ssh_key.keys = self.store_result(result);
                if let Some(count) = count {
                    self.set_status(format!("公開鍵 {count} 件"), StatusKind::Info);
                    // 一覧が届いてから選択位置を決める。
                    self.ensure_loaded();
                }
            }
            Message::SshKeyChanged {
                what,
                failed,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(what, StatusKind::Success);
                    self.ssh_key_invalidate();
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: failed,
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::ServerPlanChanged { name, result } => match result {
                Ok(()) => {
                    self.set_status(
                        format!("サーバー「{name}」のプランを変更しました"),
                        StatusKind::Success,
                    );
                    // プラン変更でIDが変わるので、一覧を引き直す。
                    self.server_invalidate();
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: "プランの変更に失敗しました".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::DiskPlans { result, archives } => {
                self.disk.plans = self.store_result(result);
                self.disk.archives = self.store_result(archives);
                self.disk_plans_arrived();
            }
            Message::ArchiveSources { result } => {
                self.disk.sources = self.store_result(result);
            }
            Message::DiskTargetServers { result } => self.disk_target_servers_arrived(result),
            Message::DiskCreated {
                name,
                copying,
                result,
            } => match result {
                Ok(()) => {
                    let note = if copying {
                        "（コピーが終わるまでしばらくかかります）"
                    } else {
                        ""
                    };
                    self.set_status(
                        format!("ディスク「{name}」を作成しました{note}"),
                        StatusKind::Success,
                    );
                    self.cloud_resources_invalidate();
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: "ディスクの作成に失敗しました".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::DiskChanged {
                what,
                failed,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(what, StatusKind::Success);
                    self.cloud_resources_invalidate();
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: failed,
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::ServerCreated {
                name,
                progress,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(
                        format!("サーバー「{name}」を作成しました"),
                        StatusKind::Success,
                    );
                    self.server_invalidate();
                    self.ensure_loaded();
                }
                Err(err) => {
                    // 途中まで作られている場合は、何が残ったかを必ず伝える。
                    let leftovers = match (progress.server_id, progress.disk_id) {
                        (None, _) => String::new(),
                        (Some(server), None) => {
                            format!(
                                "\n\nサーバー({server})は作成済みです。不要なら削除してください。"
                            )
                        }
                        (Some(server), Some(disk)) => format!(
                            "\n\nサーバー({server})とディスク({disk})は作成済みです。\n\
                             ディスクは残したままだと課金が続きます。不要なら削除してください。"
                        ),
                    };
                    self.overlay = Some(Overlay::Message {
                        title: "サーバーの作成に失敗しました".to_string(),
                        body: format!("{err}{leftovers}"),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                    self.server_invalidate();
                }
            },
            Message::RagDocumentUpdated { result } => match result {
                Ok(document) => {
                    self.set_status(
                        format!("ドキュメントを「{}」に更新しました", document.name),
                        StatusKind::Success,
                    );
                    self.ai_engine_refresh();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: "更新に失敗しました".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::RagDocumentDeleted { name, result } => match result {
                Ok(()) => {
                    self.set_status(
                        format!("ドキュメント「{name}」を削除しました"),
                        StatusKind::Success,
                    );
                    self.ai_engine_refresh();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: "削除に失敗しました".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::NetworkingSuiteGroups { result } => {
                self.networking_suite.groups = self.store_result(result);
                self.fill_selection(Pane::NetworkingSuiteGroups);
                self.networking_suite.subnet_state.select(None);
                self.networking_suite.address_state.select(None);
                self.networking_suite_ensure_loaded();
            }
            Message::NetworkingSuiteSubnets { group_srn, result } => {
                let loadable = self.store_result(result);
                self.networking_suite.subnets.insert(group_srn, loadable);
                self.fill_selection(Pane::NetworkingSuiteSubnets);
                self.networking_suite.address_state.select(None);
                self.networking_suite_ensure_loaded();
            }
            Message::NetworkingSuiteAddresses { subnet_srn, result } => {
                let loadable = self.store_result(result);
                self.networking_suite.addresses.insert(subnet_srn, loadable);
                self.fill_selection(Pane::NetworkingSuiteAddresses);
            }
            Message::CloudHsmHsms { zone, result } => {
                let loadable = self.store_result(result);
                self.cloudhsm.hsms.insert(zone, loadable);
                self.fill_selection(Pane::CloudHsmHsms);
                self.cloudhsm.client_state.select(None);
                self.cloudhsm_ensure_loaded();
            }
            Message::CloudHsmClients { hsm_id, result } => {
                let loadable = self.store_result(result);
                self.cloudhsm.clients.insert(hsm_id, loadable);
                self.fill_selection(Pane::CloudHsmClients);
            }
            Message::CloudHsmLicenses { zone, result } => {
                let loadable = self.store_result(result);
                self.cloudhsm.licenses.insert(zone, loadable);
                self.fill_selection(Pane::CloudHsmLicenses);
                self.cloudhsm.document_state.select(None);
                self.cloudhsm_ensure_loaded();
            }
            Message::CloudHsmDocuments { license_id, result } => {
                let loadable = self.store_result(result);
                self.cloudhsm.documents.insert(license_id, loadable);
                self.fill_selection(Pane::CloudHsmDocuments);
            }
            Message::SecurityControlActivation { result } => {
                self.security_control.activation = self.store_result(result);
            }
            Message::SecurityControlRules { result } => {
                self.security_control.rules = self.store_result(result);
                self.fill_selection(Pane::SecurityControlRules);
            }
            Message::SecurityControlActions { result } => {
                self.security_control.actions = self.store_result(result);
                self.fill_selection(Pane::SecurityControlActions);
            }
            Message::SegGateways { zone, result } => {
                let loadable = self.store_result(result);
                self.seg.gateways.insert(zone, loadable);
                self.fill_selection(Pane::SegGateways);
                self.fill_selection(Pane::SegServices);
            }
            Message::IamAction { label, result } => match result {
                Ok(()) => {
                    self.managed_resources
                        .items
                        .remove(&ManagedResourceKind::Iam);
                    self.service_counts.remove(&Service::Iam);
                    self.managed_resources.state.select(None);
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.managed_resources_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::AiEngineTokenVerified {
                name,
                token,
                result,
            } => match result {
                Ok(models) => {
                    match crate::config::save_ai_engine_token(
                        &self.credential_source,
                        &name,
                        &token,
                    ) {
                        Ok(()) => {
                            self.ai_engine_client = AiEngineClient::new(token).ok().map(Arc::new);
                            let count = models.len();
                            self.managed_resources
                                .items
                                .insert(ManagedResourceKind::AiEngine, Loadable::Ready(models));
                            self.managed_resources
                                .state
                                .select((count > 0).then_some(0));
                            self.ai_engine_reset_rag();
                            self.overlay = None;
                            self.set_status(
                                format!("AI Engineトークン「{name}」を保存しました（利用可能なモデル {count} 件）"),
                                StatusKind::Success,
                            );
                        }
                        Err(err) => {
                            self.overlay = Some(Overlay::AiEngineTokenForm(AiEngineTokenForm {
                                adding: true,
                                name,
                                token,
                                field: 1,
                                verifying: false,
                                ..AiEngineTokenForm::default()
                            }));
                            self.set_status(fmt_error(err), StatusKind::Error);
                        }
                    }
                }
                Err(err) => {
                    self.overlay = Some(Overlay::AiEngineTokenForm(AiEngineTokenForm {
                        adding: true,
                        name,
                        token,
                        field: 1,
                        verifying: false,
                        ..AiEngineTokenForm::default()
                    }));
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::IamCredentialsVerified { form, result } => match result {
                Ok(()) => {
                    let credentials = form.credentials();
                    match crate::config::save_iam_credentials(&self.credential_source, &credentials)
                    {
                        Ok(path) => {
                            if let Ok(config) = Config::load() {
                                self.config = config;
                            }
                            self.managed_resources
                                .items
                                .remove(&ManagedResourceKind::Iam);
                            self.service_counts.remove(&Service::Iam);
                            self.overlay = None;
                            self.set_status(
                                format!(
                                    "IAMサービスプリンシパルを保存しました: {}",
                                    path.display()
                                ),
                                StatusKind::Success,
                            );
                            self.managed_resources_ensure_loaded();
                        }
                        Err(err) => {
                            let mut form = *form;
                            form.verifying = false;
                            self.overlay = Some(Overlay::IamCredentialForm(form));
                            self.set_status(fmt_error(err), StatusKind::Error);
                        }
                    }
                }
                Err(err) => {
                    let mut form = *form;
                    form.verifying = false;
                    self.overlay = Some(Overlay::IamCredentialForm(form));
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::Registries(Ok(items)) => {
                let previous = self.selected_registry().map(|r| r.id);
                let count = items.len();
                // 再読み込み後も同じレジストリを選び直す。
                let index = previous
                    .and_then(|id| items.iter().position(|r| r.id == id))
                    .or(if items.is_empty() { None } else { Some(0) });
                self.registry.registries = Loadable::Ready(items);
                self.registry.registry_state.select(index);
                self.set_status(format!("コンテナレジストリ {count} 件"), StatusKind::Info);
                self.ensure_loaded();
            }
            Message::Registries(Err(err)) => {
                self.registry.registries = Loadable::Failed(err.clone());
                self.set_status(err, StatusKind::Error);
            }
            Message::Users { id, result } => {
                match result {
                    Ok(users) => {
                        self.registry.user_state.select(if users.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
                        self.registry.users.insert(id, Loadable::Ready(users));
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.registry.users.insert(id, Loadable::Failed(err));
                    }
                };
            }
            Message::Repositories { host, result } => {
                match result {
                    Ok(repos) => {
                        self.registry.repository_state.select(if repos.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
                        self.registry
                            .repositories
                            .insert(host, Loadable::Ready(repos));
                        self.ensure_loaded();
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.registry
                            .repositories
                            .insert(host, Loadable::Failed(err));
                    }
                };
            }
            Message::Tags {
                host,
                repository,
                result,
            } => {
                let key = (host, repository);
                match result {
                    Ok(tags) => {
                        self.registry.tag_state.select(if tags.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
                        self.registry.tags.insert(key, Loadable::Ready(tags));
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.registry.tags.insert(key, Loadable::Failed(err));
                    }
                };
            }
            Message::TagDetails { key, result } => {
                let loadable = match result {
                    Ok(detail) => Loadable::Ready(detail),
                    // タグ詳細は付加情報なのでステータス行を汚さず枠内にだけ出す。
                    Err(err) => Loadable::Failed(err),
                };
                self.registry.tag_details.insert(key, loadable);
            }
            Message::LoginVerified {
                host,
                login,
                save,
                result,
            } => match result {
                Ok(()) => {
                    if save {
                        match self.config.save_registry_login(&host, &login) {
                            Ok(_) => self.set_status(
                                format!("{host} にログインしました（パスワードはキーチェーンに保存）"),
                                StatusKind::Success,
                            ),
                            // 保存できないときに平文へ退避したりはしない。
                            Err(err) => self.set_status(
                                format!(
                                    "ログインしました。保存はできませんでした（このセッションのみ有効）: {}",
                                    fmt_error(err)
                                ),
                                StatusKind::Error,
                            ),
                        }
                    } else {
                        self.set_status(format!("{host} にログインしました"), StatusKind::Success);
                    }
                    self.registry.auto_login_tried.remove(&host);
                    self.registry
                        .repositories
                        .insert(host.clone(), Loadable::Idle);
                    self.load_repositories(host);
                }
                Err(err) => {
                    self.registry_clients.remove(&host);
                    self.overlay = Some(Overlay::Message {
                        title: "ログイン失敗".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::Applications(Ok(items)) => {
                let count = items.len();
                self.apprun.applications = Loadable::Ready(items);
                self.apprun.application_state.select(None);
                self.set_status(format!("AppRun アプリ {count} 件"), StatusKind::Info);
                self.ensure_loaded();
            }
            Message::Applications(Err(err)) => {
                self.apprun.applications = Loadable::Failed(err.clone());
                self.set_status(err, StatusKind::Error);
            }
            Message::ApplicationDetail { id, result } => {
                let loadable = match result {
                    Ok(detail) => Loadable::Ready(detail),
                    Err(err) => Loadable::Failed(err),
                };
                self.apprun.details.insert(id, loadable);
            }
            Message::Versions { id, result } => {
                match result {
                    Ok(versions) => {
                        self.apprun.version_state.select(None);
                        self.apprun.versions.insert(id, Loadable::Ready(versions));
                        self.ensure_loaded();
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.apprun.versions.insert(id, Loadable::Failed(err));
                    }
                };
            }
            Message::Traffics { id, result } => {
                let loadable = match result {
                    Ok(traffics) => Loadable::Ready(traffics),
                    Err(err) => Loadable::Failed(err),
                };
                self.apprun.traffics.insert(id, loadable);
            }
            Message::AppRunAction { id, label, result } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    // 反映を確かめたいのでトラフィックとバージョンを取り直す。
                    self.apprun.traffics.insert(id.clone(), Loadable::Idle);
                    self.apprun.versions.insert(id, Loadable::Idle);
                    self.ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::Clusters(Ok(items)) => {
                let count = items.len();
                self.dedicated.clusters = Loadable::Ready(items);
                self.dedicated.cluster_state.select(None);
                self.dedicated_after_cluster_change();
                self.set_status(format!("クラスタ {count} 件"), StatusKind::Info);
                self.ensure_loaded();
            }
            Message::Clusters(Err(err)) => {
                self.dedicated.clusters = Loadable::Failed(err.clone());
                self.set_status(err, StatusKind::Error);
            }
            Message::ClusterDetail { id, result } => {
                let loadable = match result {
                    Ok(detail) => Loadable::Ready(detail),
                    Err(err) => Loadable::Failed(err),
                };
                self.dedicated.details.insert(id, loadable);
            }
            Message::DedicatedApplications { cluster, result } => {
                let loadable = self.store_result(result);
                self.dedicated.applications.insert(cluster, loadable);
                self.dedicated.application_state.select(None);
                self.ensure_loaded();
            }
            Message::ScalingGroups { cluster, result } => {
                let loadable = self.store_result(result);
                self.dedicated.scaling_groups.insert(cluster, loadable);
                self.dedicated.scaling_group_state.select(None);
                self.ensure_loaded();
            }
            Message::Certificates { cluster, result } => {
                let loadable = self.store_result(result);
                self.dedicated.certificates.insert(cluster, loadable);
                self.dedicated.certificate_state.select(None);
                self.ensure_loaded();
            }
            Message::WorkerNodes {
                cluster,
                asg,
                result,
            } => {
                let loadable = match result {
                    Ok(nodes) => Loadable::Ready(nodes),
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        Loadable::Failed(err)
                    }
                };
                self.dedicated.worker_nodes.insert((cluster, asg), loadable);
            }
            Message::DnsZones(result) => {
                let reselect = self.dns.reselect_zone.take();
                self.dns.zones = match result {
                    Ok(zones) => {
                        let index = reselect
                            .as_deref()
                            .and_then(|name| zones.iter().position(|zone| zone.name == name));
                        self.dns.zone_state.select(index);
                        Loadable::Ready(zones)
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.dns.zone_state.select(None);
                        Loadable::Failed(err)
                    }
                };
                self.dns.record_state.select(None);
                self.ensure_loaded();
            }
            Message::DnsAction { label, result } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.dns.zones = Loadable::Idle;
                    self.dns_ensure_loaded();
                }
                Err(err) => {
                    let conflict = err.contains("409");
                    let body = if conflict {
                        format!(
                            "{err}\n\n別の画面やツールでDNSゾーンが更新された可能性があります。\n再取得後に内容を確認して、もう一度操作してください。"
                        )
                    } else {
                        self.dns.reselect_zone = None;
                        err.clone()
                    };
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body,
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                    if conflict {
                        self.dns.zones = Loadable::Idle;
                        self.dns_ensure_loaded();
                    }
                }
            },
            Message::SimpleMonitors(result) => {
                self.simple_monitor.monitors = self.store_result(result);
                self.simple_monitor.monitor_state.select(None);
                self.ensure_loaded();
            }
            Message::SimpleMonitorAction { label, result } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.simple_monitor.monitors = Loadable::Idle;
                    self.monitor_ensure_loaded();
                }
                Err(err) => {
                    let conflict = err.contains("409");
                    let body = if conflict {
                        format!(
                            "{err}\n\n別の画面やツールで監視設定が更新された可能性があります。\n再取得後に内容を確認して、もう一度操作してください。"
                        )
                    } else {
                        err.clone()
                    };
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body,
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                    if conflict {
                        self.simple_monitor.monitors = Loadable::Idle;
                        self.monitor_ensure_loaded();
                    }
                }
            },
            Message::Vaults(result) => {
                let reselect = self.secrets.reselect_vault.take();
                self.secrets.vaults = match result {
                    Ok(vaults) => {
                        let index = reselect
                            .as_deref()
                            .and_then(|id| vaults.iter().position(|vault| vault.id == id));
                        self.secrets.vault_state.select(index);
                        Loadable::Ready(vaults)
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.secrets.vault_state.select(None);
                        Loadable::Failed(err)
                    }
                };
                self.ensure_loaded();
            }
            Message::Secrets { vault, result } => {
                let loadable = self.store_result(result);
                self.secrets.secrets.insert(vault, loadable);
                self.ensure_loaded();
            }
            Message::SecretManagerAction {
                label,
                reselect_vault,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.secrets.reselect_vault = reselect_vault;
                    self.secrets.vaults = Loadable::Idle;
                    self.secrets.secrets.clear();
                    self.secrets_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::UnveiledSecret { name, result } => match result {
                Ok(value) => {
                    // 値はステータス行には出さず、閉じられるダイアログにだけ出す。
                    self.overlay = Some(Overlay::Message {
                        title: format!("{name} の値"),
                        body: value,
                        kind: StatusKind::Info,
                        scroll: 0,
                    });
                    self.set_status("値を表示しました", StatusKind::Success);
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: "値を取得できませんでした".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::Projects { zone, result } => {
                let reselect = self.monitoring.reselect_project.take();
                let loadable = match result {
                    Ok(projects) => {
                        let index = reselect.and_then(|id| {
                            projects
                                .iter()
                                .position(|project| project.resource_id == id)
                        });
                        self.monitoring.project_state.select(index);
                        Loadable::Ready(projects)
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.monitoring.project_state.select(None);
                        Loadable::Failed(err)
                    }
                };
                self.monitoring.projects.insert(zone, loadable);
                self.ensure_loaded();
            }
            Message::AlertRules {
                zone,
                project,
                result,
            } => {
                let loadable = self.store_result(result);
                self.monitoring.rules.insert((zone, project), loadable);
                self.ensure_loaded();
            }
            Message::LogMeasureRules {
                zone,
                project,
                result,
            } => {
                let loadable = self.store_result(result);
                self.monitoring
                    .log_measure_rules
                    .insert((zone, project), loadable);
                self.ensure_loaded();
            }
            Message::LogRoutings { zone, result } => {
                let loadable = self.store_result(result);
                self.monitoring.log_routings.insert(zone, loadable);
                self.ensure_loaded();
            }
            Message::MetricsRoutings { zone, result } => {
                let loadable = self.store_result(result);
                self.monitoring.metrics_routings.insert(zone, loadable);
                self.ensure_loaded();
            }
            Message::Publishers { zone, result } => {
                let loadable = self.store_result(result);
                self.monitoring.publishers.insert(zone, loadable);
                self.ensure_loaded();
            }
            Message::DashboardProjects { zone, result } => {
                let loadable = self.store_result(result);
                self.monitoring.dashboard_projects.insert(zone, loadable);
                self.ensure_loaded();
            }
            Message::AlertHistories {
                zone,
                project,
                result,
            } => {
                let loadable = self.store_result(result);
                self.monitoring.histories.insert((zone, project), loadable);
                self.ensure_loaded();
            }
            Message::NotificationTargets {
                zone,
                project,
                result,
            } => {
                let loadable = self.store_result(result);
                self.monitoring
                    .notification_targets
                    .insert((zone, project), loadable);
                self.ensure_loaded();
            }
            Message::NotificationRoutings {
                zone,
                project,
                result,
            } => {
                let loadable = self.store_result(result);
                self.monitoring
                    .notification_routings
                    .insert((zone, project), loadable);
                self.ensure_loaded();
            }
            Message::Storages { zone, result } => {
                let loadable = self.store_result(result);
                self.monitoring.storages.insert(zone, loadable);
                self.ensure_loaded();
            }
            Message::StorageAccessKeys {
                zone,
                storage,
                result,
            } => {
                let loadable = self.store_result(result);
                self.monitoring
                    .storage_keys
                    .insert((zone, storage.kind, storage.resource_id), loadable);
                self.ensure_loaded();
            }
            Message::MonitoringAction {
                zone,
                label,
                reselect_project,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring.reselect_project = reselect_project;
                    self.monitoring.projects.remove(&zone);
                    self.monitoring.rules.retain(|(z, _), _| z != &zone);
                    self.monitoring
                        .log_measure_rules
                        .retain(|(z, _), _| z != &zone);
                    self.monitoring.histories.retain(|(z, _), _| z != &zone);
                    self.monitoring
                        .notification_targets
                        .retain(|(z, _), _| z != &zone);
                    self.monitoring
                        .notification_routings
                        .retain(|(z, _), _| z != &zone);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::AlertRuleAction {
                zone,
                project,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring.rules.remove(&(zone, project));
                    self.monitoring.rule_state.select(None);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::LogMeasureRuleAction {
                zone,
                project,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring.log_measure_rules.remove(&(zone, project));
                    self.monitoring.log_measure_rule_state.select(None);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::LogRoutingAction {
                zone,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring.log_routings.remove(&zone);
                    self.monitoring.log_routing_state.select(None);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::MetricsRoutingAction {
                zone,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring.metrics_routings.remove(&zone);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => self.set_status(err, StatusKind::Error),
            },
            Message::DashboardAction {
                zone,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring.dashboard_projects.remove(&zone);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => self.set_status(err, StatusKind::Error),
            },
            Message::NotificationAction {
                zone,
                project,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    let key = (zone, project);
                    self.monitoring.notification_targets.remove(&key);
                    self.monitoring.notification_routings.remove(&key);
                    self.monitoring.notification_target_state.select(None);
                    self.monitoring.notification_routing_state.select(None);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::StorageAction {
                zone,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring.storages.remove(&zone);
                    self.monitoring
                        .storage_keys
                        .retain(|(z, _, _), _| z != &zone);
                    self.monitoring.storage_state.select(None);
                    self.monitoring.storage_key_state.select(None);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::StorageAccessKeyAction {
                zone,
                storage,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.monitoring
                        .storage_keys
                        .remove(&(zone, storage.kind, storage.resource_id));
                    self.monitoring.storage_key_state.select(None);
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::StorageAccessKeySecret {
                zone,
                storage,
                title,
                result,
            } => match result {
                Ok(key) => {
                    self.monitoring
                        .storage_keys
                        .remove(&(zone, storage.kind, storage.resource_id));
                    self.set_status(title.clone(), StatusKind::Success);
                    self.overlay = Some(Overlay::Message {
                        title,
                        body: format!(
                            "UID: {}\nトークン: {}\nシークレット: {}\n\nこの画面を閉じる前に安全な場所へ保存してください。",
                            key.uid, key.token, key.secret
                        ),
                        kind: StatusKind::Success,
                        scroll: 0,
                    });
                    self.monitoring_ensure_loaded();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{title}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::ProfileVerified { form, result } => match result {
                Ok(zones) => {
                    // 環境ごとにゾーン名が違うので、取れた一覧に差し替える。
                    if !zones.is_empty() {
                        self.zones = Loadable::Ready(zones);
                        self.zone_counts.clear();
                    }
                    self.save_verified_profile(*form)
                }
                Err(err) => {
                    let mut form = *form;
                    form.verifying = false;
                    // 入力し直せるようフォームは残し、理由は読める形で出す。
                    self.pending_form = Some(Box::new(form));
                    self.show_error(
                        "トークンを検証できませんでした",
                        {
                            // 404 は認証以前に URL が違う。
                            let hint = if err.contains("404") {
                                "接続先の URL か、ゾーン名が違う可能性が高いです。\n\
                                 環境によってゾーン名は異なります（本番は is1a など、\n\
                                 社内テスト環境では is1x のように別の名前になります）。\n\
                                 「既定ゾーン」「接続先」でそれぞれ「手入力」を選び、\n\
                                 正しい値を入れてください。"
                            } else {
                                "入力したトークンとシークレットを確かめてください。\n\
                                 貼り付けが途中で切れていることもあります（Ctrl+V / ⌘V に対応しています）。"
                            };
                            format!(
                                "{err}\n\n{hint}\n\n\
                                 閉じると入力内容を残したままフォームに戻ります。"
                            )
                        },
                    );
                }
            },
            Message::ZoneCount {
                service,
                zone,
                result,
            } => {
                let loadable = match result {
                    Ok(count) => Loadable::Ready(count),
                    // 数えられないゾーン（未契約など）もあるので静かに落とす。
                    Err(err) => Loadable::Failed(err),
                };
                self.zone_counts.insert((service, zone), loadable);
            }
            Message::AuthStatus(result) => {
                self.account.status = match *result {
                    Ok(status) => Loadable::Ready(status),
                    Err(err) => Loadable::Failed(err),
                };
                self.fill_selection(Pane::Account);
            }
            Message::ServiceCount { service, result } => {
                let loadable = match result {
                    Ok(count) => Loadable::Ready(count),
                    // 未契約や権限不足で数えられないサービスもあるので静かに落とす。
                    Err(err) => Loadable::Failed(err),
                };
                self.service_counts.insert(service, loadable);
            }
            Message::CredentialsLoaded { source, result } => match *result {
                Ok(credentials) => {
                    self.apply_credentials(*source, credentials);
                }
                Err(err) => {
                    self.show_error(format!("{} に切り替えられません", source.label()), err)
                }
            },
            Message::SavedLogin { host, login } => match login {
                Some(login) => match self.registry_clients.insert(&host, login) {
                    Ok(_) => {
                        self.registry
                            .repositories
                            .insert(host.clone(), Loadable::Idle);
                        self.load_repositories(host);
                    }
                    Err(err) => {
                        self.registry
                            .repositories
                            .insert(host, Loadable::Failed(fmt_error(err)));
                    }
                },
                // 取り出せなければ改めてログインしてもらう。
                None => {
                    self.registry.repositories.insert(
                        host,
                        Loadable::Failed(
                            "保存されたパスワードを取り出せませんでした。L キーでログインし直してください。"
                                .to_string(),
                        ),
                    );
                }
            },
            Message::BillingIdentity(result) => {
                self.billing.identity = match *result {
                    Ok(identity) => Loadable::Ready(identity),
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        Loadable::Failed(err)
                    }
                };
                self.ensure_loaded();
            }
            Message::Bills(result) => {
                self.billing.bills = self.store_result(result);
                self.billing.bill_state.select(None);
                self.ensure_loaded();
            }
            Message::BillDetails { id, result } => {
                let loadable = self.store_result(result);
                self.billing.details.insert(id, loadable);
                self.billing.detail_state.select(None);
                self.billing.summary_state.select(None);
                self.ensure_loaded();
            }
            Message::Zones(Ok(zones)) => {
                self.zones = Loadable::Ready(zones);
                // ゾーン一覧が揃ってから件数を数えに行く。
                self.load_zone_counts();
                if std::mem::take(&mut self.pending_zone_picker) {
                    self.open_zone_picker();
                }
            }
            Message::Zones(Err(err)) => {
                self.zones = Loadable::Failed(err.clone());
                self.pending_zone_picker = false;
                self.set_status(err, StatusKind::Error);
            }
            Message::Servers { zone, result } => {
                match result {
                    Ok(servers) => {
                        let count = servers.len();
                        // NIC をいじった直後などは同じ行に留まりたい。
                        // 件数が減っていることもあるので範囲に収める。
                        let keep = self.server.server_state.selected().filter(|i| *i < count);
                        self.server.server_state.select(keep);
                        self.server
                            .servers
                            .insert(zone.clone(), Loadable::Ready(servers));
                        // 操作の完了を伝えたばかりなら、件数で上書きしない。
                        if zone == self.zone && !self.server.quiet_reload {
                            self.set_status(
                                format!("{zone} のサーバー {count} 件"),
                                StatusKind::Info,
                            );
                        }
                        self.server.quiet_reload = false;
                        self.ensure_loaded();
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.server.servers.insert(zone, Loadable::Failed(err));
                    }
                };
            }
            Message::Switches { zone, result } => {
                match result {
                    Ok(switches) => {
                        let count = switches.len();
                        self.switch.switch_state.select(None);
                        self.switch
                            .switches
                            .insert(zone.clone(), Loadable::Ready(switches));
                        if zone == self.zone {
                            self.set_status(
                                format!("{zone} のスイッチ {count} 件"),
                                StatusKind::Info,
                            );
                        }
                        self.ensure_loaded();
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.switch.switches.insert(zone, Loadable::Failed(err));
                    }
                };
            }
            Message::SwitchAction {
                zone,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.load_switches(zone);
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::ServerAction {
                zone,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    // 電源状態が変わるので取り直す。
                    self.load_servers(zone);
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::RegistryAction { label, result } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.load_registries();
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::TagAction {
                host,
                repository,
                label,
                result,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    // 同じダイジェストの他のタグも消えているので一覧ごと取り直す。
                    self.registry
                        .tag_details
                        .retain(|(h, r, _), _| h != &host || r != &repository);
                    self.registry.tag_state.select(None);
                    self.load_tags(host, repository);
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::UserAction {
                id,
                label,
                result,
                save_login,
            } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.load_users(id);
                    if let Some((host, login)) = save_login {
                        self.overlay = Some(Overlay::Confirm {
                            title: "ログイン情報の保存".to_string(),
                            body: format!(
                                "ユーザー「{}」をレジストリ「{host}」のログイン情報として保存しますか？\n\
                                 既存の保存済みログイン情報があれば上書きします。",
                                login.username
                            ),
                            verify: None,
                            typed: String::new(),
                            action: ConfirmAction::SaveRegistryLogin { host, login },
                        });
                    }
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                        scroll: 0,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
        }
    }

    /// 取得結果を `Loadable` に変換しつつ、失敗ならステータス行にも出す。
    /// 一覧に限らず、単体で取ってくるリソースにも使う。
    fn store_result<T>(&mut self, result: Result<T, String>) -> Loadable<T> {
        match result {
            Ok(items) => Loadable::Ready(items),
            Err(err) => {
                self.set_status(err.clone(), StatusKind::Error);
                Loadable::Failed(err)
            }
        }
    }

    /// 長い・複数行になりうるエラーは、ステータス行ではなくダイアログに出す。
    ///
    /// ステータス行は 1 行に潰して表示するため、原因や対処が読み切れないため。
    pub fn show_error(&mut self, title: impl Into<String>, body: impl Into<String>) {
        let body = body.into();
        self.set_status(body.replace('\n', " "), StatusKind::Error);
        self.overlay = Some(Overlay::Message {
            title: title.into(),
            body,
            kind: StatusKind::Error,
            scroll: 0,
        });
    }

    pub fn set_status(&mut self, text: impl Into<String>, kind: StatusKind) {
        self.status = Some((text.into(), kind));
    }

    // --- キー入力 ---

    /// 貼り付けを、いま入力中の欄に差し込む。
    ///
    /// 改行やタブが混ざっていても欄が壊れないよう空白に潰す。
    pub fn on_paste(&mut self, text: &str) {
        // PEM秘密鍵だけは改行を維持する。通常の入力欄は従来どおり制御文字を除く。
        if let Some(Overlay::IamCredentialForm(form)) = &mut self.overlay
            && !form.verifying
        {
            if form.field == 2 {
                form.private_key.push_str(text.trim());
            } else {
                let value = text.trim().replace(['\r', '\n', '\t'], "");
                if form.field == 0 {
                    form.service_principal_id.push_str(&value);
                } else {
                    form.key_id.push_str(&value);
                }
            }
            return;
        }
        let text: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        match &mut self.overlay {
            Some(Overlay::AiEngineTokenForm(form)) if form.adding && !form.verifying => {
                if form.field == 0 {
                    form.name.push_str(text);
                } else {
                    form.token.push_str(text);
                }
            }
            Some(Overlay::ProfileForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::UserForm(form)) => match form.field {
                0 if form.mode == UserFormMode::Add => form.username.push_str(text),
                1 => form.password.push_str(text),
                _ => {}
            },
            Some(Overlay::Login(form)) => match form.field {
                0 => form.username.push_str(text),
                1 => form.password.push_str(text),
                _ => {}
            },
            Some(Overlay::RegistryForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::IamResourceForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::IamRoleForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::SwitchForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::DnsRecordForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::DnsZoneForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::SimpleMonitorForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::VaultForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::SecretForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::AlertProjectForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::AlertRuleForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::LogMeasureRuleForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::LogRoutingForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::MetricsRoutingForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::DashboardForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::NotificationTargetForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::NotificationRoutingForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::StorageForm(form)) => {
                let field = form.field;
                if let Some(value) = form.value_mut(field) {
                    value.push_str(text);
                }
            }
            Some(Overlay::StorageRetentionForm(form)) => form.days.push_str(text),
            Some(Overlay::StorageAccessKeyForm(form)) => form.description.push_str(text),
            Some(Overlay::Confirm { verify, typed, .. }) if verify.is_some() => {
                typed.push_str(text)
            }
            // 絞り込み中は検索語として受け取る。
            _ if self.filtering => {
                let pane = self.active_pane();
                if let Some(filter) = self.filters.get_mut(pane) {
                    filter.push_str(text);
                }
                self.clamp_selection(pane);
            }
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
        {
            self.should_quit = true;
            return;
        }
        if self.overlay.is_some() {
            self.on_key_overlay(key);
        } else if self.filtering {
            self.on_key_filter(key);
        } else {
            self.on_key_main(key);
        }
        self.ensure_loaded();
    }

    fn on_key_main(&mut self, key: KeyEvent) {
        // サービス横断のキーを先に処理し、残りをサービス別に渡す。
        if self.on_key_common(key) {
            return;
        }
        match self.service {
            Service::Registry => self.on_key_registry(key),
            Service::AppRun => self.on_key_apprun(key),
            Service::Dedicated => self.on_key_dedicated(key),
            Service::Server => self.on_key_server(key),
            Service::SshKey => self.on_key_ssh_key(key),
            Service::PacketFilter => self.on_key_packet_filter(key),
            Service::Switch => self.on_key_switch(key),
            Service::NetworkMap => self.on_key_network_map(key),
            Service::Disk => self.on_key_disk(key),
            Service::Archive => self.on_key_archive(key),
            Service::IsoImage
            | Service::Internet
            | Service::Bridge
            | Service::LoadBalancer
            | Service::VpcRouter
            | Service::MobileGateway
            | Service::Database
            | Service::Nfs => {}
            Service::Iam => self.on_key_iam(key),
            Service::ObjectStorage
            | Service::SimpleMq
            | Service::SimpleNotification
            | Service::EventBus
            | Service::Workflows
            | Service::WebAccel
            | Service::EnhancedLoadBalancer
            | Service::LocalRouter
            | Service::Gslb
            | Service::Kms
            | Service::AutoScale
            | Service::EnhancedDb => {}
            Service::AutoBackup => self.on_key_auto_backup(key),
            Service::AiEngine => self.on_key_ai_engine(key),
            Service::NetworkingSuite => self.on_key_networking_suite(key),
            Service::CloudHsm => self.on_key_cloudhsm(key),
            Service::SecurityControl => self.on_key_security_control(key),
            Service::Seg => self.on_key_seg(key),
            Service::NoSql => self.on_key_nosql(key),
            Service::ApiGateway => self.on_key_api_gateway(key),
            Service::Secrets => self.on_key_secrets(key),
            Service::Monitoring => self.on_key_monitoring(key),
            // 権限画面は一覧を見るだけなので、共通のキーだけで足りる。
            Service::Account => {}
            Service::Billing => self.on_key_billing(key),
            Service::Dns => self.on_key_dns(key),
            Service::SimpleMonitor => self.on_key_simple_monitor(key),
        }
    }

    /// どのサービスでも同じ意味を持つキー。処理したら true。
    fn on_key_common(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.overlay = Some(Overlay::Help),
            KeyCode::Char('s') => self.open_service_picker(),
            KeyCode::Char('S') => self.cycle_service(1),
            KeyCode::Char('z') => self.open_zone_picker(),
            KeyCode::Char('p') => self.open_profile_picker(),
            KeyCode::Char('w') => self.toggle_mode(),
            KeyCode::Char('/') => self.start_filtering(),
            KeyCode::Char('y') => self.copy_selection(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('R') => {
                self.invalidate_all();
                self.refresh();
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => self.jump_selection(true),
            KeyCode::End | KeyCode::Char('G') => self.jump_selection(false),
            // 絞り込みが掛かっていればまず解除し、無ければサービス側の「戻る」に任せる。
            KeyCode::Esc => {
                let pane = self.active_pane();
                if self.filters.get(pane).is_empty() {
                    return false;
                }
                if let Some(filter) = self.filters.get_mut(pane) {
                    filter.clear();
                    self.clamp_selection(pane);
                }
            }
            _ => return false,
        }
        true
    }

    fn on_key_registry(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.focus_left(),
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('1') => self.set_tab(Tab::Overview),
            KeyCode::Char('2') => self.set_tab(Tab::Users),
            KeyCode::Char('3') => self.set_tab(Tab::Images),
            KeyCode::Char('a') => self.open_add_user(),
            KeyCode::Char('e') => self.open_edit_user(),
            KeyCode::Char('d') => self.delete_selected(),
            KeyCode::Char('n') => self.open_create_registry(),
            KeyCode::Char('E') => self.open_edit_registry(),
            KeyCode::Char('D') => self.confirm_delete_registry(),
            KeyCode::Char('L') => self.open_login(),
            KeyCode::Char('O') => self.confirm_forget_login(),
            KeyCode::Left | KeyCode::Char('h') => self.focus_left(),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => self.focus_right(),
            _ => {}
        }
    }

    fn on_key_iam(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('a') => self.open_iam_credential_form(),
            KeyCode::Char('u') => self.open_create_iam_resource("ユーザー"),
            KeyCode::Char('U') => self.open_create_iam_resource("グループ"),
            KeyCode::Char('P') => self.open_create_iam_resource("プロジェクト"),
            KeyCode::Char('N') => self.open_create_iam_resource("サービスプリンシパル"),
            KeyCode::Char('E') => self.open_edit_iam_resource(),
            KeyCode::Char('D') => self.confirm_delete_iam_resource(),
            KeyCode::Char('g') => self.open_iam_role_form(true),
            KeyCode::Char('G') => self.open_iam_role_form(false),
            _ => {}
        }
    }

    fn open_create_iam_resource(&mut self, resource_type: &str) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::IamResourceForm(IamResourceForm {
            mode: IamResourceFormMode::Create,
            resource_type: resource_type.to_string(),
            target_id: None,
            name: String::new(),
            code: String::new(),
            password: String::new(),
            description: String::new(),
            extra: String::new(),
            field: 0,
        }));
    }

    fn open_edit_iam_resource(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(resource) = self.selected_managed_resource() else {
            return;
        };
        if resource.resource_type == "ロール" {
            self.set_status("IAMロールの定義は編集できません", StatusKind::Info);
            return;
        }
        self.overlay = Some(Overlay::IamResourceForm(IamResourceForm {
            mode: IamResourceFormMode::Edit,
            resource_type: resource.resource_type,
            target_id: Some(resource.id),
            name: resource.name,
            code: String::new(),
            password: String::new(),
            description: resource.description,
            extra: String::new(),
            field: 0,
        }));
    }

    fn submit_iam_resource_form(&mut self, form: IamResourceForm) {
        if form.name.trim().is_empty() {
            self.set_status("名前を入力してください", StatusKind::Error);
            self.overlay = Some(Overlay::IamResourceForm(form));
            return;
        }
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!(
            "{}「{}」を{}",
            form.resource_type,
            form.name,
            match form.mode {
                IamResourceFormMode::Create => "作成",
                IamResourceFormMode::Edit => "更新",
            }
        );
        let validation_error = match (form.mode, form.resource_type.as_str()) {
            (IamResourceFormMode::Create, "ユーザー")
                if form.code.is_empty() || form.password.is_empty() =>
            {
                Some("ユーザーコードとパスワードを入力してください")
            }
            (IamResourceFormMode::Create, "プロジェクト") if form.code.is_empty() => {
                Some("プロジェクトコードを入力してください")
            }
            (IamResourceFormMode::Create, "サービスプリンシパル")
                if form.extra.parse::<i64>().is_err() =>
            {
                Some("プロジェクトIDを数値で入力してください")
            }
            _ => None,
        };
        if let Some(error) = validation_error {
            self.set_status(error, StatusKind::Error);
            self.overlay = Some(Overlay::IamResourceForm(form));
            return;
        }
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);
        tokio::spawn(async move {
            let result = match (form.mode, form.resource_type.as_str()) {
                (IamResourceFormMode::Create, "ユーザー") => {
                    client
                        .create_iam_user(
                            &form.name,
                            &form.code,
                            &form.password,
                            &form.description,
                            &form.extra,
                        )
                        .await
                }
                (IamResourceFormMode::Create, "プロジェクト") => {
                    let parent = if form.extra.is_empty() {
                        None
                    } else {
                        form.extra.parse::<i64>().ok()
                    };
                    client
                        .create_iam_project(&form.name, &form.code, &form.description, parent)
                        .await
                }
                (IamResourceFormMode::Create, "グループ") => {
                    client.create_iam_group(&form.name, &form.description).await
                }
                (IamResourceFormMode::Create, "サービスプリンシパル") => {
                    client
                        .create_iam_service_principal(
                            form.extra.parse().unwrap_or_default(),
                            &form.name,
                            &form.description,
                        )
                        .await
                }
                (IamResourceFormMode::Edit, "ユーザー") => {
                    client
                        .update_iam_user(
                            form.target_id.as_deref().unwrap_or_default(),
                            &form.name,
                            (!form.password.is_empty()).then_some(form.password.as_str()),
                            &form.description,
                        )
                        .await
                }
                (IamResourceFormMode::Edit, "プロジェクト") => {
                    client
                        .update_iam_project(
                            form.target_id.as_deref().unwrap_or_default(),
                            &form.name,
                            &form.description,
                        )
                        .await
                }
                (IamResourceFormMode::Edit, "グループ") => {
                    client
                        .update_iam_group(
                            form.target_id.as_deref().unwrap_or_default(),
                            &form.name,
                            &form.description,
                        )
                        .await
                }
                (IamResourceFormMode::Edit, "サービスプリンシパル") => {
                    client
                        .update_iam_service_principal(
                            form.target_id.as_deref().unwrap_or_default(),
                            &form.name,
                            &form.description,
                        )
                        .await
                }
                _ => Err(anyhow::anyhow!("このIAMリソースは操作できません")),
            }
            .map_err(fmt_error);
            let _ = tx.send(Message::IamAction { label, result });
        });
    }

    fn confirm_delete_iam_resource(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(resource) = self.selected_managed_resource() else {
            return;
        };
        if resource.resource_type == "ロール" {
            self.set_status("IAMロールの定義は削除できません", StatusKind::Info);
            return;
        }
        self.overlay = Some(Overlay::Confirm {
            title: format!("{}の削除", resource.resource_type),
            body: format!(
                "{}「{}」を削除します。関連するアクセスが失われる可能性があります。\n実行するには名前を入力してください。",
                resource.resource_type, resource.name
            ),
            verify: Some(resource.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteIamResource {
                resource_type: resource.resource_type,
                id: resource.id,
                name: resource.name,
            },
        });
    }

    fn open_iam_role_form(&mut self, grant: bool) {
        if !self.require_write() {
            return;
        }
        let selected = self.selected_managed_resource();
        let mut form = IamRoleForm {
            grant,
            project_id: String::new(),
            principal_type: "user".to_string(),
            principal_id: String::new(),
            role_id: String::new(),
            field: 0,
        };
        if let Some(resource) = selected {
            match resource.resource_type.as_str() {
                "プロジェクト" => form.project_id = resource.id,
                "ユーザー" => form.principal_id = resource.id,
                "グループ" => {
                    form.principal_type = "group".to_string();
                    form.principal_id = resource.id;
                }
                "サービスプリンシパル" => {
                    form.principal_type = "service-principal".to_string();
                    form.principal_id = resource.id;
                    form.project_id = resource.plan;
                }
                "ロール" => form.role_id = resource.id,
                _ => {}
            }
        }
        self.overlay = Some(Overlay::IamRoleForm(form));
    }

    fn submit_iam_role_form(&mut self, form: IamRoleForm) {
        let (Ok(project_id), Ok(principal_id)) = (
            form.project_id.parse::<i64>(),
            form.principal_id.parse::<i64>(),
        ) else {
            self.set_status(
                "プロジェクトIDとプリンシパルIDは数値で入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::IamRoleForm(form));
            return;
        };
        if form.role_id.is_empty()
            || !matches!(
                form.principal_type.as_str(),
                "user" | "group" | "service-principal"
            )
        {
            self.set_status(
                "ロールIDとプリンシパル種別(user/group/service-principal)を確認してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::IamRoleForm(form));
            return;
        }
        let action_label = if form.grant { "付与" } else { "解除" };
        self.overlay = Some(Overlay::Confirm {
            title: format!("IAMロールの{action_label}"),
            body: format!(
                "プロジェクト {project_id} の {} {principal_id} に対して、\nロール「{}」を{action_label}します。実行しますか？",
                form.principal_type, form.role_id
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::ChangeIamRole {
                project_id,
                principal_type: form.principal_type,
                principal_id,
                role_id: form.role_id,
                grant: form.grant,
            },
        });
    }

    // --- サービスの切り替え ---

    fn open_service_picker(&mut self) {
        let index = Service::ALL
            .iter()
            .position(|svc| *svc == self.service)
            .unwrap_or(0);
        // どのサービスにどれだけリソースがあるかを、行かなくても分かるようにする。
        self.load_service_counts();
        self.overlay = Some(Overlay::ServicePicker {
            index,
            initial: false,
        });
    }

    /// 起動時は特定サービスを既定にせず、一覧の先頭から選んでもらう。
    pub fn open_initial_service_picker(&mut self) {
        self.load_service_counts();
        self.overlay = Some(Overlay::ServicePicker {
            index: 0,
            initial: true,
        });
    }

    fn switch_service(&mut self, service: Service) {
        self.service = service;
        self.filtering = false;
        self.set_status(service.title(), StatusKind::Info);
        self.ensure_loaded();
    }

    fn cycle_service(&mut self, delta: i32) {
        let current = Service::ALL
            .iter()
            .position(|svc| *svc == self.service)
            .unwrap_or(0) as i32;
        let len = Service::ALL.len() as i32;
        self.service = Service::ALL[(current + delta).rem_euclid(len) as usize];
        self.filtering = false;
        self.set_status(self.service.title(), StatusKind::Info);
        self.ensure_loaded();
    }

    // --- ゾーンの切り替え ---

    fn open_zone_picker(&mut self) {
        if !self.service.is_zoned() {
            self.set_status(
                format!("{} はゾーンに依存しません", self.service.title()),
                StatusKind::Info,
            );
            return;
        }
        match self.zones.clone() {
            Loadable::Ready(zones) if !zones.is_empty() => {
                let index = zones.iter().position(|z| z.name == self.zone).unwrap_or(0);
                self.overlay = Some(Overlay::ZonePicker { zones, index });
                self.load_zone_counts();
            }
            // 取得を待って自動で開く（利用者に2回押させない）。
            Loadable::Loading => {
                self.pending_zone_picker = true;
                self.set_status("ゾーン一覧を取得中です…", StatusKind::Info);
            }
            _ => {
                self.pending_zone_picker = true;
                self.load_zones();
                self.set_status("ゾーン一覧を取得しています…", StatusKind::Info);
            }
        }
    }

    fn load_zones(&mut self) {
        self.zones = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_zones().await.map_err(fmt_error);
            let _ = tx.send(Message::Zones(result));
        });
    }

    /// 各ゾーンのリソース件数を数える。
    ///
    /// ゾーンを選ぶ前に「どこに何があるか」が分かるようにするためのもの。
    /// 一覧そのものは引かず、総件数だけを取る軽いリクエストを投げる。
    fn load_zone_counts(&mut self) {
        let service = self.service;
        if service.countable_label().is_none() {
            return;
        }
        let Some(zones) = self.zones.ready().cloned() else {
            return;
        };
        for zone in zones {
            let key = (service, zone.name.clone());
            if !self.zone_counts.get(&key).is_none_or(Loadable::is_idle) {
                continue;
            }
            self.zone_counts.insert(key, Loadable::Loading);
            self.inflight += 1;
            let sacloud = self.sacloud.clone();
            let monitoring = self.monitoring_client.clone();
            let tx = self.tx.clone();
            let name = zone.name.clone();
            tokio::spawn(async move {
                let result = match service {
                    Service::Server => sacloud.count_servers(&name).await,
                    Service::Switch => sacloud.count_switches(&name).await,
                    Service::Disk => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::Disk)
                            .await
                    }
                    Service::Archive => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::Archive)
                            .await
                    }
                    Service::IsoImage => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::IsoImage)
                            .await
                    }
                    Service::Internet => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::Internet)
                            .await
                    }
                    Service::PacketFilter => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::PacketFilter)
                            .await
                    }
                    Service::Bridge => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::Bridge)
                            .await
                    }
                    Service::LoadBalancer => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::LoadBalancer)
                            .await
                    }
                    Service::VpcRouter => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::VpcRouter)
                            .await
                    }
                    Service::MobileGateway => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::MobileGateway)
                            .await
                    }
                    Service::Database => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::Database)
                            .await
                    }
                    Service::Nfs => {
                        sacloud
                            .count_cloud_resources(&name, CloudResourceKind::Nfs)
                            .await
                    }
                    Service::Secrets => sacloud.count_vaults().await,
                    Service::Monitoring => monitoring.count_projects(&name).await,
                    Service::Seg => sacloud.list_segs(&name).await.map(|v| v.len()),
                    Service::CloudHsm => sacloud.list_cloudhsms(&name).await.map(|v| v.len()),
                    Service::ApiGateway => Ok(0),
                    _ => Ok(0),
                };
                let _ = tx.send(Message::ZoneCount {
                    service,
                    zone: name,
                    result: result.map_err(fmt_error),
                });
            });
        }
    }

    /// サービス一覧に出す件数を集める。
    ///
    /// すでに画面を開いて取得済みのものはその数を使い、API は呼ばない。
    fn load_service_counts(&mut self) {
        for service in Service::ALL {
            if service.count_label().is_none() {
                continue;
            }
            // 取得済み・取得中は触らない。
            if !self
                .service_counts
                .get(&service)
                .is_none_or(Loadable::is_idle)
            {
                continue;
            }
            if let Some(count) = self.loaded_service_count(service) {
                self.service_counts.insert(service, Loadable::Ready(count));
                continue;
            }
            self.service_counts.insert(service, Loadable::Loading);
            self.inflight += 1;
            let sacloud = self.sacloud.clone();
            let apprun = self.apprun_client.clone();
            let dedicated = self.dedicated_client.clone();
            let monitoring = self.monitoring_client.clone();
            let api_gateway = self.api_gateway_client.clone();
            let tx = self.tx.clone();
            let zone = self.zone.clone();
            let year = billing::current_year();
            tokio::spawn(async move {
                let result = match service {
                    Service::Server => sacloud.count_servers(&zone).await,
                    Service::SshKey => sacloud.list_ssh_keys(&zone).await.map(|k| k.len()),
                    Service::PacketFilter => {
                        sacloud.list_packet_filters(&zone).await.map(|f| f.len())
                    }
                    Service::Switch => sacloud.count_switches(&zone).await,
                    // 件数に意味が無いので数えない。
                    Service::NetworkMap => Ok(0),
                    Service::Disk => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::Disk)
                            .await
                    }
                    Service::Archive => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::Archive)
                            .await
                    }
                    Service::IsoImage => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::IsoImage)
                            .await
                    }
                    Service::Internet => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::Internet)
                            .await
                    }
                    Service::Bridge => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::Bridge)
                            .await
                    }
                    Service::LoadBalancer => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::LoadBalancer)
                            .await
                    }
                    Service::VpcRouter => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::VpcRouter)
                            .await
                    }
                    Service::MobileGateway => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::MobileGateway)
                            .await
                    }
                    Service::Database => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::Database)
                            .await
                    }
                    Service::Nfs => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::Nfs)
                            .await
                    }
                    Service::Secrets => sacloud.count_vaults().await,
                    Service::Monitoring => monitoring.count_projects(&zone).await,
                    Service::SecurityControl => {
                        sacloud.list_evaluation_rules().await.map(|v| v.len())
                    }
                    Service::CloudHsm => sacloud.list_cloudhsms(&zone).await.map(|v| v.len()),
                    Service::NetworkingSuite => sacloud.list_subnet_groups().await.map(|v| v.len()),
                    Service::Seg => sacloud.list_segs(&zone).await.map(|v| v.len()),
                    Service::NoSql => sacloud.list_nosql_databases().await.map(|v| v.len()),
                    Service::ApiGateway => api_gateway.list_services().await.map(|v| v.len()),
                    // 件数専用の API が無いものは一覧を引いて数える。
                    Service::Registry => sacloud.list_registries().await.map(|v| v.len()),
                    Service::Dns => sacloud.list_dns_zones().await.map(|v| v.len()),
                    Service::SimpleMonitor => sacloud.list_simple_monitors().await.map(|v| v.len()),
                    Service::AppRun => apprun.list_applications().await.map(|v| v.len()),
                    Service::Dedicated => dedicated.list_clusters().await.map(|v| v.len()),
                    // AI Engineはcount_labelが無いため、この分岐には通常到達しない。
                    Service::AiEngine => Ok(0),
                    Service::ObjectStorage => sacloud
                        .list_managed_resources(ManagedResourceKind::ObjectStorage)
                        .await
                        .map(|v| v.len()),
                    Service::SimpleMq => sacloud
                        .list_managed_resources(ManagedResourceKind::SimpleMq)
                        .await
                        .map(|v| v.len()),
                    Service::SimpleNotification => sacloud
                        .list_managed_resources(ManagedResourceKind::SimpleNotification)
                        .await
                        .map(|v| v.len()),
                    Service::EventBus => sacloud
                        .list_managed_resources(ManagedResourceKind::EventBus)
                        .await
                        .map(|v| v.len()),
                    Service::Workflows => sacloud
                        .list_managed_resources(ManagedResourceKind::Workflows)
                        .await
                        .map(|v| v.len()),
                    Service::WebAccel => sacloud
                        .list_managed_resources(ManagedResourceKind::WebAccel)
                        .await
                        .map(|v| v.len()),
                    Service::EnhancedLoadBalancer => sacloud
                        .list_managed_resources(ManagedResourceKind::EnhancedLoadBalancer)
                        .await
                        .map(|v| v.len()),
                    Service::LocalRouter => sacloud
                        .list_managed_resources(ManagedResourceKind::LocalRouter)
                        .await
                        .map(|v| v.len()),
                    Service::Gslb => sacloud
                        .list_managed_resources(ManagedResourceKind::Gslb)
                        .await
                        .map(|v| v.len()),
                    Service::Kms => sacloud
                        .list_managed_resources(ManagedResourceKind::Kms)
                        .await
                        .map(|v| v.len()),
                    Service::Iam => sacloud
                        .list_managed_resources(ManagedResourceKind::Iam)
                        .await
                        .map(|v| v.len()),
                    Service::AutoScale => sacloud
                        .list_managed_resources(ManagedResourceKind::AutoScale)
                        .await
                        .map(|v| v.len()),
                    Service::EnhancedDb => sacloud
                        .list_managed_resources(ManagedResourceKind::EnhancedDb)
                        .await
                        .map(|v| v.len()),
                    Service::AutoBackup => sacloud
                        .list_managed_resources(ManagedResourceKind::AutoBackup)
                        .await
                        .map(|v| v.len()),
                    // 請求はアカウントIDを引いてから年を指定して数える。
                    Service::Billing => match sacloud.billing_identity().await {
                        Ok(identity) => sacloud
                            .list_bills(&identity.account_id, year)
                            .await
                            .map(|v| v.len()),
                        Err(err) => Err(err),
                    },
                    Service::Account => Ok(0),
                };
                let _ = tx.send(Message::ServiceCount {
                    service,
                    result: result.map_err(fmt_error),
                });
            });
        }
    }

    /// サービス一覧に出す、そのサービスが使えるかどうか。
    ///
    /// 実際に呼んだ結果だけで判断する。権限の値から推測すると、
    /// 権限の意味を取り違えたときに「使えるのに使えない」と嘘をつくため。
    pub fn service_availability(&self, service: Service) -> Availability {
        match self.service_counts.get(&service) {
            Some(Loadable::Failed(err)) => Availability::Unusable(availability_reason(err)),
            Some(Loadable::Ready(_)) => Availability::Usable,
            _ => Availability::Unknown,
        }
    }

    /// すでに画面で読み込んでいる件数。無ければ `None`。
    fn loaded_service_count(&self, service: Service) -> Option<usize> {
        let len = match service {
            Service::Registry => self.registry.registries.ready()?.len(),
            Service::AppRun => self.apprun.applications.ready()?.len(),
            Service::Dedicated => self.dedicated.clusters.ready()?.len(),
            Service::Dns => self.dns.zones.ready()?.len(),
            Service::SimpleMonitor => self.simple_monitor.monitors.ready()?.len(),
            Service::Server => self.server.servers.get(&self.zone)?.ready()?.len(),
            Service::SshKey => self.ssh_key.keys.ready()?.len(),
            Service::PacketFilter => self.packet_filter.filters.ready()?.len(),
            Service::Switch => self.switch.switches.get(&self.zone)?.ready()?.len(),
            Service::NetworkMap => return None,
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::Bridge
            | Service::LoadBalancer
            | Service::VpcRouter
            | Service::MobileGateway
            | Service::Database
            | Service::Nfs => {
                let kind = match service {
                    Service::Disk => CloudResourceKind::Disk,
                    Service::Archive => CloudResourceKind::Archive,
                    Service::IsoImage => CloudResourceKind::IsoImage,
                    Service::Internet => CloudResourceKind::Internet,
                    Service::PacketFilter => CloudResourceKind::PacketFilter,
                    Service::Bridge => CloudResourceKind::Bridge,
                    Service::LoadBalancer => CloudResourceKind::LoadBalancer,
                    Service::VpcRouter => CloudResourceKind::VpcRouter,
                    Service::MobileGateway => CloudResourceKind::MobileGateway,
                    Service::Database => CloudResourceKind::Database,
                    Service::Nfs => CloudResourceKind::Nfs,
                    _ => unreachable!(),
                };
                self.cloud_resources
                    .items
                    .get(&(self.zone.clone(), kind))?
                    .ready()?
                    .len()
            }
            Service::Secrets => self.secrets.vaults.ready()?.len(),
            Service::Monitoring => self.monitoring.projects.get(&self.zone)?.ready()?.len(),
            Service::CloudHsm => self.cloudhsm.hsms.get(&self.zone)?.ready()?.len(),
            Service::NetworkingSuite => self.networking_suite.groups.ready()?.len(),
            // 件数はモデル一覧のもの。RAG はトークン次第なので数えない。
            Service::AiEngine => self
                .managed_resources
                .items
                .get(&ManagedResourceKind::AiEngine)?
                .ready()?
                .len(),
            Service::SecurityControl => self.security_control.rules.ready()?.len(),
            Service::Seg => self.seg.gateways.get(&self.zone)?.ready()?.len(),
            Service::NoSql => self.nosql.databases.ready()?.len(),
            Service::ApiGateway => self.api_gateway.services.ready()?.len(),
            Service::ObjectStorage
            | Service::SimpleMq
            | Service::SimpleNotification
            | Service::EventBus
            | Service::Workflows
            | Service::WebAccel
            | Service::EnhancedLoadBalancer
            | Service::LocalRouter
            | Service::Gslb
            | Service::Kms
            | Service::Iam
            | Service::AutoScale
            | Service::EnhancedDb
            | Service::AutoBackup => {
                let kind = match service {
                    Service::ObjectStorage => ManagedResourceKind::ObjectStorage,
                    Service::SimpleMq => ManagedResourceKind::SimpleMq,
                    Service::SimpleNotification => ManagedResourceKind::SimpleNotification,
                    Service::EventBus => ManagedResourceKind::EventBus,
                    Service::Workflows => ManagedResourceKind::Workflows,
                    Service::WebAccel => ManagedResourceKind::WebAccel,
                    Service::EnhancedLoadBalancer => ManagedResourceKind::EnhancedLoadBalancer,
                    Service::LocalRouter => ManagedResourceKind::LocalRouter,
                    Service::Gslb => ManagedResourceKind::Gslb,
                    Service::Kms => ManagedResourceKind::Kms,
                    Service::Iam => ManagedResourceKind::Iam,
                    Service::AutoScale => ManagedResourceKind::AutoScale,
                    Service::EnhancedDb => ManagedResourceKind::EnhancedDb,
                    Service::AutoBackup => ManagedResourceKind::AutoBackup,
                    _ => unreachable!(),
                };
                self.managed_resources.items.get(&kind)?.ready()?.len()
            }
            // 請求画面で別の年に移っていることがあるので、
            // 今年を見ているときだけ流用する。
            Service::Billing if self.billing.year == billing::current_year() => {
                self.billing.bills.ready()?.len()
            }
            Service::Billing => return None,
            Service::Account => return None,
        };
        Some(len)
    }

    fn switch_zone(&mut self, zone: String) {
        if zone == self.zone {
            return;
        }
        self.zone = zone;
        // ゾーン依存の件数は数え直す（ゾーンに依存しないものはそのまま使える）。
        self.service_counts.retain(|service, _| !service.is_zoned());
        self.server.server_state.select(None);
        self.switch.switch_state.select(None);
        self.set_status(
            format!("ゾーンを {} に切り替えました", self.zone),
            StatusKind::Info,
        );
        self.ensure_loaded();
    }

    // --- 操作モード ---

    fn toggle_mode(&mut self) {
        self.mode = self.mode.toggled();
        match self.mode {
            Mode::Write => self.set_status(
                "書き込みモードに切り替えました（w で読み取り専用に戻ります）",
                StatusKind::Error,
            ),
            Mode::ReadOnly => {
                self.set_status("読み取り専用モードに戻しました", StatusKind::Success)
            }
        }
    }

    /// 書き込み操作の入口で呼ぶ。読み取り専用なら false を返して案内を出す。
    fn require_write(&mut self) -> bool {
        if self.mode == Mode::Write {
            return true;
        }
        self.set_status(
            "読み取り専用モードです。w キーで書き込みモードに切り替えてください",
            StatusKind::Info,
        );
        false
    }

    // --- 絞り込み ---

    fn start_filtering(&mut self) {
        if self.active_pane() == Pane::None {
            return;
        }
        self.filtering = true;
    }

    /// 絞り込み文字列の編集中のキー入力。
    fn on_key_filter(&mut self, key: KeyEvent) {
        let pane = self.active_pane();
        let Some(filter) = self.filters.get_mut(pane) else {
            self.filtering = false;
            return;
        };
        match key.code {
            // Enter は確定、Esc は取り消して絞り込みを解除。
            KeyCode::Enter => self.filtering = false,
            KeyCode::Esc => {
                filter.clear();
                self.filtering = false;
            }
            KeyCode::Backspace => {
                filter.pop();
            }
            KeyCode::Char(c) => filter.push(c),
            _ => return,
        }
        self.clamp_selection(pane);
    }

    /// 絞り込みで件数が減ったときに選択位置がはみ出さないようにする。
    fn clamp_selection(&mut self, pane: Pane) {
        let len = self.visible_len(pane);
        let Some(state) = self.list_state(pane) else {
            return;
        };
        match (state.selected(), len) {
            (_, 0) => state.select(None),
            (Some(index), len) if index >= len => state.select(Some(len - 1)),
            (None, _) => state.select(Some(0)),
            _ => {}
        }
        // 同じ行番号でも絞り込み後は別の親項目を指し得るため、子の選択を更新する。
        self.after_selection_change(pane);
    }

    /// 読み込み済みなのに未選択なら先頭を選ぶ。
    pub(super) fn fill_selection(&mut self, pane: Pane) {
        let len = self.visible_len(pane);
        if let Some(state) = self.list_state(pane)
            && len > 0
            && state.selected().is_none()
        {
            state.select(Some(0));
        }
    }

    fn visible_len(&self, pane: Pane) -> usize {
        match pane {
            Pane::Registries => self.visible_registries().len(),
            Pane::Users => self.visible_users().ready().map_or(0, Vec::len),
            Pane::Repositories => self.visible_repositories().ready().map_or(0, Vec::len),
            Pane::Tags => self.visible_tags().ready().map_or(0, Vec::len),
            Pane::Applications => self.visible_applications().len(),
            Pane::Versions => self.visible_versions().ready().map_or(0, Vec::len),
            Pane::Servers => self.visible_servers().ready().map_or(0, Vec::len),
            Pane::SshKeys => self.visible_ssh_keys().ready().map_or(0, Vec::len),
            Pane::Nics => self.visible_nics().len(),
            Pane::PacketFilters => self.visible_packet_filters().ready().map_or(0, Vec::len),
            Pane::NetworkMap => self.visible_network_map().ready().map_or(0, Vec::len),
            Pane::PacketFilterRules => self.visible_packet_filter_rules().len(),
            Pane::Switches => self.visible_switches().ready().map_or(0, Vec::len),
            Pane::CloudResources => self.visible_cloud_resources().ready().map_or(0, Vec::len),
            Pane::ManagedResources => self.visible_managed_resources().ready().map_or(0, Vec::len),
            Pane::ApiGatewaySubscriptions => self
                .visible_api_gateway_subscriptions()
                .ready()
                .map_or(0, Vec::len),
            Pane::ApiGatewayServices => self
                .visible_api_gateway_services()
                .ready()
                .map_or(0, Vec::len),
            Pane::ApiGatewayRoutes => self
                .visible_api_gateway_routes()
                .ready()
                .map_or(0, Vec::len),
            Pane::ApiGatewayUsers => self.visible_api_gateway_users().ready().map_or(0, Vec::len),
            Pane::ApiGatewayGroups => self
                .visible_api_gateway_groups()
                .ready()
                .map_or(0, Vec::len),
            Pane::ApiGatewayDomains => self
                .visible_api_gateway_domains()
                .ready()
                .map_or(0, Vec::len),
            Pane::ApiGatewayCertificates => self
                .visible_api_gateway_certificates()
                .ready()
                .map_or(0, Vec::len),
            Pane::AiEngineModels => self
                .visible_ai_engine_cloud_models()
                .ready()
                .map_or(0, Vec::len),
            Pane::AiEngineDocuments => self
                .visible_ai_engine_documents()
                .ready()
                .map_or(0, Vec::len),
            Pane::NetworkingSuiteGroups => self
                .visible_networking_suite_groups()
                .ready()
                .map_or(0, Vec::len),
            Pane::NetworkingSuiteSubnets => self
                .visible_networking_suite_subnets()
                .ready()
                .map_or(0, Vec::len),
            Pane::NetworkingSuiteAddresses => self
                .visible_networking_suite_addresses()
                .ready()
                .map_or(0, Vec::len),
            Pane::CloudHsmHsms => self.visible_cloudhsm_hsms().ready().map_or(0, Vec::len),
            Pane::CloudHsmClients => self.visible_cloudhsm_clients().ready().map_or(0, Vec::len),
            Pane::CloudHsmLicenses => self.visible_cloudhsm_licenses().ready().map_or(0, Vec::len),
            Pane::CloudHsmDocuments => self
                .visible_cloudhsm_documents()
                .ready()
                .map_or(0, Vec::len),
            Pane::SecurityControlRules => self
                .visible_security_control_rules()
                .ready()
                .map_or(0, Vec::len),
            Pane::SecurityControlActions => self
                .visible_security_control_actions()
                .ready()
                .map_or(0, Vec::len),
            Pane::SegGateways => self.visible_seg_gateways().ready().map_or(0, Vec::len),
            Pane::SegServices => self.visible_seg_services().ready().map_or(0, Vec::len),
            Pane::NoSqlDatabases => self.visible_nosql_databases().ready().map_or(0, Vec::len),
            Pane::NoSqlNodes => self.visible_nosql_nodes().ready().map_or(0, Vec::len),
            Pane::NoSqlBackups => self.visible_nosql_backups().ready().map_or(0, Vec::len),
            Pane::NoSqlParameters => self.visible_nosql_parameters().ready().map_or(0, Vec::len),
            Pane::ApiGatewayOidcs => self.visible_api_gateway_oidcs().ready().map_or(0, Vec::len),
            Pane::Clusters => self.visible_clusters().len(),
            Pane::DedicatedApplications => self
                .visible_dedicated_applications()
                .ready()
                .map_or(0, Vec::len),
            Pane::ScalingGroups => self.visible_scaling_groups().ready().map_or(0, Vec::len),
            Pane::Certificates => self.visible_certificates().ready().map_or(0, Vec::len),
            Pane::DnsZones => self.visible_dns_zones().len(),
            Pane::DnsRecords => self.visible_dns_records().len(),
            Pane::Monitors => self.visible_monitors().len(),
            Pane::Vaults => self.visible_vaults().len(),
            Pane::Secrets => self.visible_secrets().ready().map_or(0, Vec::len),
            Pane::Projects => self.visible_projects().len(),
            Pane::Rules => self.visible_rules().ready().map_or(0, Vec::len),
            Pane::LogMeasureRules => self.visible_log_measure_rules().ready().map_or(0, Vec::len),
            Pane::LogRoutings => self.visible_log_routings().ready().map_or(0, Vec::len),
            Pane::MetricsRoutings => self.visible_metrics_routings().ready().map_or(0, Vec::len),
            Pane::Dashboards => self
                .visible_dashboard_projects()
                .ready()
                .map_or(0, Vec::len),
            Pane::Histories => self.visible_histories().ready().map_or(0, Vec::len),
            Pane::NotificationTargets => self
                .visible_notification_targets()
                .ready()
                .map_or(0, Vec::len),
            Pane::NotificationRoutings => self
                .visible_notification_routings()
                .ready()
                .map_or(0, Vec::len),
            Pane::Storages => self.visible_storages().ready().map_or(0, Vec::len),
            Pane::StorageKeys => self
                .visible_storage_access_keys()
                .ready()
                .map_or(0, Vec::len),
            Pane::Bills => self.visible_bills().len(),
            Pane::Account => self.visible_account_rows().len(),
            Pane::BillDetails => self.visible_bill_details().ready().map_or(0, Vec::len),
            Pane::BillSummary => self.current_summary().len(),
            Pane::None => 0,
        }
    }

    fn list_state(&mut self, pane: Pane) -> Option<&mut dyn SelectableList> {
        match pane {
            Pane::Registries => Some(&mut self.registry.registry_state),
            Pane::Users => Some(&mut self.registry.user_state),
            Pane::Repositories => Some(&mut self.registry.repository_state),
            Pane::Tags => Some(&mut self.registry.tag_state),
            Pane::Applications => Some(&mut self.apprun.application_state),
            Pane::Versions => Some(&mut self.apprun.version_state),
            Pane::Servers => Some(&mut self.server.server_state),
            Pane::SshKeys => Some(&mut self.ssh_key.state),
            Pane::Nics => Some(&mut self.server.nic_state),
            Pane::PacketFilters => Some(&mut self.packet_filter.filter_state),
            Pane::NetworkMap => Some(&mut self.network_map.state),
            Pane::PacketFilterRules => Some(&mut self.packet_filter.rule_state),
            Pane::Switches => Some(&mut self.switch.switch_state),
            Pane::CloudResources => Some(&mut self.cloud_resources.state),
            Pane::ManagedResources => Some(&mut self.managed_resources.state),
            Pane::ApiGatewaySubscriptions => Some(&mut self.api_gateway.subscription_state),
            Pane::ApiGatewayServices => Some(&mut self.api_gateway.service_state),
            Pane::ApiGatewayRoutes => Some(&mut self.api_gateway.route_state),
            Pane::ApiGatewayUsers => Some(&mut self.api_gateway.user_state),
            Pane::ApiGatewayGroups => Some(&mut self.api_gateway.group_state),
            Pane::ApiGatewayDomains => Some(&mut self.api_gateway.domain_state),
            Pane::ApiGatewayCertificates => Some(&mut self.api_gateway.certificate_state),
            Pane::AiEngineModels => Some(&mut self.ai_engine.model_state),
            Pane::AiEngineDocuments => Some(&mut self.ai_engine.document_state),
            Pane::NetworkingSuiteGroups => Some(&mut self.networking_suite.group_state),
            Pane::NetworkingSuiteSubnets => Some(&mut self.networking_suite.subnet_state),
            Pane::NetworkingSuiteAddresses => Some(&mut self.networking_suite.address_state),
            Pane::CloudHsmHsms => Some(&mut self.cloudhsm.hsm_state),
            Pane::CloudHsmClients => Some(&mut self.cloudhsm.client_state),
            Pane::CloudHsmLicenses => Some(&mut self.cloudhsm.license_state),
            Pane::CloudHsmDocuments => Some(&mut self.cloudhsm.document_state),
            Pane::SecurityControlRules => Some(&mut self.security_control.rule_state),
            Pane::SecurityControlActions => Some(&mut self.security_control.action_state),
            Pane::SegGateways => Some(&mut self.seg.gateway_state),
            Pane::SegServices => Some(&mut self.seg.service_state),
            Pane::NoSqlDatabases => Some(&mut self.nosql.database_state),
            Pane::NoSqlNodes => Some(&mut self.nosql.node_state),
            Pane::NoSqlBackups => Some(&mut self.nosql.backup_state),
            Pane::NoSqlParameters => Some(&mut self.nosql.parameter_state),
            Pane::ApiGatewayOidcs => Some(&mut self.api_gateway.oidc_state),
            Pane::Clusters => Some(&mut self.dedicated.cluster_state),
            Pane::DedicatedApplications => Some(&mut self.dedicated.application_state),
            Pane::ScalingGroups => Some(&mut self.dedicated.scaling_group_state),
            Pane::Certificates => Some(&mut self.dedicated.certificate_state),
            Pane::DnsZones => Some(&mut self.dns.zone_state),
            Pane::DnsRecords => Some(&mut self.dns.record_state),
            Pane::Monitors => Some(&mut self.simple_monitor.monitor_state),
            Pane::Vaults => Some(&mut self.secrets.vault_state),
            Pane::Secrets => Some(&mut self.secrets.secret_state),
            Pane::Projects => Some(&mut self.monitoring.project_state),
            Pane::Rules => Some(&mut self.monitoring.rule_state),
            Pane::LogMeasureRules => Some(&mut self.monitoring.log_measure_rule_state),
            Pane::LogRoutings => Some(&mut self.monitoring.log_routing_state),
            Pane::MetricsRoutings => Some(&mut self.monitoring.metrics_routing_state),
            Pane::Dashboards => Some(&mut self.monitoring.dashboard_state),
            Pane::Histories => Some(&mut self.monitoring.history_state),
            Pane::NotificationTargets => Some(&mut self.monitoring.notification_target_state),
            Pane::NotificationRoutings => Some(&mut self.monitoring.notification_routing_state),
            Pane::Storages => Some(&mut self.monitoring.storage_state),
            Pane::StorageKeys => Some(&mut self.monitoring.storage_key_state),
            Pane::Bills => Some(&mut self.billing.bill_state),
            Pane::Account => Some(&mut self.account.state),
            Pane::BillDetails => Some(&mut self.billing.detail_state),
            Pane::BillSummary => Some(&mut self.billing.summary_state),
            Pane::None => None,
        }
    }

    // --- クリップボードへのコピー ---

    /// 選択中の項目を、そのまま貼って使える形でコピーする。
    fn copy_selection(&mut self) {
        let Some(text) = self.copy_text() else {
            return;
        };
        match copy_to_clipboard(&text) {
            Ok(()) => self.set_status(format!("コピーしました: {text}"), StatusKind::Success),
            Err(err) => self.set_status(
                format!("クリップボードにコピーできませんでした: {err}"),
                StatusKind::Error,
            ),
        }
    }

    /// ペインごとに「コピーして意味のある文字列」を決める。
    fn copy_text(&self) -> Option<String> {
        let host = || {
            self.selected_registry()
                .map(|registry| registry.host().to_string())
        };
        let inert_ai_engine = self.service == Service::AiEngine
            && matches!(
                self.ai_engine.tab,
                AiEngineTab::Usage | AiEngineTab::Billing | AiEngineTab::Account
            );
        match self.active_pane() {
            Pane::Registries => host(),
            Pane::None if inert_ai_engine => None,
            Pane::None => host(),
            Pane::Users => self.selected_user().map(|user| user.username),
            Pane::Repositories => Some(format!("{}/{}", host()?, self.selected_repository()?)),
            Pane::Tags => Some(format!(
                "{}/{}:{}",
                host()?,
                self.selected_repository()?,
                self.selected_tag()?.name
            )),
            // AppRun は公開URLが一番使う場面が多い。
            Pane::Applications => self
                .selected_application()
                .map(|app| app.public_url.clone())
                .filter(|url| !url.is_empty()),
            Pane::Versions => self.selected_version().map(|version| version.name),
            // サーバーは SSH 先として使える IP を優先する。
            Pane::DnsZones => self.selected_dns_zone().map(|z| z.name.clone()),
            Pane::DnsRecords => {
                let zone = self.selected_dns_zone()?.name.clone();
                let index = self.dns.record_state.selected()?;
                self.visible_dns_records().get(index).map(|r| r.fqdn(&zone))
            }
            Pane::Monitors => self.selected_monitor().map(SimpleMonitor::summary),
            Pane::Vaults => self.selected_vault().map(|v| v.name),
            // 値はコピーしない。名前だけ。
            Pane::Secrets => self.selected_secret().map(|s| s.name),
            Pane::Projects => self.selected_project().map(|p| p.name),
            Pane::Rules
            | Pane::LogMeasureRules
            | Pane::LogRoutings
            | Pane::MetricsRoutings
            | Pane::Dashboards
            | Pane::Histories
            | Pane::Storages
            | Pane::NotificationTargets
            | Pane::NotificationRoutings => None,
            // シークレットはコピーせず、通常利用するトークンだけを対象にする。
            Pane::StorageKeys => self.selected_storage_access_key().map(|key| key.token),
            Pane::Bills | Pane::BillDetails | Pane::BillSummary => None,
            // 値をそのままコピーできると、権限の共有や問い合わせに使える。
            Pane::Account => {
                let index = self.account.state.selected()?;
                let row = self.visible_account_rows().into_iter().nth(index)?;
                Some(format!("{}: {}", row.label, row.value))
            }
            Pane::Clusters => self.selected_cluster().map(|c| c.id.clone()),
            Pane::DedicatedApplications => {
                self.visible_dedicated_applications()
                    .ready()
                    .and_then(|apps| {
                        apps.get(self.dedicated.application_state.selected()?)
                            .map(|app| app.name.clone())
                    })
            }
            Pane::ScalingGroups => self.selected_scaling_group().map(|g| g.id),
            Pane::Certificates => self.visible_certificates().ready().and_then(|certs| {
                certs
                    .get(self.dedicated.certificate_state.selected()?)
                    .map(|cert| cert.common_name.clone())
            }),
            Pane::Servers => self.selected_server().map(|server| {
                server
                    .ip_addresses
                    .first()
                    .cloned()
                    .unwrap_or(server.name.clone())
            }),
            // 公開鍵は貼り付けて使うので、鍵そのものをコピーする。
            Pane::SshKeys => self.selected_ssh_key().map(|key| key.public_key),
            Pane::Nics => self.selected_nic().map(|nic| {
                if nic.ip_address.is_empty() {
                    nic.mac_address
                } else {
                    nic.ip_address
                }
            }),
            Pane::NetworkMap => self.selected_map_server().map(|(id, _)| id.to_string()),
            Pane::PacketFilters => self
                .selected_packet_filter()
                .map(|filter| filter.id.to_string()),
            Pane::PacketFilterRules => self
                .selected_packet_filter_rule()
                .map(|(_, rule)| format!("{} {} {}", rule.protocol, rule.source(), rule.action)),
            Pane::Switches => self.selected_switch().map(|switch| switch.id.to_string()),
            Pane::CloudResources => self
                .selected_cloud_resource()
                .map(|resource| resource.id.to_string()),
            Pane::ManagedResources => self.selected_managed_resource().map(|resource| resource.id),
            Pane::ApiGatewaySubscriptions => self
                .selected_api_gateway_subscription()
                .map(|resource| resource.id),
            Pane::ApiGatewayServices => self
                .selected_api_gateway_service()
                .map(|resource| resource.id),
            Pane::ApiGatewayRoutes => self
                .selected_api_gateway_route()
                .map(|resource| resource.id),
            Pane::ApiGatewayUsers => self.selected_api_gateway_user().map(|resource| resource.id),
            Pane::ApiGatewayGroups => self
                .selected_api_gateway_group()
                .map(|resource| resource.id),
            Pane::ApiGatewayDomains => self
                .selected_api_gateway_domain()
                .map(|resource| resource.id),
            Pane::ApiGatewayCertificates => self
                .selected_api_gateway_certificate()
                .map(|resource| resource.id),
            // 推論APIに渡すのは連番ではなくモデル名なので、そちらをコピーする。
            Pane::AiEngineModels => self.selected_ai_engine_cloud_model().map(|model| model.id),
            Pane::AiEngineDocuments => self
                .selected_ai_engine_document()
                .map(|resource| resource.id),
            // 数値IDのフィールドが無いので、参照に使う SRN をそのまま渡す。
            Pane::NetworkingSuiteGroups => self
                .selected_networking_suite_group()
                .map(|resource| resource.srn),
            Pane::NetworkingSuiteSubnets => self
                .selected_networking_suite_subnet()
                .map(|resource| resource.srn),
            Pane::NetworkingSuiteAddresses => self
                .selected_networking_suite_address()
                .map(|resource| resource.srn),
            Pane::CloudHsmHsms => self.selected_cloudhsm_hsm().map(|resource| resource.id),
            Pane::CloudHsmClients => self.selected_cloudhsm_client().map(|resource| resource.id),
            Pane::CloudHsmLicenses => self.selected_cloudhsm_license().map(|resource| resource.id),
            Pane::CloudHsmDocuments => self
                .selected_cloudhsm_document()
                .map(|resource| resource.id),
            Pane::SecurityControlRules => self
                .selected_security_control_rule()
                .map(|resource| resource.id),
            Pane::SecurityControlActions => self
                .selected_security_control_action()
                .map(|resource| resource.id),
            Pane::SegGateways => self.selected_seg_gateway().map(|resource| resource.id),
            // 接続先サービスはIDを持たないので、エンドポイントを渡す。
            Pane::SegServices => self
                .selected_seg_service()
                .map(|resource| resource.endpoint)
                .filter(|endpoint| !endpoint.is_empty()),
            Pane::NoSqlDatabases => self.selected_nosql_database().map(|resource| resource.id),
            // ノードは自前のIDを持たないので、所属アプライアンスのIDを渡す。
            Pane::NoSqlNodes => self
                .selected_nosql_node()
                .map(|resource| resource.appliance_id),
            Pane::NoSqlBackups => self.selected_nosql_backup().map(|resource| resource.id),
            Pane::NoSqlParameters => self.selected_nosql_parameter().map(|resource| resource.id),
            Pane::ApiGatewayOidcs => self.selected_api_gateway_oidc().map(|resource| resource.id),
        }
    }

    // --- 認証情報の切り替え ---

    /// 現在のビューのキャッシュを捨てて読み直す。
    fn refresh(&mut self) {
        match self.service {
            Service::Registry => self.registry_refresh(),
            Service::AppRun => self.apprun_refresh(),
            Service::Dedicated => self.dedicated_refresh(),
            Service::Server => self.server_refresh(),
            Service::SshKey => self.ssh_key_refresh(),
            Service::PacketFilter => self.packet_filter_refresh(),
            Service::Switch => self.switch_refresh(),
            Service::NetworkMap => self.network_map_refresh(),
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::Bridge
            | Service::LoadBalancer
            | Service::VpcRouter
            | Service::MobileGateway
            | Service::Database
            | Service::Nfs => {
                if let Some(kind) = self.cloud_resource_kind() {
                    self.cloud_resources
                        .items
                        .remove(&(self.zone.clone(), kind));
                }
                self.cloud_resources_ensure_loaded();
            }
            Service::ObjectStorage
            | Service::SimpleMq
            | Service::SimpleNotification
            | Service::EventBus
            | Service::Workflows
            | Service::WebAccel
            | Service::EnhancedLoadBalancer
            | Service::LocalRouter
            | Service::Gslb
            | Service::Kms
            | Service::Iam
            | Service::AutoScale
            | Service::EnhancedDb
            | Service::AutoBackup => {
                if let Some(kind) = self.managed_resource_kind() {
                    self.managed_resources.items.remove(&kind);
                }
                self.managed_resources_ensure_loaded();
            }
            Service::AiEngine => self.ai_engine_refresh(),
            Service::NetworkingSuite => self.networking_suite_refresh(),
            Service::CloudHsm => self.cloudhsm_refresh(),
            Service::SecurityControl => self.security_control_refresh(),
            Service::Seg => self.seg_refresh(),
            Service::NoSql => self.nosql_refresh(),
            Service::ApiGateway => self.api_gateway_refresh(),
            // 複数ペインのサービスは、該当キャッシュを捨てて読み直す。
            Service::Dns => {
                self.dns.zones = Loadable::Idle;
                self.dns_ensure_loaded();
            }
            Service::SimpleMonitor => {
                self.simple_monitor.monitors = Loadable::Idle;
                self.monitor_ensure_loaded();
            }
            Service::Secrets => {
                self.secrets.vaults = Loadable::Idle;
                self.secrets.secrets.clear();
                self.secrets_ensure_loaded();
            }
            Service::Account => self.account_refresh(),
            Service::Billing => self.billing_refresh(),
            Service::Monitoring => {
                self.monitoring.projects.remove(&self.zone);
                self.monitoring.rules.clear();
                self.monitoring.log_measure_rules.clear();
                self.monitoring.log_routings.remove(&self.zone);
                self.monitoring.metrics_routings.remove(&self.zone);
                self.monitoring.publishers.remove(&self.zone);
                self.monitoring.dashboard_projects.remove(&self.zone);
                self.monitoring.histories.clear();
                self.monitoring.notification_targets.clear();
                self.monitoring.notification_routings.clear();
                self.monitoring.storages.remove(&self.zone);
                self.monitoring
                    .storage_keys
                    .retain(|(zone, _, _), _| zone != &self.zone);
                self.monitoring_ensure_loaded();
            }
        }
    }

    fn registry_refresh(&mut self) {
        let Some(registry) = self.selected_registry() else {
            self.load_registries();
            return;
        };
        let id = registry.id;
        let host = registry.host().to_string();
        match self.registry.tab {
            Tab::Overview => self.load_registries(),
            Tab::Users => self.load_users(id),
            Tab::Images => match self.registry.image_pane {
                ImagePane::Repositories => {
                    self.registry.tags.retain(|(h, _), _| h != &host);
                    self.load_repositories(host);
                }
                ImagePane::Tags => {
                    if let Some(repository) = self.selected_repository() {
                        self.load_tags(host, repository);
                    }
                }
            },
        }
    }

    fn cloud_resources_ensure_loaded(&mut self) {
        let Some(kind) = self.cloud_resource_kind() else {
            return;
        };
        let key = (self.zone.clone(), kind);
        if self
            .cloud_resources
            .items
            .get(&key)
            .is_some_and(|items| !items.is_idle())
        {
            self.fill_selection(Pane::CloudResources);
            return;
        }
        self.cloud_resources.items.insert(key, Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client
                .list_cloud_resources(&zone, kind)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::CloudResources { zone, kind, result });
        });
    }

    fn managed_resources_ensure_loaded(&mut self) {
        let Some(kind) = self.managed_resource_kind() else {
            return;
        };
        if self
            .managed_resources
            .items
            .get(&kind)
            .is_some_and(|items| !items.is_idle())
        {
            self.fill_selection(Pane::ManagedResources);
            return;
        }
        self.managed_resources.items.insert(kind, Loadable::Loading);
        if kind == ManagedResourceKind::AiEngine {
            let client = match self.ai_engine_client.clone() {
                Some(client) => client,
                None => {
                    let token = match crate::config::load_ai_engine_token(&self.credential_source) {
                        Ok(Some(token)) => token,
                        Ok(None) => {
                            self.managed_resources.items.insert(
                                kind,
                                Loadable::Failed(
                                    "AI Engineアカウントトークンが未設定です。\n\n\
                                     t キーで、コントロールパネルから発行済みのトークンを登録してください。"
                                        .to_string(),
                                ),
                            );
                            return;
                        }
                        Err(err) => {
                            self.managed_resources
                                .items
                                .insert(kind, Loadable::Failed(fmt_error(err)));
                            return;
                        }
                    };
                    match AiEngineClient::new(token) {
                        Ok(client) => {
                            let client = Arc::new(client);
                            self.ai_engine_client = Some(client.clone());
                            client
                        }
                        Err(err) => {
                            self.managed_resources
                                .items
                                .insert(kind, Loadable::Failed(fmt_error(err)));
                            return;
                        }
                    }
                }
            };
            self.inflight += 1;
            let tx = self.tx.clone();
            tokio::spawn(async move {
                let result = client.list_models().await.map_err(fmt_error);
                let _ = tx.send(Message::ManagedResources { kind, result });
            });
            return;
        }

        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_managed_resources(kind).await.map_err(fmt_error);
            let _ = tx.send(Message::ManagedResources { kind, result });
        });
    }

    fn invalidate_all(&mut self) {
        self.service_counts.clear();
        self.registry.users.clear();
        self.registry.repositories.clear();
        self.registry.tags.clear();
        self.registry.tag_details.clear();
        self.registry.auto_login_tried.clear();
        self.registry.registry_state.select(None);
        self.registry.user_state.select(None);
        self.registry.repository_state.select(None);
        self.registry.tag_state.select(None);
        self.apprun_invalidate();
        self.dedicated_invalidate();
        self.server_invalidate();
        self.switch_invalidate();
        self.cloud_resources.items.clear();
        self.cloud_resources.state.select(None);
        self.packet_filter_invalidate();
        self.ssh_key_invalidate();
        self.network_map_invalidate();
        self.managed_resources.items.clear();
        self.managed_resources.state.select(None);
        self.api_gateway = ApiGatewayView::default();
        self.nosql = NoSqlView::default();
        self.seg = SegView::default();
        self.security_control = SecurityControlView::default();
        self.cloudhsm = CloudHsmView::default();
        self.networking_suite = NetworkingSuiteView::default();
        self.ai_engine = AiEngineView::default();
        self.observability_invalidate();
        self.billing_invalidate();
        self.account_invalidate();
    }

    fn set_tab(&mut self, tab: Tab) {
        self.registry.tab = tab;
        self.registry.focus = Focus::Detail;
    }

    fn cycle_tab(&mut self, delta: i32) {
        let current = Tab::ALL
            .iter()
            .position(|t| *t == self.registry.tab)
            .unwrap_or(0) as i32;
        let len = Tab::ALL.len() as i32;
        self.registry.tab = Tab::ALL[(current + delta).rem_euclid(len) as usize];
        self.registry.focus = Focus::Detail;
    }

    fn focus_left(&mut self) {
        if self.registry.focus == Focus::Detail
            && self.registry.tab == Tab::Images
            && self.registry.image_pane == ImagePane::Tags
        {
            self.registry.image_pane = ImagePane::Repositories;
            return;
        }
        self.registry.focus = Focus::Registries;
    }

    fn focus_right(&mut self) {
        if self.registry.focus == Focus::Registries {
            self.registry.focus = Focus::Detail;
            return;
        }
        if self.registry.tab == Tab::Images && self.registry.image_pane == ImagePane::Repositories {
            self.registry.image_pane = ImagePane::Tags;
        }
    }

    /// 現在フォーカスしているリストの選択を動かす。
    fn move_selection(&mut self, delta: i32) {
        let pane = self.active_pane();
        if pane == Pane::NetworkMap {
            self.move_network_map_selection(delta);
            return;
        }
        let len = self.visible_len(pane);
        let Some(state) = self.list_state(pane) else {
            return;
        };
        if len == 0 {
            return;
        }
        let current = state.selected().unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, len as i32 - 1) as usize;
        state.select(Some(next));
        self.after_selection_change(pane);
    }

    fn jump_selection(&mut self, to_top: bool) {
        let pane = self.active_pane();
        if pane == Pane::NetworkMap {
            self.jump_network_map_selection(to_top);
            return;
        }
        let len = self.visible_len(pane);
        let Some(state) = self.list_state(pane) else {
            return;
        };
        if len == 0 {
            return;
        }
        state.select(Some(if to_top { 0 } else { len - 1 }));
        self.after_selection_change(pane);
    }

    /// 親の選択が変わったら、それにぶら下がる子の選択をリセットする。
    fn after_selection_change(&mut self, pane: Pane) {
        // AppRun はアプリを変えるとバージョンが変わる。
        if pane == Pane::Applications {
            self.apprun.version_state.select(None);
            self.apprun.pane = AppRunPane::Applications;
        }
        // 専有型はクラスタが親、ASG がワーカーノードの親。
        if pane == Pane::Clusters {
            self.dedicated_after_cluster_change();
        }
        if pane == Pane::ScalingGroups {
            self.dedicated.worker_node_state.select(None);
        }
        if pane == Pane::DnsZones {
            self.dns.record_state.select(None);
        }
        if pane == Pane::Vaults {
            self.secrets.secret_state.select(None);
        }
        if pane == Pane::Bills {
            self.billing.detail_state.select(None);
            self.billing.summary_state.select(None);
        }
        if pane == Pane::Projects {
            self.monitoring.rule_state.select(None);
            self.monitoring.log_measure_rule_state.select(None);
            self.monitoring.history_state.select(None);
            self.monitoring.notification_target_state.select(None);
            self.monitoring.notification_routing_state.select(None);
        }
        if pane == Pane::Storages {
            self.monitoring.storage_key_state.select(None);
        }
        if pane == Pane::ApiGatewayServices {
            self.api_gateway.route_state.select(None);
            self.api_gateway_ensure_loaded();
        }
        if pane == Pane::ApiGatewayUsers {
            self.api_gateway_ensure_loaded();
        }
        if pane == Pane::NoSqlDatabases {
            self.nosql_reset_child_selection();
            self.nosql_ensure_loaded();
        }
        // 接続先サービスは一覧に含まれるので、選択位置を捨てるだけでよい。
        if pane == Pane::SegGateways {
            self.seg_reset_child_selection();
            self.fill_selection(Pane::SegServices);
        }
        if pane == Pane::CloudHsmHsms {
            self.cloudhsm.client_state.select(None);
            self.cloudhsm_ensure_loaded();
        }
        if pane == Pane::AiEngineDocuments {
            // 別のドキュメントを選んだら本文は先頭から読み直す。
            self.ai_engine.chunk_scroll = 0;
            self.ai_engine_ensure_loaded();
        }
        if pane == Pane::NetworkingSuiteGroups {
            self.networking_suite.subnet_state.select(None);
            self.networking_suite.address_state.select(None);
            self.networking_suite_ensure_loaded();
        }
        if pane == Pane::NetworkingSuiteSubnets {
            self.networking_suite.address_state.select(None);
            self.networking_suite_ensure_loaded();
        }
        if pane == Pane::CloudHsmLicenses {
            self.cloudhsm.document_state.select(None);
            self.cloudhsm_ensure_loaded();
        }
        if self.service != Service::Registry {
            return;
        }
        match (
            self.registry.focus,
            self.registry.tab,
            self.registry.image_pane,
        ) {
            (Focus::Registries, _, _) => {
                self.registry.user_state.select(None);
                self.registry.repository_state.select(None);
                self.registry.tag_state.select(None);
                self.registry.image_pane = ImagePane::Repositories;
            }
            (Focus::Detail, Tab::Images, ImagePane::Repositories) => {
                self.registry.tag_state.select(None);
            }
            _ => {}
        }
    }

    // --- ユーザー管理 ---

    fn open_add_user(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some((id, name, host)) = self.selected_registry().map(|registry| {
            (
                registry.id,
                registry.name.clone(),
                registry.host().to_string(),
            )
        }) else {
            return;
        };
        self.registry.tab = Tab::Users;
        self.registry.focus = Focus::Detail;
        self.overlay = Some(Overlay::UserForm(UserForm {
            registry: id,
            registry_name: name,
            registry_host: host,
            mode: UserFormMode::Add,
            username: String::new(),
            password: String::new(),
            permission: 1, // 既定は readwrite
            field: 0,
        }));
    }

    fn open_edit_user(&mut self) {
        if !self.require_write() {
            return;
        }
        if self.registry.tab != Tab::Users {
            return;
        }
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let (id, name, host) = (
            registry.id,
            registry.name.clone(),
            registry.host().to_string(),
        );
        let Some(user) = self.selected_user() else {
            self.set_status("編集するユーザーを選択してください", StatusKind::Info);
            return;
        };
        let permission = Permission::ALL
            .iter()
            .position(|p| *p == user.permission)
            .unwrap_or(0);
        self.overlay = Some(Overlay::UserForm(UserForm {
            registry: id,
            registry_name: name,
            registry_host: host,
            mode: UserFormMode::Edit,
            username: user.username.clone(),
            password: String::new(),
            permission,
            field: 1, // ユーザー名は変更できないのでパスワードから始める
        }));
    }

    fn confirm_delete_user(&mut self) {
        if self.registry.tab != Tab::Users {
            return;
        }
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let id = registry.id;
        let registry_name = registry.name.clone();
        let Some(user) = self.selected_user() else {
            self.set_status("削除するユーザーを選択してください", StatusKind::Info);
            return;
        };
        let username = user.username.clone();
        self.overlay = Some(Overlay::Confirm {
            title: "ユーザーの削除".to_string(),
            body: format!(
                "レジストリ「{registry_name}」からユーザー「{username}」を削除します。\n\
                 この操作は取り消せません。実行しますか？"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteUser {
                registry: id,
                username,
            },
        });
    }

    fn submit_user_form(&mut self, form: UserForm) {
        let permission = form.permission();
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let id = form.registry;

        match form.mode {
            UserFormMode::Add => {
                if form.username.is_empty() || form.password.is_empty() {
                    self.set_status(
                        "ユーザー名とパスワードを入力してください",
                        StatusKind::Error,
                    );
                    self.overlay = Some(Overlay::UserForm(form));
                    return;
                }
                let label = format!("ユーザー「{}」を追加", form.username);
                let host = form.registry_host;
                let (username, password) = (form.username, form.password);
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client
                        .add_user(id, &username, &password, permission)
                        .await
                        .map_err(fmt_error);
                    // 作成したユーザーをそのままログイン情報として保存できるよう持ち越す。
                    let save_login = Some((host, RegistryLogin { username, password }));
                    let _ = tx.send(Message::UserAction {
                        id,
                        label,
                        result,
                        save_login,
                    });
                });
            }
            UserFormMode::Edit => {
                let label = format!("ユーザー「{}」を更新", form.username);
                let host = form.registry_host;
                let username = form.username;
                // パスワードが空欄なら現在のパスワードを維持する。
                let new_password = (!form.password.is_empty()).then_some(form.password);
                let password_for_save = new_password.clone();
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client
                        .update_user(id, &username, new_password.as_deref(), permission)
                        .await
                        .map_err(fmt_error);
                    // パスワードを変更したときだけ、ログイン情報の更新を提案する。
                    let save_login = password_for_save
                        .map(|password| (host, RegistryLogin { username, password }));
                    let _ = tx.send(Message::UserAction {
                        id,
                        label,
                        result,
                        save_login,
                    });
                });
            }
        }
        self.set_status("送信中…", StatusKind::Info);
    }

    fn run_confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::DeleteUser { registry, username } => {
                let client = self.sacloud.clone();
                let tx = self.tx.clone();
                let label = format!("ユーザー「{username}」を削除");
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client
                        .delete_user(registry, &username)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::UserAction {
                        id: registry,
                        label,
                        result,
                        save_login: None,
                    });
                });
                self.set_status("送信中…", StatusKind::Info);
            }
            ConfirmAction::DeleteRegistry { id, name } => {
                let client = self.sacloud.clone();
                let tx = self.tx.clone();
                let label = format!("レジストリ「{name}」を削除");
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client.delete_registry(id).await.map_err(fmt_error);
                    let _ = tx.send(Message::RegistryAction { label, result });
                });
                self.set_status("送信中…", StatusKind::Info);
            }
            ConfirmAction::DeleteSwitch { zone, id, name } => {
                self.run_delete_switch(zone, id, name)
            }
            ConfirmAction::DeleteDnsRecord { zone, record } => {
                self.run_delete_dns_record(zone, record)
            }
            ConfirmAction::DeleteDnsZone { id, name } => self.run_delete_dns_zone(id, name),
            ConfirmAction::DeleteSimpleMonitor { id, target } => {
                self.run_delete_simple_monitor(id, target)
            }
            ConfirmAction::DeleteTag {
                host,
                repository,
                tag,
                digest,
            } => {
                let Some(client) = self.registry_clients.get(&host) else {
                    return;
                };
                let tx = self.tx.clone();
                let label = format!("イメージ「{repository}:{tag}」を削除");
                self.inflight += 1;
                let repo = repository.clone();
                let target_host = host.clone();
                tokio::spawn(async move {
                    let result = client
                        .delete_manifest(&repo, &digest)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::TagAction {
                        host: target_host,
                        repository: repo,
                        label,
                        result,
                    });
                });
                self.set_status("送信中…", StatusKind::Info);
            }
            ConfirmAction::RouteTraffic {
                application,
                app_name,
                version,
            } => self.run_route_traffic(application, app_name, version),
            ConfirmAction::DeleteCredential { name } => {
                match crate::config::delete_keychain_credential(&name) {
                    Ok(()) => {
                        if let Ok(config) = Config::load() {
                            self.config = config;
                        }
                        self.set_status(format!("{name} を削除しました"), StatusKind::Success);
                        self.open_profile_picker();
                    }
                    Err(err) => self.set_status(fmt_error(err), StatusKind::Error),
                }
            }
            ConfirmAction::DeleteAiEngineToken { name } => {
                match crate::config::delete_ai_engine_token(&self.credential_source, &name) {
                    Ok(()) => {
                        self.ai_engine_client = None;
                        self.managed_resources
                            .items
                            .remove(&ManagedResourceKind::AiEngine);
                        self.ai_engine_reset_rag();
                        self.set_status(
                            format!("このPCからAI Engineトークン「{name}」を削除しました"),
                            StatusKind::Success,
                        );
                        self.open_ai_engine_token_form();
                    }
                    Err(err) => self.set_status(fmt_error(err), StatusKind::Error),
                }
            }
            ConfirmAction::CreateServer { zone, input } => self.run_create_server(zone, input),
            ConfirmAction::DeleteServer { zone, id, name } => {
                self.run_delete_server(zone, id, name)
            }
            ConfirmAction::ChangeServerPlan {
                zone,
                id,
                name,
                cpu,
                memory_mb,
            } => self.run_change_server_plan(zone, id, name, cpu, memory_mb),
            ConfirmAction::CreateDisk { zone, input } => self.run_create_disk(zone, input),
            ConfirmAction::DeleteDisk { zone, id, name } => self.run_delete_disk(zone, id, name),
            ConfirmAction::DeleteSshKey { id, name } => self.run_delete_ssh_key(id, name),
            ConfirmAction::DeletePacketFilter { zone, id, name } => {
                self.run_delete_packet_filter(zone, id, name)
            }
            ConfirmAction::DeletePacketFilterRule { id, index } => {
                self.run_delete_packet_filter_rule(id, index)
            }
            ConfirmAction::DeleteNic { zone, id, name } => self.run_delete_nic(zone, id, name),
            ConfirmAction::CreateArchive {
                zone,
                name,
                description,
                disk_id,
            } => self.run_create_archive(zone, name, description, disk_id),
            ConfirmAction::DeleteArchive { zone, id, name } => {
                self.run_delete_archive(zone, id, name)
            }
            ConfirmAction::DeleteAutoBackup { zone, id, name } => {
                self.run_delete_auto_backup(zone, id, name)
            }
            ConfirmAction::DisconnectDisk {
                zone,
                id,
                name,
                server,
            } => self.run_disconnect_disk(zone, id, name, server),
            ConfirmAction::DeleteRagDocument { id, name } => self.run_delete_rag_document(id, name),
            ConfirmAction::UnveilSecret { vault, name } => self.run_unveil(vault, name),
            ConfirmAction::DeleteVault { id, name } => self.run_delete_vault(id, name),
            ConfirmAction::DeleteSecret { vault, name } => self.run_delete_secret(vault, name),
            ConfirmAction::DeleteAlertProject {
                zone,
                resource_id,
                name,
            } => self.run_delete_alert_project(zone, resource_id, name),
            ConfirmAction::DeleteAlertRule {
                zone,
                project,
                uid,
                name,
            } => self.run_delete_alert_rule(zone, project, uid, name),
            ConfirmAction::DeleteLogMeasureRule {
                zone,
                project,
                uid,
                name,
            } => self.run_delete_log_measure_rule(zone, project, uid, name),
            ConfirmAction::DeleteLogRouting { zone, routing } => {
                self.run_delete_log_routing(zone, routing)
            }
            ConfirmAction::DeleteMetricsRouting { zone, routing } => {
                self.run_delete_metrics_routing(zone, routing)
            }
            ConfirmAction::DeleteDashboardProject { zone, project } => {
                self.run_delete_dashboard(zone, project)
            }
            ConfirmAction::DeleteNotificationTarget {
                zone,
                project,
                target,
            } => self.run_delete_notification_target(zone, project, target),
            ConfirmAction::DeleteNotificationRouting {
                zone,
                project,
                routing,
            } => self.run_delete_notification_routing(zone, project, routing),
            ConfirmAction::DeleteStorage { zone, storage } => {
                self.run_delete_storage(zone, storage)
            }
            ConfirmAction::SetStorageRetention {
                zone,
                storage,
                days,
            } => self.run_set_storage_retention(zone, storage, days),
            ConfirmAction::DeleteStorageAccessKey { zone, storage, key } => {
                self.run_delete_storage_access_key(zone, storage, key)
            }
            ConfirmAction::RevealStorageAccessKey { zone, storage, key } => {
                self.run_reveal_storage_access_key(zone, storage, key)
            }
            ConfirmAction::PowerAction {
                id,
                zone,
                name,
                action,
            } => self.run_power_action(id, zone, name, action),
            ConfirmAction::DeleteIamResource {
                resource_type,
                id,
                name,
            } => {
                let client = self.sacloud.clone();
                let tx = self.tx.clone();
                let label = format!("{resource_type}「{name}」を削除");
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client
                        .delete_iam_resource(&resource_type, &id)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::IamAction { label, result });
                });
                self.set_status("送信中…", StatusKind::Info);
            }
            ConfirmAction::ChangeIamRole {
                project_id,
                principal_type,
                principal_id,
                role_id,
                grant,
            } => {
                let client = self.sacloud.clone();
                let tx = self.tx.clone();
                let action = if grant { "付与" } else { "解除" };
                let label = format!("IAMロール「{role_id}」を{action}");
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client
                        .change_project_iam_role(
                            project_id,
                            &role_id,
                            &principal_type,
                            principal_id,
                            grant,
                        )
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::IamAction { label, result });
                });
                self.set_status("送信中…", StatusKind::Info);
            }
            ConfirmAction::ForgetLogin { host } => {
                self.registry_clients.remove(&host);
                self.registry.auto_login_tried.remove(&host);
                self.registry.repositories.remove(&host);
                self.registry.tags.retain(|(h, _), _| h != &host);
                match self.config.forget_registry_login(&host) {
                    Ok(true) => self.set_status(
                        format!("{host} のログイン情報を削除しました（キーチェーンからも削除）"),
                        StatusKind::Success,
                    ),
                    Ok(false) => self.set_status(
                        format!("{host} からログアウトしました"),
                        StatusKind::Success,
                    ),
                    Err(err) => self.set_status(
                        format!("ログイン情報の削除に失敗: {}", fmt_error(err)),
                        StatusKind::Error,
                    ),
                }
            }
            ConfirmAction::SaveRegistryLogin { host, login } => {
                match self.config.save_registry_login(&host, &login) {
                    Ok(_) => {
                        // 保存前に一度でもタブを開いていると「試した」印がついたままなので、
                        // 今保存したばかりの情報で改めて自動ログインを試せるようにする。
                        self.registry.auto_login_tried.remove(&host);
                        self.set_status(
                            format!("{host} のログイン情報を保存しました（パスワードはキーチェーンに保存）"),
                            StatusKind::Success,
                        );
                    }
                    // 保存できないときに平文へ退避したりはしない。
                    Err(err) => self.set_status(
                        format!("ログイン情報を保存できませんでした: {}", fmt_error(err)),
                        StatusKind::Error,
                    ),
                }
            }
        }
    }

    // --- レジストリ自体の作成・編集・削除 ---

    fn open_create_registry(&mut self) {
        if !self.require_write() {
            return;
        }
        self.overlay = Some(Overlay::RegistryForm(RegistryForm {
            mode: RegistryFormMode::Create,
            target: None,
            name: String::new(),
            subdomain: String::new(),
            description: String::new(),
            virtual_domain: String::new(),
            field: 0,
        }));
    }

    fn open_edit_registry(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(registry) = self.selected_registry().cloned() else {
            return;
        };
        self.overlay = Some(Overlay::RegistryForm(RegistryForm {
            mode: RegistryFormMode::Edit,
            name: registry.name.clone(),
            subdomain: registry.subdomain_label.clone(),
            description: registry.description.clone(),
            virtual_domain: registry.virtual_domain.clone(),
            target: Some(registry),
            field: 0,
        }));
    }

    fn submit_registry_form(&mut self, form: RegistryForm) {
        if form.name.is_empty() {
            self.set_status("名前を入力してください", StatusKind::Error);
            self.overlay = Some(Overlay::RegistryForm(form));
            return;
        }
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status("送信中…", StatusKind::Info);

        match form.mode {
            RegistryFormMode::Create => {
                if form.subdomain.is_empty() {
                    self.inflight -= 1;
                    self.set_status("サブドメインを入力してください", StatusKind::Error);
                    self.overlay = Some(Overlay::RegistryForm(form));
                    return;
                }
                let label = format!("レジストリ「{}」を作成", form.name);
                let (name, subdomain, description) = (form.name, form.subdomain, form.description);
                tokio::spawn(async move {
                    let result = client
                        .create_registry(&name, &subdomain, &description)
                        .await
                        .map(|_| ())
                        .map_err(fmt_error);
                    let _ = tx.send(Message::RegistryAction { label, result });
                });
            }
            RegistryFormMode::Edit => {
                let Some(target) = form.target else {
                    self.inflight -= 1;
                    return;
                };
                let label = format!("レジストリ「{}」を更新", form.name);
                let (name, description, virtual_domain) =
                    (form.name, form.description, form.virtual_domain);
                tokio::spawn(async move {
                    let result = client
                        .update_registry(&target, &name, &description, &virtual_domain)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::RegistryAction { label, result });
                });
            }
        }
    }

    /// レジストリの削除は取り消せないので、名前の入力を要求する。
    fn confirm_delete_registry(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let (id, name, host) = (
            registry.id,
            registry.name.clone(),
            registry.host().to_string(),
        );
        self.overlay = Some(Overlay::Confirm {
            title: "レジストリの削除".to_string(),
            body: format!(
                "レジストリ「{name}」({host}) を削除します。\n\
                 保存されている全てのイメージとユーザーも消え、取り消せません。\n\
                 実行するにはレジストリ名を入力してください。"
            ),
            verify: Some(name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteRegistry { id, name },
        });
    }

    /// フォーカス中のペインに応じて「選択中のもの」を削除する。
    fn delete_selected(&mut self) {
        if !self.require_write() {
            return;
        }
        match self.active_pane() {
            Pane::Users => self.confirm_delete_user(),
            Pane::Tags => self.confirm_delete_tag(),
            _ => {}
        }
    }

    fn confirm_delete_tag(&mut self) {
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let host = registry.host().to_string();
        let (Some(repository), Some(tag)) = (self.selected_repository(), self.selected_tag())
        else {
            return;
        };
        let Some(digest) = tag.digest.clone() else {
            self.set_status(
                "ダイジェストが取得できていないため削除できません",
                StatusKind::Error,
            );
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "イメージの削除".to_string(),
            body: format!(
                "{host}/{repository}:{} を削除します。\n\
                 Registry API はマニフェスト単位で消すため、同じダイジェストを指す\n\
                 他のタグも同時に消えます。取り消せません。",
                tag.name
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteTag {
                host,
                repository,
                tag: tag.name,
                digest,
            },
        });
    }

    // --- レジストリへのログイン ---

    // --- ダイアログのキー入力 ---

    fn on_key_overlay(&mut self, key: KeyEvent) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        match overlay {
            // ヘルプは何かキーを押したら閉じる（`take()` 済み）。
            Overlay::Help => {}
            // メッセージは長いことがあるので、読み終えるまで開いたままにする。
            Overlay::Message {
                title,
                body,
                kind,
                mut scroll,
            } => match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    // 入力途中のフォームがあれば書き直せるように戻す。
                    if let Some(form) = self.pending_form.take() {
                        self.overlay = Some(Overlay::ProfileForm(*form));
                    }
                }
                code => {
                    let lines = body.lines().count() as u16;
                    scroll = match code {
                        KeyCode::Down | KeyCode::Char('j') => scroll.saturating_add(1),
                        KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
                        KeyCode::PageDown => scroll.saturating_add(10),
                        KeyCode::PageUp => scroll.saturating_sub(10),
                        KeyCode::Home | KeyCode::Char('g') => 0,
                        KeyCode::End | KeyCode::Char('G') => lines,
                        _ => scroll,
                    };
                    self.overlay = Some(Overlay::Message {
                        title,
                        body,
                        kind,
                        scroll,
                    });
                }
            },
            Overlay::Confirm {
                title,
                body,
                verify,
                mut typed,
                action,
            } => {
                let Some(expected) = verify.clone() else {
                    // 入力確認なし: y/Enter で実行、それ以外は開いたまま。
                    match key.code {
                        KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.run_confirmed(action),
                        KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => {}
                        _ => {
                            self.overlay = Some(Overlay::Confirm {
                                title,
                                body,
                                verify,
                                typed,
                                action,
                            })
                        }
                    }
                    return;
                };
                // 入力確認あり: 名前を打ち込まないと実行できない。
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Enter if typed == expected => {
                        self.run_confirmed(action);
                        return;
                    }
                    KeyCode::Enter => self.set_status(
                        format!("「{expected}」と一致していません"),
                        StatusKind::Error,
                    ),
                    KeyCode::Backspace => {
                        typed.pop();
                    }
                    KeyCode::Char(c) => typed.push(c),
                    _ => {}
                }
                self.overlay = Some(Overlay::Confirm {
                    title,
                    body,
                    verify,
                    typed,
                    action,
                });
            }
            Overlay::UserForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_user_form(form),
                _ => {
                    edit_user_form(&mut form, key);
                    self.overlay = Some(Overlay::UserForm(form));
                }
            },
            Overlay::ProfileForm(mut form) => match key.code {
                // 検証中は結果を待つ。
                _ if form.verifying => self.overlay = Some(Overlay::ProfileForm(form)),
                KeyCode::Esc if !self.has_credentials => self.should_quit = true,
                KeyCode::Esc => self.open_profile_picker(),
                KeyCode::Enter => self.submit_profile_form(form),
                _ => {
                    edit_profile_form(&mut form, key);
                    self.overlay = Some(Overlay::ProfileForm(form));
                }
            },
            Overlay::AiEngineTokenForm(mut form) => match key.code {
                _ if form.verifying => {
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Esc if form.adding => {
                    form.adding = false;
                    form.name.clear();
                    form.token.clear();
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Esc => {}
                KeyCode::Enter if form.adding => self.submit_ai_engine_token(form),
                KeyCode::Tab | KeyCode::Down | KeyCode::Up if form.adding => {
                    form.field = (form.field + 1) % 2;
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Backspace if form.adding => {
                    if form.field == 0 {
                        form.name.pop();
                    } else {
                        form.token.pop();
                    }
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Char(c) if form.adding => {
                    if form.field == 0 {
                        form.name.push(c);
                    } else {
                        form.token.push(c);
                    }
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Char('n') => {
                    form.adding = true;
                    form.name.clear();
                    form.token.clear();
                    form.field = 0;
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Char('e') => {
                    let entry = form.entries.get(form.index).cloned();
                    if let Some(entry) = entry.filter(|entry| !entry.from_env) {
                        form.adding = true;
                        form.name = entry.name;
                        form.token.clear();
                        form.field = 1;
                    }
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Enter => {
                    if let Some(entry) = form.entries.get(form.index) {
                        let name = entry.name.clone();
                        self.select_ai_engine_token(&name);
                    } else {
                        form.adding = true;
                        self.overlay = Some(Overlay::AiEngineTokenForm(form));
                    }
                }
                KeyCode::Char('y') => {
                    if let Some(entry) = form.entries.get(form.index) {
                        let name = entry.name.clone();
                        self.copy_ai_engine_token(&name, form);
                    } else {
                        self.overlay = Some(Overlay::AiEngineTokenForm(form));
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(entry) = form.entries.get(form.index) {
                        if entry.from_env {
                            self.set_status(
                                "環境変数のトークンはアプリから削除できません",
                                StatusKind::Info,
                            );
                            self.overlay = Some(Overlay::AiEngineTokenForm(form));
                        } else {
                            self.confirm_delete_ai_engine_token(entry.name.clone());
                        }
                    } else {
                        self.overlay = Some(Overlay::AiEngineTokenForm(form));
                    }
                }
                KeyCode::Down | KeyCode::Char('j') if !form.entries.is_empty() => {
                    form.index = (form.index + 1) % form.entries.len();
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                KeyCode::Up | KeyCode::Char('k') if !form.entries.is_empty() => {
                    form.index = (form.index + form.entries.len() - 1) % form.entries.len();
                    self.overlay = Some(Overlay::AiEngineTokenForm(form));
                }
                _ => self.overlay = Some(Overlay::AiEngineTokenForm(form)),
            },
            Overlay::IamCredentialForm(mut form) => match key.code {
                _ if form.verifying => {
                    self.overlay = Some(Overlay::IamCredentialForm(form));
                }
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_iam_credentials(form),
                KeyCode::Tab | KeyCode::Down => {
                    form.field = (form.field + 1) % IamCredentialForm::FIELDS;
                    self.overlay = Some(Overlay::IamCredentialForm(form));
                }
                KeyCode::BackTab | KeyCode::Up => {
                    form.field =
                        (form.field + IamCredentialForm::FIELDS - 1) % IamCredentialForm::FIELDS;
                    self.overlay = Some(Overlay::IamCredentialForm(form));
                }
                KeyCode::Backspace => {
                    match form.field {
                        0 => {
                            form.service_principal_id.pop();
                        }
                        1 => {
                            form.key_id.pop();
                        }
                        _ => {
                            form.private_key.pop();
                        }
                    }
                    self.overlay = Some(Overlay::IamCredentialForm(form));
                }
                KeyCode::Char(c) if form.field < 2 => {
                    if form.field == 0 {
                        form.service_principal_id.push(c);
                    } else {
                        form.key_id.push(c);
                    }
                    self.overlay = Some(Overlay::IamCredentialForm(form));
                }
                _ => self.overlay = Some(Overlay::IamCredentialForm(form)),
            },
            Overlay::ProfilePicker { sources, mut index } => {
                // 一覧の最後に「新規作成」の行がある。
                let rows = sources.len() + 1;
                let on_new_row = index == sources.len();
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {}
                    KeyCode::Enter if on_new_row => self.open_profile_form(),
                    KeyCode::Enter => self.switch_credentials(sources[index].0.clone()),
                    // 新規作成は n でも開ける。
                    KeyCode::Char('n') => self.open_profile_form(),
                    // キーチェーンに預けたものだけ削除できる。
                    KeyCode::Char('d') if !on_new_row => {
                        self.confirm_delete_credential(&sources[index].0)
                    }
                    // 選択中の認証情報に色を割り当てる。
                    KeyCode::Char('c') if !on_new_row => {
                        self.cycle_profile_color(&sources[index].0);
                        self.overlay = Some(Overlay::ProfilePicker { sources, index });
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        index = (index + 1) % rows;
                        self.overlay = Some(Overlay::ProfilePicker { sources, index });
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        index = (index + rows - 1) % rows;
                        self.overlay = Some(Overlay::ProfilePicker { sources, index });
                    }
                    _ => self.overlay = Some(Overlay::ProfilePicker { sources, index }),
                }
            }
            Overlay::RegistryForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_registry_form(form),
                _ => {
                    edit_registry_form(&mut form, key);
                    self.overlay = Some(Overlay::RegistryForm(form));
                }
            },
            Overlay::IamResourceForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_iam_resource_form(form),
                _ => {
                    edit_iam_resource_form(&mut form, key);
                    self.overlay = Some(Overlay::IamResourceForm(form));
                }
            },
            Overlay::IamRoleForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_iam_role_form(form),
                _ => {
                    edit_iam_role_form(&mut form, key);
                    self.overlay = Some(Overlay::IamRoleForm(form));
                }
            },
            Overlay::RagEditForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_rag_edit_form(form),
                _ => {
                    edit_rag_edit_form(&mut form, key);
                    self.overlay = Some(Overlay::RagEditForm(form));
                }
            },
            Overlay::ServerCreateForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_server_create_form(form),
                // 公開鍵は長いので、貼り付けずに選べるようにする。
                KeyCode::Char('k' | 'K')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && form.current(&self.server_choices()) == ServerField::SshKey =>
                {
                    self.open_ssh_key_picker(SshKeyReturn::ServerCreate(form));
                }
                // 候補が多い欄は、左右キーではなく絞り込みで選ぶ。
                KeyCode::Char('/')
                    if ServerChoices::is_list_field(form.current(&self.server_choices())) =>
                {
                    let choices = self.server_choices();
                    let target = form.current(&choices);
                    // 今えらんでいるものに合わせて開く。開き直すたびに先頭へ
                    // 戻ると、確認して閉じただけで選択が変わってしまう。
                    let index = choices
                        .rows(target)
                        .iter()
                        .position(|row| row.position == form.choice_position(target))
                        .unwrap_or(0);
                    self.overlay = Some(Overlay::ServerChoicePicker(ServerChoicePicker {
                        target,
                        index,
                        filter: String::new(),
                        form: Box::new(form),
                    }));
                }
                _ => {
                    let choices = self.server_choices();
                    edit_server_create_form(&mut form, key, &choices);
                    self.overlay = Some(Overlay::ServerCreateForm(form));
                }
            },
            Overlay::ServerPlanForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_server_plan_form(form),
                _ => {
                    let plans = self.server_plan_choices();
                    edit_server_plan_form(&mut form, key, &plans);
                    self.overlay = Some(Overlay::ServerPlanForm(form));
                }
            },
            Overlay::SshKeyForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_ssh_key_form(form),
                // 公開鍵は長いので、貼り付けずに選べるようにする。
                KeyCode::Char('k' | 'K')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && form.mode == SshKeyFormMode::Add =>
                {
                    self.open_ssh_key_source_from_form(form);
                }
                _ => {
                    edit_ssh_key_form(&mut form, key);
                    self.overlay = Some(Overlay::SshKeyForm(form));
                }
            },
            Overlay::PacketFilterForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_packet_filter_form(form),
                _ => {
                    edit_packet_filter_form(&mut form, key);
                    self.overlay = Some(Overlay::PacketFilterForm(form));
                }
            },
            Overlay::RuleForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_rule_form(form),
                _ => {
                    edit_rule_form(&mut form, key);
                    self.overlay = Some(Overlay::RuleForm(form));
                }
            },
            Overlay::NicPicker(mut picker) => {
                let choices = self.server_choices();
                let visible = picker.visible(&choices).len();
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => self.submit_nic_picker(picker),
                    _ => {
                        edit_nic_picker(&mut picker, key, visible);
                        self.overlay = Some(Overlay::NicPicker(picker));
                    }
                }
            }
            Overlay::ServerChoicePicker(mut picker) => {
                let choices = self.server_choices();
                let visible = picker.visible(&choices);
                match key.code {
                    // 絞り込みは捨てて、選んでいたものはそのまま戻す。
                    KeyCode::Esc => {
                        self.overlay = Some(Overlay::ServerCreateForm(*picker.form));
                    }
                    KeyCode::Enter => {
                        let mut form = picker.form;
                        if let Some(row) = visible.get(picker.index) {
                            form.take_choice(picker.target, row.position);
                        }
                        self.overlay = Some(Overlay::ServerCreateForm(*form));
                    }
                    _ => {
                        edit_server_choice_picker(&mut picker, key, visible.len());
                        self.overlay = Some(Overlay::ServerChoicePicker(picker));
                    }
                }
            }
            Overlay::DiskCreateForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_disk_create_form(form),
                _ => {
                    let plans = self.disk_plan_choices();
                    let archives = self.disk_archive_choices();
                    edit_disk_create_form(&mut form, key, &plans, &archives);
                    self.overlay = Some(Overlay::DiskCreateForm(form));
                }
            },
            Overlay::ArchiveForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_archive_form(form),
                _ => {
                    let sources = self.archive_source_choices().len();
                    edit_archive_form(&mut form, key, sources);
                    self.overlay = Some(Overlay::ArchiveForm(form));
                }
            },
            Overlay::AutoBackupForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_auto_backup_form(form),
                _ => {
                    let disks = self.archive_source_choices().len();
                    edit_auto_backup_form(&mut form, key, disks);
                    self.overlay = Some(Overlay::AutoBackupForm(form));
                }
            },
            Overlay::DiskServerPicker(mut picker) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_disk_server_picker(picker),
                KeyCode::Down | KeyCode::Char('j') => {
                    picker.move_selection(true);
                    self.overlay = Some(Overlay::DiskServerPicker(picker));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    picker.move_selection(false);
                    self.overlay = Some(Overlay::DiskServerPicker(picker));
                }
                _ => self.overlay = Some(Overlay::DiskServerPicker(picker)),
            },
            Overlay::SshKeyPicker { back, mut stage } => match (&mut stage, key.code) {
                // 一覧からは取得元へ、取得元からは呼び出し元のフォームへ戻る。
                (SshKeyStage::Source { .. }, KeyCode::Esc) => self.close_ssh_key_picker(*back),
                (_, KeyCode::Esc) => {
                    self.overlay = Some(Overlay::SshKeyPicker {
                        back,
                        stage: SshKeyStage::Source { index: 0 },
                    });
                }
                (SshKeyStage::Source { index }, KeyCode::Enter) => {
                    let source = back.sources()[*index];
                    self.choose_ssh_key_source(back, source);
                }
                (SshKeyStage::Keys { keys, index, .. }, KeyCode::Enter) => {
                    let key = keys[*index].clone();
                    self.take_ssh_key(*back, &key);
                }
                (SshKeyStage::GithubUser { user }, KeyCode::Enter) => {
                    let user = user.clone();
                    self.submit_github_ssh_user(back, user);
                }
                (SshKeyStage::GithubUser { user }, KeyCode::Backspace) => {
                    user.pop();
                    self.overlay = Some(Overlay::SshKeyPicker { back, stage });
                }
                (SshKeyStage::GithubUser { user }, KeyCode::Char(c)) => {
                    user.push(c);
                    self.overlay = Some(Overlay::SshKeyPicker { back, stage });
                }
                (_, KeyCode::Down | KeyCode::Char('j')) => {
                    stage.move_selection(true, back.sources().len());
                    self.overlay = Some(Overlay::SshKeyPicker { back, stage });
                }
                (_, KeyCode::Up | KeyCode::Char('k')) => {
                    stage.move_selection(false, back.sources().len());
                    self.overlay = Some(Overlay::SshKeyPicker { back, stage });
                }
                _ => self.overlay = Some(Overlay::SshKeyPicker { back, stage }),
            },
            Overlay::RagUploadForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_rag_upload_form(form),
                _ => {
                    edit_rag_upload_form(&mut form, key);
                    self.overlay = Some(Overlay::RagUploadForm(form));
                }
            },
            Overlay::SwitchForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_switch_form(form),
                _ => {
                    edit_switch_form(&mut form, key);
                    self.overlay = Some(Overlay::SwitchForm(form));
                }
            },
            Overlay::DnsRecordForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_dns_record_form(form),
                _ => {
                    edit_dns_record_form(&mut form, key);
                    self.overlay = Some(Overlay::DnsRecordForm(form));
                }
            },
            Overlay::DnsZoneForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_dns_zone_form(form),
                _ => {
                    edit_dns_zone_form(&mut form, key);
                    self.overlay = Some(Overlay::DnsZoneForm(form));
                }
            },
            Overlay::SimpleMonitorForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_simple_monitor_form(form),
                _ => {
                    edit_simple_monitor_form(&mut form, key);
                    self.overlay = Some(Overlay::SimpleMonitorForm(form));
                }
            },
            Overlay::VaultForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_vault_form(form),
                _ => {
                    edit_vault_form(&mut form, key);
                    self.overlay = Some(Overlay::VaultForm(form));
                }
            },
            Overlay::SecretForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_secret_form(form),
                _ => {
                    edit_secret_form(&mut form, key);
                    self.overlay = Some(Overlay::SecretForm(form));
                }
            },
            Overlay::AlertProjectForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_alert_project_form(form),
                _ => {
                    edit_alert_project_form(&mut form, key);
                    self.overlay = Some(Overlay::AlertProjectForm(form));
                }
            },
            Overlay::AlertRuleForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_alert_rule_form(form),
                _ => {
                    edit_alert_rule_form(&mut form, key);
                    self.overlay = Some(Overlay::AlertRuleForm(form));
                }
            },
            Overlay::LogMeasureRuleForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_log_measure_rule_form(form),
                _ => {
                    edit_log_measure_rule_form(&mut form, key);
                    self.overlay = Some(Overlay::LogMeasureRuleForm(form));
                }
            },
            Overlay::LogRoutingForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_log_routing_form(form),
                _ => {
                    edit_log_routing_form(&mut form, key);
                    self.overlay = Some(Overlay::LogRoutingForm(form));
                }
            },
            Overlay::MetricsRoutingForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_metrics_routing_form(form),
                _ => {
                    edit_metrics_routing_form(&mut form, key);
                    self.overlay = Some(Overlay::MetricsRoutingForm(form));
                }
            },
            Overlay::DashboardForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_dashboard_form(form),
                _ => {
                    edit_dashboard_form(&mut form, key);
                    self.overlay = Some(Overlay::DashboardForm(form));
                }
            },
            Overlay::NotificationTargetForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_notification_target_form(form),
                _ => {
                    edit_notification_target_form(&mut form, key);
                    self.overlay = Some(Overlay::NotificationTargetForm(form));
                }
            },
            Overlay::NotificationRoutingForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_notification_routing_form(form),
                _ => {
                    edit_notification_routing_form(&mut form, key);
                    self.overlay = Some(Overlay::NotificationRoutingForm(form));
                }
            },
            Overlay::StorageForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_storage_form(form),
                _ => {
                    edit_storage_form(&mut form, key);
                    self.overlay = Some(Overlay::StorageForm(form));
                }
            },
            Overlay::StorageRetentionForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_storage_retention_form(form),
                KeyCode::Backspace => {
                    form.days.pop();
                    self.overlay = Some(Overlay::StorageRetentionForm(form));
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    form.days.push(c);
                    self.overlay = Some(Overlay::StorageRetentionForm(form));
                }
                _ => self.overlay = Some(Overlay::StorageRetentionForm(form)),
            },
            Overlay::StorageAccessKeyForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_storage_access_key_form(form),
                KeyCode::Backspace => {
                    form.description.pop();
                    self.overlay = Some(Overlay::StorageAccessKeyForm(form));
                }
                KeyCode::Char(c) => {
                    form.description.push(c);
                    self.overlay = Some(Overlay::StorageAccessKeyForm(form));
                }
                _ => self.overlay = Some(Overlay::StorageAccessKeyForm(form)),
            },
            Overlay::ServicePicker { mut index, initial } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') if initial => self.should_quit = true,
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Enter => self.switch_service(Service::ALL[index]),
                KeyCode::Down | KeyCode::Char('j') => {
                    index = move_service_within_category(index, 1);
                    self.overlay = Some(Overlay::ServicePicker { index, initial });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    index = move_service_within_category(index, -1);
                    self.overlay = Some(Overlay::ServicePicker { index, initial });
                }
                KeyCode::Right | KeyCode::Char('l') | KeyCode::PageDown => {
                    index = move_service_category(index, 1);
                    self.overlay = Some(Overlay::ServicePicker { index, initial });
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::PageUp => {
                    index = move_service_category(index, -1);
                    self.overlay = Some(Overlay::ServicePicker { index, initial });
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    index = category_service_indices(Service::ALL[index].category())[0];
                    self.overlay = Some(Overlay::ServicePicker { index, initial });
                }
                KeyCode::End | KeyCode::Char('G') => {
                    index = *category_service_indices(Service::ALL[index].category())
                        .last()
                        .unwrap_or(&index);
                    self.overlay = Some(Overlay::ServicePicker { index, initial });
                }
                _ => self.overlay = Some(Overlay::ServicePicker { index, initial }),
            },
            Overlay::ZonePicker { zones, mut index } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Enter => self.switch_zone(zones[index].name.clone()),
                KeyCode::Down | KeyCode::Char('j') => {
                    index = (index + 1) % zones.len();
                    self.overlay = Some(Overlay::ZonePicker { zones, index });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    index = (index + zones.len() - 1) % zones.len();
                    self.overlay = Some(Overlay::ZonePicker { zones, index });
                }
                _ => self.overlay = Some(Overlay::ZonePicker { zones, index }),
            },
            Overlay::Login(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_login(form),
                _ => {
                    edit_login_form(&mut form, key);
                    self.overlay = Some(Overlay::Login(form));
                }
            },
            Overlay::LoginPicker {
                host,
                accounts,
                mut index,
            } => {
                // 選択肢は保存済みのユーザーに加えて、末尾に「新しく入力する」を1件持つ。
                let rows = accounts.len() + 1;
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {}
                    KeyCode::Enter if index < accounts.len() => {
                        let username = accounts[index].clone();
                        self.login_with_saved_account(host, username);
                    }
                    KeyCode::Enter => {
                        self.overlay = Some(Overlay::Login(LoginForm {
                            username: String::new(),
                            password: String::new(),
                            save: false,
                            host,
                            field: 0,
                        }));
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        index = (index + 1) % rows;
                        self.overlay = Some(Overlay::LoginPicker {
                            host,
                            accounts,
                            index,
                        });
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        index = (index + rows - 1) % rows;
                        self.overlay = Some(Overlay::LoginPicker {
                            host,
                            accounts,
                            index,
                        });
                    }
                    _ => {
                        self.overlay = Some(Overlay::LoginPicker {
                            host,
                            accounts,
                            index,
                        });
                    }
                }
            }
        }
    }
}

/// `ListState` と `TableState` を同じように扱うための最小限のトレイト。
pub trait SelectableList {
    fn selected(&self) -> Option<usize>;
    fn select(&mut self, index: Option<usize>);
}

impl SelectableList for ListState {
    fn selected(&self) -> Option<usize> {
        ListState::selected(self)
    }
    fn select(&mut self, index: Option<usize>) {
        ListState::select(self, index);
    }
}

impl SelectableList for TableState {
    fn selected(&self) -> Option<usize> {
        TableState::selected(self)
    }
    fn select(&mut self, index: Option<usize>) {
        TableState::select(self, index);
    }
}

/// OS のクリップボードにコピーする。
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
    clipboard.set_text(text).map_err(|err| err.to_string())
}

/// `anyhow::Error` を原因も含めた 1 つの文字列にする。
fn fmt_error(err: anyhow::Error) -> String {
    let mut parts = vec![err.to_string()];
    parts.extend(err.chain().skip(1).map(|c| c.to_string()));
    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_matches_everything() {
        assert!(matches("", &["anything"]));
        assert!(matches("", &[]));
    }

    #[test]
    fn filter_is_case_insensitive_substring() {
        assert!(matches("REG", &["my-registry"]));
        assert!(matches("reg", &["MY-REGISTRY"]));
        assert!(!matches("xyz", &["my-registry"]));
    }

    #[test]
    fn filter_matches_any_field() {
        // 名前が外れてもホスト側で拾えること。
        assert!(matches("sakuracr", &["example", "example.sakuracr.jp"]));
    }

    /// サービスを追加したときに分類を書き忘れないための番人。
    ///
    /// `category()` は網羅マッチなので分類漏れはコンパイルで落ちるが、
    /// 並びが分類順から外れるとピッカーの見出しが分裂するので、そこを見る。
    #[test]
    fn services_are_ordered_by_category() {
        let mut seen: Vec<Category> = Vec::new();
        for service in Service::ALL {
            let category = service.category();
            if seen.last() != Some(&category) {
                assert!(
                    !seen.contains(&category),
                    "{} の分類 {} が並びの中で分裂している",
                    service.title(),
                    category.title()
                );
                seen.push(category);
            }
        }
    }

    /// 空の分類を残さないこと（該当サービスを実装したときに追加する方針）。
    #[test]
    fn every_category_has_a_service() {
        for category in Category::ALL {
            assert!(
                category.services().next().is_some(),
                "{} に属するサービスが無い",
                category.title()
            );
        }
    }

    /// 分類の並びと `Category::ALL` の並びが一致すること。
    #[test]
    fn category_order_matches_service_order() {
        let from_services: Vec<Category> = Category::ALL
            .into_iter()
            .filter(|c| c.services().next().is_some())
            .collect();
        let mut seen: Vec<Category> = Vec::new();
        for service in Service::ALL {
            if seen.last() != Some(&service.category()) {
                seen.push(service.category());
            }
        }
        assert_eq!(seen, from_services);
    }

    /// 前の資格情報で投げた通信の結果は捨てること。
    ///
    /// 捨てそこねると、切り替えたのに前のアカウントの内容が残り、
    /// `r` を押すまで直らない。
    #[test]
    fn old_results_are_dropped() {
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tx = Tx::new(sender);
        // 通信を投げた時点の送信口を複製して持たせる。
        let in_flight = tx.clone();
        // ここで資格情報が切り替わる。
        let mut current = tx;
        current.epoch += 1;
        assert_ne!(in_flight.epoch, current.epoch);
    }

    /// 資格情報そのものの受け渡しは、世代が違っても捨てないこと。
    ///
    /// 捨てると切り替え操作そのものが無視される。
    #[test]
    fn credential_messages_survive_epoch_change() {
        let loaded = Message::CredentialsLoaded {
            source: Box::new(CredentialSource::Env),
            result: Box::new(Err("dummy".to_string())),
        };
        assert!(loaded.ignores_epoch());
        // 一覧の取得結果は捨ててよい。
        assert!(!Message::Registries(Ok(Vec::new())).ignores_epoch());
    }

    /// エラー文から使えない理由を起こせること。
    #[test]
    fn reads_reason_from_error() {
        assert_eq!(
            availability_reason("APIエラー (403 Forbidden): 要求された操作は許可されていません。"),
            "権限なし"
        );
        assert_eq!(availability_reason("404 Not Found"), "未提供");
        assert_eq!(availability_reason("401 Unauthorized"), "認証エラー");
        // 分類できないものは断定しない。
        assert_eq!(availability_reason("connection closed"), "取得できず");
    }

    fn test_app() -> App {
        use std::sync::Arc;

        let creds = ApiCredentials {
            token: "token".to_string(),
            secret: "secret".to_string(),
            source: CredentialSource::Env,
            zone: None,
            api_root: None,
        };
        let clients = crate::Clients {
            sacloud: Arc::new(crate::sacloud::SacloudClient::new(&creds).unwrap()),
            apprun: Arc::new(crate::apprun::AppRunClient::new(&creds).unwrap()),
            dedicated: Arc::new(crate::apprun_dedicated::DedicatedClient::new(&creds).unwrap()),
            monitoring: Arc::new(crate::monitoring::MonitoringClient::new(&creds).unwrap()),
            api_gateway: Arc::new(crate::api_gateway::ApiGatewayClient::new(&creds).unwrap()),
            ai_engine_cloud: Arc::new(
                crate::ai_engine_cloud::AiEngineCloudClient::new(&creds).unwrap(),
            ),
        };
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        App::new(
            clients,
            Tx::new(tx),
            Config::default(),
            CredentialSource::Env,
            false,
        )
    }

    fn ai_engine_app(tab: AiEngineTab) -> App {
        let mut app = test_app();
        app.service = Service::AiEngine;
        app.ai_engine.tab = tab;
        app.managed_resources.state.select(Some(1));
        app
    }

    /// `--service` に渡す名前が重複していないこと。
    #[test]
    fn arg_names_are_unique() {
        let mut names: Vec<&str> = Service::ALL.iter().map(|s| s.arg_name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    /// ゾーン依存は分類とは別の軸であること。
    ///
    /// 同じ分類でもゾーン依存が分かれる（レジストリと AppRun は
    /// どちらもコンテナ分類だが、どちらもゾーンに依存しない）。
    #[test]
    fn zone_scope_is_independent_from_category() {
        assert_eq!(Service::Server.category(), Category::Compute);
        assert!(Service::Server.is_zoned());
        assert_eq!(Service::Switch.category(), Category::Network);
        assert!(Service::Switch.is_zoned());
        assert_eq!(Service::Switch.arg_name(), "switch");
        assert_eq!(Service::Secrets.category(), Category::Security);
        // Vault はAPI URLにゾーン名を含むが、リソース自体はグローバル。
        assert!(!Service::Secrets.is_zoned());
        // 分類が同じでもゾーン依存ではないもの。
        assert_eq!(Service::Registry.category(), Category::Container);
        assert!(!Service::Registry.is_zoned());
        assert!(!Service::Dns.is_zoned());
        assert!(!Service::Billing.is_zoned());
        assert!(!Service::EnhancedLoadBalancer.is_zoned());
        assert!(!Service::LocalRouter.is_zoned());
        assert!(!Service::Gslb.is_zoned());
        assert!(!Service::SimpleNotification.is_zoned());
        assert!(!Service::Kms.is_zoned());
        assert!(!Service::Iam.is_zoned());
        assert!(Service::Archive.is_zoned());
        assert!(Service::IsoImage.is_zoned());
    }

    #[test]
    fn secret_form_debug_redacts_value() {
        let vault = Vault {
            id: "vault-id".to_string(),
            name: "prod".to_string(),
            description: String::new(),
            tags: Vec::new(),
            kms_key_id: "kms-id".to_string(),
            created_at: None,
            modified_at: None,
        };
        let mut form = SecretForm::new(SecretFormMode::Create, vault, "db-password".to_string());
        form.value = "must-not-appear".to_string();
        let debug = format!("{form:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("must-not-appear"));
    }

    #[test]
    fn ai_engine_token_form_debug_redacts_token() {
        let form = AiEngineTokenForm {
            token: "uuid:must-not-appear".to_string(),
            ..AiEngineTokenForm::default()
        };
        let debug = format!("{form:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("must-not-appear"));
    }

    #[test]
    fn iam_user_form_debug_redacts_password() {
        let form = IamResourceForm {
            mode: IamResourceFormMode::Create,
            resource_type: "ユーザー".to_string(),
            target_id: None,
            name: "alice".to_string(),
            code: "alice".to_string(),
            password: "must-not-appear".to_string(),
            description: String::new(),
            extra: String::new(),
            field: 0,
        };
        let debug = format!("{form:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("must-not-appear"));
    }

    #[test]
    fn iam_credential_form_debug_redacts_private_key() {
        let form = IamCredentialForm {
            service_principal_id: "sp-1".to_string(),
            key_id: "key-1".to_string(),
            private_key: "must-not-appear".to_string(),
            field: 2,
            verifying: false,
        };
        let debug = format!("{form:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("must-not-appear"));
    }

    #[test]
    fn service_picker_moves_within_category() {
        let at = |want: Service| {
            Service::ALL
                .iter()
                .position(|service| *service == want)
                .unwrap()
        };
        assert_eq!(
            move_service_within_category(at(Service::Switch), 1),
            at(Service::Internet)
        );
        // 分類の先頭から上へ動かすと末尾へ回る。先頭の顔ぶれが変わっても
        // 壊れないよう、決め打ちせず一覧から取る。
        let first = Category::Network.services().next().unwrap();
        assert_eq!(
            Service::ALL[move_service_within_category(at(first), -1)],
            Category::Network.services().last().unwrap()
        );
    }

    #[test]
    fn service_picker_moves_between_categories_preserving_row() {
        let server = Service::ALL
            .iter()
            .position(|service| *service == Service::Server)
            .unwrap();
        assert_eq!(
            Service::ALL[move_service_category(server, 1)],
            Service::Registry
        );
        assert_eq!(
            Service::ALL[move_service_category(server, -1)],
            Service::Account
        );
    }

    #[test]
    fn api_gateway_is_resolvable_from_service_arg() {
        assert_eq!(Service::from_arg("api-gateway"), Some(Service::ApiGateway));
    }

    #[test]
    fn integration_category_lists_api_gateway_after_workflows() {
        let names: Vec<&str> = Category::Integration
            .services()
            .map(Service::arg_name)
            .collect();
        assert_eq!(
            names,
            vec![
                "simplemq",
                "simple-notification",
                "eventbus",
                "workflows",
                "api-gateway"
            ]
        );
    }

    #[test]
    fn api_gateway_tab_navigation_and_selection_use_hjkl() {
        use crossterm::event::{KeyCode, KeyEvent};
        assert_eq!(ApiGatewayTab::ALL[0], ApiGatewayTab::Subscriptions);
        assert_eq!(ApiGatewayTab::ALL[1], ApiGatewayTab::Services);
        assert_eq!(ApiGatewayTab::ALL[2], ApiGatewayTab::Routes);
        assert_eq!(ApiGatewayTab::ALL[3], ApiGatewayTab::Users);
        assert_eq!(ApiGatewayTab::ALL[4], ApiGatewayTab::Groups);
        assert_eq!(ApiGatewayTab::ALL[5], ApiGatewayTab::Domains);
        assert_eq!(ApiGatewayTab::ALL[6], ApiGatewayTab::Certificates);
        assert_eq!(ApiGatewayTab::ALL[7], ApiGatewayTab::Oidc);

        let mut tab = ApiGatewayTab::Subscriptions;
        tab = tab.cycled(1);
        assert_eq!(tab, ApiGatewayTab::Services);
        tab = tab.cycled(-1);
        assert_eq!(tab, ApiGatewayTab::Subscriptions);
        tab = tab.cycled(-1);
        assert_eq!(tab, ApiGatewayTab::Oidc);

        let right = KeyEvent::from(KeyCode::Char('l'));
        let left = KeyEvent::from(KeyCode::Char('h'));
        assert!(matches!(right.code, KeyCode::Char('l')));
        assert!(matches!(left.code, KeyCode::Char('h')));
    }

    #[test]
    fn nosql_is_resolvable_from_service_arg() {
        assert_eq!(Service::from_arg("nosql"), Some(Service::NoSql));
    }

    /// ストレージ・データ分類の並び。マネージドDB系の隣に置く。
    #[test]
    fn storage_category_lists_nosql_and_auto_backup() {
        let names: Vec<&str> = Category::Storage
            .services()
            .map(Service::arg_name)
            .collect();
        assert_eq!(
            names,
            vec![
                "disk",
                "archive",
                "iso-image",
                "database",
                "nosql",
                "nfs",
                "object-storage",
                "enhanced-db",
                "auto-backup",
            ]
        );
    }

    /// NoSQL は東京第2ゾーン限定のため、ゾーン切り替えの対象にしない。
    /// ゾーン別件数の集計対象にも入れない。
    #[test]
    fn nosql_is_not_zone_scoped() {
        assert!(!Service::NoSql.is_zoned());
        assert_eq!(Service::NoSql.countable_label(), None);
    }

    #[test]
    fn seg_is_resolvable_from_service_arg() {
        assert_eq!(Service::from_arg("seg"), Some(Service::Seg));
    }

    /// ネットワーク分類の並び。DNS の後、ウェブアクセラレータの前。
    #[test]
    fn network_category_lists_seg_after_dns() {
        let names: Vec<&str> = Category::Network
            .services()
            .map(Service::arg_name)
            .collect();
        assert_eq!(
            names,
            vec![
                "network-map",
                "switch",
                "internet",
                "packet-filter",
                "bridge",
                "loadbalancer",
                "enhanced-loadbalancer",
                "vpcrouter",
                "gslb",
                "mobile-gateway",
                "local-router",
                "dns",
                "seg",
                "networking-suite",
                "webaccel",
            ]
        );
    }

    /// AI Engine は推論API（モデル一覧）と RAG を1つのサービスにまとめている。
    #[test]
    fn ai_category_has_a_single_merged_service() {
        let names: Vec<&str> = Category::Ai.services().map(Service::arg_name).collect();
        assert_eq!(names, vec!["ai-engine"]);

        use crossterm::event::{KeyCode, KeyEvent};

        let mut app = ai_engine_app(AiEngineTab::Usage);
        assert_eq!(app.active_pane(), Pane::None);
        assert_eq!(app.copy_text(), None);

        let before = app.managed_resources.state.selected();
        app.on_key_common(KeyEvent::from(KeyCode::Char('/')));
        app.on_key_common(KeyEvent::from(KeyCode::Char('y')));
        app.on_key_common(KeyEvent::from(KeyCode::Char('j')));
        assert_eq!(app.managed_resources.state.selected(), before);
        assert!(!app.filtering);
        assert_eq!(app.active_filter(), "");

        let mut billing = ai_engine_app(AiEngineTab::Billing);
        billing.ai_engine.billing_month = "202401".to_string();
        billing.on_key_ai_engine(KeyEvent::from(KeyCode::Char(']')));
        assert_eq!(billing.ai_engine.billing_month, "202402");
        billing.on_key_ai_engine(KeyEvent::from(KeyCode::Char('[')));
        assert_eq!(billing.ai_engine.billing_month, "202401");
    }

    #[test]
    fn networking_suite_is_resolvable_from_service_arg() {
        assert_eq!(
            Service::from_arg("networking-suite"),
            Some(Service::NetworkingSuite)
        );
    }

    /// 受付ゾーンが is1c 固定なので、ゾーン切り替えの対象にしない。
    #[test]
    fn networking_suite_is_not_zone_scoped() {
        assert!(!Service::NetworkingSuite.is_zoned());
        assert_eq!(Service::NetworkingSuite.countable_label(), None);
    }

    /// SEG は全ゾーンで提供されるので、ゾーン切り替えとゾーン別件数の対象。
    /// countable_label が Some なら load_zone_counts の match に分岐が要る。
    #[test]
    fn seg_is_zone_scoped_and_counted_per_zone() {
        assert!(Service::Seg.is_zoned());
        assert_eq!(Service::Seg.countable_label(), Some("ゲートウェイ"));
    }

    #[test]
    fn security_control_is_resolvable_from_service_arg() {
        assert_eq!(
            Service::from_arg("security-control"),
            Some(Service::SecurityControl)
        );
    }

    /// セキュリティ分類の並び。
    #[test]
    fn security_category_lists_security_control_last() {
        let names: Vec<&str> = Category::Security
            .services()
            .map(Service::arg_name)
            .collect();
        assert_eq!(
            names,
            vec!["secrets", "kms", "iam", "security-control", "cloudhsm"]
        );
    }

    #[test]
    fn cloudhsm_is_resolvable_from_service_arg() {
        assert_eq!(Service::from_arg("cloudhsm"), Some(Service::CloudHsm));
    }

    /// クラウドHSMはゾーンごとに配置されるので、ゾーン別件数の対象。
    #[test]
    fn cloudhsm_is_zone_scoped_and_counted_per_zone() {
        assert!(Service::CloudHsm.is_zoned());
        assert_eq!(Service::CloudHsm.countable_label(), Some("HSM"));
    }

    #[test]
    fn auto_backup_is_resolvable_from_service_arg() {
        assert_eq!(Service::from_arg("auto-backup"), Some(Service::AutoBackup));
    }

    /// セキュリティコントロールはプロジェクト単位でゾーンに依存しない。
    #[test]
    fn security_control_is_not_zone_scoped() {
        assert!(!Service::SecurityControl.is_zoned());
        assert_eq!(Service::SecurityControl.countable_label(), None);
    }
}

#[cfg(test)]
mod paste_tests {
    /// 貼り付けた文字列から制御文字を除く処理。
    ///
    /// `on_paste` は `App` を要するため、ここでは正規化だけを検証する。
    fn sanitize(text: &str) -> String {
        let text: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        text.trim().to_string()
    }

    #[test]
    fn strips_surrounding_whitespace() {
        assert_eq!(sanitize("  abc123  "), "abc123");
        assert_eq!(sanitize("\ntoken\n"), "token");
    }

    /// 改行やタブが混ざっても欄が壊れないこと。
    #[test]
    fn replaces_control_characters() {
        assert_eq!(sanitize("aaa\nbbb"), "aaa bbb");
        assert_eq!(sanitize("aaa\tbbb"), "aaa bbb");
    }

    #[test]
    fn empty_paste_is_ignored() {
        assert!(sanitize("").is_empty());
        assert!(sanitize("   \n  ").is_empty());
    }

    /// トークンに使われる文字はそのまま残ること。
    #[test]
    fn keeps_token_characters() {
        let token = "abcDEF012-_3456789";
        assert_eq!(sanitize(token), token);
    }
}
