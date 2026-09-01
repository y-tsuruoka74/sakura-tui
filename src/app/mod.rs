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
mod dedicated;
mod networking_suite;
mod nosql;
mod observability;
mod security_control;
mod seg;
mod server;
mod switch;

pub use account::AccountView;
pub use ai_engine::{AiEngineTab, AiEngineView};
pub use api_gateway::{ApiGatewayTab, ApiGatewayView};
pub use apprun::{AppRunPane, AppRunView};
pub use billing::{BillingFocus, BillingTab, BillingView};
pub use cloudhsm::{CloudHsmTab, CloudHsmView};
pub use dedicated::{DedicatedFocus, DedicatedTab, DedicatedView};
pub use networking_suite::{NetworkingSuiteTab, NetworkingSuiteView};
pub use nosql::{NoSqlTab, NoSqlView};
pub use observability::{
    DnsView, ListFocus, MonitoringTab, MonitoringView, SecretsView, SimpleMonitorView,
};
pub use security_control::{SecurityControlTab, SecurityControlView};
pub use seg::{SegTab, SegView};
pub use server::ServerView;
pub use switch::SwitchView;

use crate::account::AuthStatus;
use crate::ai_engine::AiEngineClient;
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
use crate::config::{ApiCredentials, Config, CredentialSource, IamCredentials, RegistryLogin};
use crate::iaas::{PowerAction, Server, Zone};
use crate::managed_resources::{ManagedResource, ManagedResourceKind};
use crate::monitoring::{
    AlertHistory, AlertProject, AlertRule, AlertRuleInput, DashboardProject, LogMeasureRule,
    LogMeasureRuleInput, LogRouting, LogRoutingInput, MetricsRouting, MetricsRoutingInput,
    MonitoringClient, NotificationRouting, NotificationTarget, Publisher, Storage,
    StorageAccessKey, StorageAccessKeySecret, StorageKind,
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
    // スイッチ
    Switches,
    CloudResources,
    ManagedResources,
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

fn category_service_indices(category: Category) -> Vec<usize> {
    Service::ALL
        .iter()
        .enumerate()
        .filter_map(|(index, service)| (service.category() == category).then_some(index))
        .collect()
}

fn move_service_within_category(index: usize, delta: i32) -> usize {
    let indices = category_service_indices(Service::ALL[index].category());
    let position = indices
        .iter()
        .position(|candidate| *candidate == index)
        .unwrap_or(0) as i32;
    indices[(position + delta).rem_euclid(indices.len() as i32) as usize]
}

fn move_service_category(index: usize, delta: i32) -> usize {
    let category = Service::ALL[index].category();
    let category_index = Category::ALL
        .iter()
        .position(|candidate| *candidate == category)
        .unwrap_or(0) as i32;
    let current_indices = category_service_indices(category);
    let row = current_indices
        .iter()
        .position(|candidate| *candidate == index)
        .unwrap_or(0);
    let next =
        Category::ALL[(category_index + delta).rem_euclid(Category::ALL.len() as i32) as usize];
    let next_indices = category_service_indices(next);
    next_indices[row.min(next_indices.len() - 1)]
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

/// サービスが今の資格情報で使えるかどうか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// 判断する材料がまだ無い。
    Unknown,
    Usable,
    /// 使えない。添えてあるのは短い理由。
    Unusable(&'static str),
}

/// エラー文から短い理由を起こす。
///
/// 一覧に出すので、原因が一目で分かる長さに切り詰める。
fn availability_reason(error: &str) -> &'static str {
    if error.contains("403") || error.contains("許可されていません") {
        "権限なし"
    } else if error.contains("404") {
        "未提供"
    } else if error.contains("401") {
        "認証エラー"
    } else {
        "取得できず"
    }
}

/// サービスの大分類。
///
/// 利用者がコントロールパネルで探すときの括りに合わせる。
/// サービスを増やすときは、まず公式のカタログでの分類に従うこと。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Compute,
    Container,
    Ai,
    Integration,
    Network,
    Storage,
    Security,
    Ops,
    Account,
}

impl Category {
    /// 表示順。サービス一覧の並びもこの順に揃える。
    pub const ALL: [Category; 9] = [
        Category::Compute,
        Category::Container,
        Category::Ai,
        Category::Integration,
        Category::Network,
        Category::Storage,
        Category::Security,
        Category::Ops,
        Category::Account,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Compute => "コンピュート",
            Category::Container => "コンテナ・アプリ実行",
            Category::Ai => "AI",
            Category::Integration => "アプリケーション連携",
            Category::Network => "ネットワーク",
            Category::Storage => "ストレージ・データ",
            Category::Security => "セキュリティ",
            Category::Ops => "運用・監視",
            Category::Account => "アカウント",
        }
    }

    /// この分類に属するサービス。`Service::ALL` が分類順に並んでいる前提。
    pub fn services(self) -> impl Iterator<Item = Service> {
        Service::ALL
            .into_iter()
            .filter(move |svc| svc.category() == self)
    }
}

/// TUI が扱うサービス。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Service {
    #[default]
    Registry,
    AppRun,
    Dedicated,
    AiEngine,
    SimpleMq,
    SimpleNotification,
    EventBus,
    Workflows,
    ApiGateway,
    AutoScale,
    Server,
    Switch,
    Disk,
    Internet,
    PacketFilter,
    Bridge,
    LoadBalancer,
    EnhancedLoadBalancer,
    VpcRouter,
    Gslb,
    MobileGateway,
    LocalRouter,
    Seg,
    NetworkingSuite,
    Database,
    NoSql,
    Nfs,
    Archive,
    IsoImage,
    ObjectStorage,
    EnhancedDb,
    AutoBackup,
    WebAccel,
    Dns,
    SimpleMonitor,
    Secrets,
    Kms,
    Iam,
    SecurityControl,
    CloudHsm,
    Monitoring,
    Account,
    Billing,
}

#[derive(Debug, Clone, Copy)]
struct ServiceMeta {
    category: Category,
    title: &'static str,
    arg_name: &'static str,
    countable_label: Option<&'static str>,
    count_label: Option<&'static str>,
    zoned: bool,
}

impl Service {
    /// 分類順に並べる。ピッカーの並び・`s` での巡回・`--service` のヘルプが
    /// すべてこの順になるので、分類をまたぐ並べ替えはしないこと。
    pub const ALL: [Service; 43] = [
        // コンピュート
        Service::Server,
        // コンテナ・アプリ実行
        Service::Registry,
        Service::AppRun,
        Service::Dedicated,
        // AI
        Service::AiEngine,
        // アプリケーション連携
        Service::SimpleMq,
        Service::SimpleNotification,
        Service::EventBus,
        Service::Workflows,
        Service::ApiGateway,
        // ネットワーク
        Service::Switch,
        Service::Internet,
        Service::PacketFilter,
        Service::Bridge,
        Service::LoadBalancer,
        Service::EnhancedLoadBalancer,
        Service::VpcRouter,
        Service::Gslb,
        Service::MobileGateway,
        Service::LocalRouter,
        Service::Dns,
        Service::Seg,
        Service::NetworkingSuite,
        Service::WebAccel,
        // ストレージ・データ
        Service::Disk,
        Service::Archive,
        Service::IsoImage,
        Service::Database,
        Service::NoSql,
        Service::Nfs,
        Service::ObjectStorage,
        Service::EnhancedDb,
        Service::AutoBackup,
        // セキュリティ
        Service::Secrets,
        Service::Kms,
        Service::Iam,
        Service::SecurityControl,
        Service::CloudHsm,
        // 運用・監視
        Service::SimpleMonitor,
        Service::Monitoring,
        Service::AutoScale,
        // アカウント
        Service::Account,
        Service::Billing,
    ];

    /// サービス追加時に更新するメタデータを一か所へ集約する。
    fn meta(self) -> ServiceMeta {
        match self {
            Service::Server => ServiceMeta {
                category: Category::Compute,
                title: "サーバー",
                arg_name: "server",
                countable_label: Some("サーバー"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::Registry => ServiceMeta {
                category: Category::Container,
                title: "コンテナレジストリ",
                arg_name: "registry",
                countable_label: None,
                count_label: Some("件"),
                zoned: false,
            },
            Service::AppRun => ServiceMeta {
                category: Category::Container,
                title: "AppRun",
                arg_name: "apprun",
                countable_label: None,
                count_label: Some("アプリ"),
                zoned: false,
            },
            Service::Dedicated => ServiceMeta {
                category: Category::Container,
                title: "AppRun専有型",
                arg_name: "dedicated",
                countable_label: None,
                count_label: Some("クラスタ"),
                zoned: false,
            },
            Service::AiEngine => ServiceMeta {
                category: Category::Ai,
                title: "AI Engine",
                arg_name: "ai-engine",
                countable_label: None,
                // 専用トークンをキーチェーンから読むのはサービスを開いたときだけ。
                count_label: None,
                zoned: false,
            },
            Service::SimpleMq => ServiceMeta {
                category: Category::Integration,
                title: "シンプルMQ",
                arg_name: "simplemq",
                countable_label: None,
                count_label: Some("キュー"),
                zoned: false,
            },
            Service::SimpleNotification => ServiceMeta {
                category: Category::Integration,
                title: "シンプル通知",
                arg_name: "simple-notification",
                countable_label: None,
                count_label: Some("設定"),
                zoned: false,
            },
            Service::EventBus => ServiceMeta {
                category: Category::Integration,
                title: "イベントバス",
                arg_name: "eventbus",
                countable_label: None,
                count_label: Some("リソース"),
                zoned: false,
            },
            Service::Workflows => ServiceMeta {
                category: Category::Integration,
                title: "ワークフロー",
                arg_name: "workflows",
                countable_label: None,
                count_label: Some("件"),
                zoned: false,
            },
            Service::ApiGateway => ServiceMeta {
                category: Category::Integration,
                title: "APIゲートウェイ",
                arg_name: "api-gateway",
                countable_label: None,
                count_label: Some("サービス"),
                zoned: false,
            },
            Service::AutoScale => ServiceMeta {
                category: Category::Ops,
                title: "オートスケール",
                arg_name: "autoscale",
                countable_label: None,
                count_label: Some("設定"),
                zoned: false,
            },
            Service::Switch => ServiceMeta {
                category: Category::Network,
                title: "スイッチ",
                arg_name: "switch",
                countable_label: Some("スイッチ"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::Disk => ServiceMeta {
                category: Category::Storage,
                title: "ディスク",
                arg_name: "disk",
                countable_label: Some("ディスク"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::Internet => ServiceMeta {
                category: Category::Network,
                title: "ルータ＋スイッチ",
                arg_name: "internet",
                countable_label: Some("ルータ＋スイッチ"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::PacketFilter => ServiceMeta {
                category: Category::Network,
                title: "パケットフィルタ",
                arg_name: "packet-filter",
                countable_label: Some("パケットフィルタ"),
                count_label: Some("件"),
                zoned: true,
            },
            Service::Bridge => ServiceMeta {
                category: Category::Network,
                title: "ブリッジ接続",
                arg_name: "bridge",
                countable_label: Some("ブリッジ"),
                count_label: Some("件"),
                zoned: true,
            },
            Service::LoadBalancer => ServiceMeta {
                category: Category::Network,
                title: "ロードバランサ",
                arg_name: "loadbalancer",
                countable_label: Some("ロードバランサ"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::EnhancedLoadBalancer => ServiceMeta {
                category: Category::Network,
                title: "エンハンスドロードバランサ",
                arg_name: "enhanced-loadbalancer",
                countable_label: None,
                count_label: Some("台"),
                zoned: false,
            },
            Service::VpcRouter => ServiceMeta {
                category: Category::Network,
                title: "VPCルータ",
                arg_name: "vpcrouter",
                countable_label: Some("VPCルータ"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::Gslb => ServiceMeta {
                category: Category::Network,
                title: "GSLB",
                arg_name: "gslb",
                countable_label: None,
                count_label: Some("台"),
                zoned: false,
            },
            Service::MobileGateway => ServiceMeta {
                category: Category::Network,
                title: "モバイルゲートウェイ",
                arg_name: "mobile-gateway",
                countable_label: Some("モバイルゲートウェイ"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::LocalRouter => ServiceMeta {
                category: Category::Network,
                title: "ローカルルータ",
                arg_name: "local-router",
                countable_label: None,
                count_label: Some("台"),
                zoned: false,
            },
            Service::Database => ServiceMeta {
                category: Category::Storage,
                title: "データベース",
                arg_name: "database",
                countable_label: Some("データベース"),
                count_label: Some("台"),
                zoned: true,
            },
            // 東京第2ゾーン限定のため、ゾーン切り替えの対象にはしない。
            // 問い合わせ先のゾーンは画面のタイトルに出す。
            Service::NoSql => ServiceMeta {
                category: Category::Storage,
                title: "NoSQL",
                arg_name: "nosql",
                countable_label: None,
                count_label: Some("DB"),
                zoned: false,
            },
            Service::Nfs => ServiceMeta {
                category: Category::Storage,
                title: "NFS",
                arg_name: "nfs",
                countable_label: Some("NFS"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::Archive => ServiceMeta {
                category: Category::Storage,
                title: "アーカイブ",
                arg_name: "archive",
                countable_label: Some("アーカイブ"),
                count_label: Some("件"),
                zoned: true,
            },
            Service::IsoImage => ServiceMeta {
                category: Category::Storage,
                title: "ISOイメージ",
                arg_name: "iso-image",
                countable_label: Some("ISOイメージ"),
                count_label: Some("件"),
                zoned: true,
            },
            Service::ObjectStorage => ServiceMeta {
                category: Category::Storage,
                title: "オブジェクトストレージ",
                arg_name: "object-storage",
                countable_label: None,
                count_label: Some("バケット"),
                zoned: false,
            },
            Service::EnhancedDb => ServiceMeta {
                category: Category::Storage,
                title: "エンハンスドデータベース",
                arg_name: "enhanced-db",
                countable_label: None,
                count_label: Some("DB"),
                zoned: false,
            },
            Service::AutoBackup => ServiceMeta {
                category: Category::Storage,
                title: "自動バックアップ",
                arg_name: "auto-backup",
                countable_label: None,
                count_label: Some("設定"),
                zoned: false,
            },
            Service::WebAccel => ServiceMeta {
                category: Category::Network,
                title: "ウェブアクセラレータ",
                arg_name: "webaccel",
                countable_label: None,
                count_label: Some("サイト"),
                zoned: false,
            },
            Service::Dns => ServiceMeta {
                category: Category::Network,
                title: "DNS",
                arg_name: "dns",
                countable_label: None,
                count_label: Some("DNSゾーン"),
                zoned: false,
            },
            Service::Seg => ServiceMeta {
                category: Category::Network,
                title: "サービスエンドポイントゲートウェイ",
                arg_name: "seg",
                countable_label: Some("ゲートウェイ"),
                count_label: Some("台"),
                zoned: true,
            },
            // 受付ゾーンが is1c 固定なので、ゾーン切り替えの対象にはしない。
            // 問い合わせ先のゾーンは画面のタイトルに出す。
            Service::NetworkingSuite => ServiceMeta {
                category: Category::Network,
                title: "ネットワークスイート",
                arg_name: "networking-suite",
                countable_label: None,
                count_label: Some("グループ"),
                zoned: false,
            },
            Service::Secrets => ServiceMeta {
                category: Category::Security,
                title: "シークレットマネージャ",
                arg_name: "secrets",
                countable_label: Some("Vault"),
                count_label: Some("Vault"),
                zoned: false,
            },
            Service::Kms => ServiceMeta {
                category: Category::Security,
                title: "KMS",
                arg_name: "kms",
                countable_label: None,
                count_label: Some("鍵"),
                zoned: false,
            },
            Service::Iam => ServiceMeta {
                category: Category::Security,
                title: "IAM",
                arg_name: "iam",
                countable_label: None,
                count_label: Some("リソース"),
                zoned: false,
            },
            // プロジェクト単位の機能でゾーンに依存しない。
            Service::SecurityControl => ServiceMeta {
                category: Category::Security,
                title: "セキュリティコントロール",
                arg_name: "security-control",
                countable_label: None,
                count_label: Some("ルール"),
                zoned: false,
            },
            // ゾーンごとに配置されるアプライアンス。全ゾーンで提供される。
            Service::CloudHsm => ServiceMeta {
                category: Category::Security,
                title: "クラウドHSM",
                arg_name: "cloudhsm",
                countable_label: Some("HSM"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::SimpleMonitor => ServiceMeta {
                category: Category::Ops,
                title: "シンプル監視",
                arg_name: "monitor",
                countable_label: None,
                count_label: Some("件"),
                zoned: false,
            },
            Service::Monitoring => ServiceMeta {
                category: Category::Ops,
                title: "モニタリングスイート",
                arg_name: "monitoring",
                countable_label: Some("プロジェクト"),
                count_label: Some("プロジェクト"),
                zoned: true,
            },
            Service::Account => ServiceMeta {
                category: Category::Account,
                title: "権限",
                arg_name: "account",
                countable_label: None,
                count_label: None,
                zoned: false,
            },
            Service::Billing => ServiceMeta {
                category: Category::Account,
                title: "請求",
                arg_name: "billing",
                countable_label: None,
                count_label: Some("件"),
                zoned: false,
            },
        }
    }

    /// このサービスが属する大分類。
    ///
    /// 分類は「利用者が何のために使うか」で決める。API の置き場所では決めない
    /// （レジストリ・DNS・シンプル監視は API 上どれも `commonserviceitem` だが
    /// 分類は別々、AppRun 共用型と専有型はエンドポイントが違うが同じ分類）。
    /// ゾーン依存かどうかは分類とは別の軸なので [`Service::is_zoned`] を使う。
    pub fn category(self) -> Category {
        self.meta().category
    }

    pub fn title(self) -> &'static str {
        self.meta().title
    }

    /// `--service` に渡せる短い名前。
    pub fn arg_name(self) -> &'static str {
        self.meta().arg_name
    }

    pub fn from_arg(name: &str) -> Option<Self> {
        Service::ALL
            .into_iter()
            .find(|svc| svc.arg_name().eq_ignore_ascii_case(name))
    }

    /// ゾーンごとの件数を数えるときの対象の呼び名。
    ///
    /// ゾーンに依存しないサービスは数えない。
    pub fn countable_label(self) -> Option<&'static str> {
        self.meta().countable_label
    }

    /// サービス一覧に出す件数の呼び名。数えられないサービスは `None`。
    ///
    /// ゾーン依存のサービスは現在のゾーンだけを数える。
    pub fn count_label(self) -> Option<&'static str> {
        self.meta().count_label
    }

    /// ゾーンを選ぶ意味があるサービスか。
    pub fn is_zoned(self) -> bool {
        self.meta().zoned
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserFormMode {
    Add,
    Edit,
}

/// ユーザー追加・編集フォーム。
#[derive(Debug, Clone)]
pub struct UserForm {
    pub registry: ResourceId,
    pub registry_name: String,
    pub registry_host: String,
    pub mode: UserFormMode,
    pub username: String,
    pub password: String,
    pub permission: usize,
    pub field: usize,
}

impl UserForm {
    pub const FIELDS: usize = 3;

    pub fn permission(&self) -> Permission {
        Permission::ALL[self.permission % Permission::ALL.len()]
    }
}

/// 資格情報の保存先。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfileStorage {
    /// `~/.usacloud/<名前>/config.json`。usacloud・Terraform・Packer と共用できる。
    #[default]
    Usacloud,
    /// OS のキーチェーン。平文は残らないが、この TUI からしか使えない。
    Keychain,
}

impl ProfileStorage {
    pub const ALL: [ProfileStorage; 2] = [ProfileStorage::Usacloud, ProfileStorage::Keychain];

    pub fn title(self) -> &'static str {
        match self {
            ProfileStorage::Usacloud => "usacloud 互換",
            ProfileStorage::Keychain => "キーチェーン",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ProfileStorage::Usacloud => {
                "~/.usacloud に平文(0600)。usacloud/Terraform/Packer と共用できます"
            }
            ProfileStorage::Keychain => {
                "OSのキーチェーンに保存。平文は残りませんが他ツールからは使えません"
            }
        }
    }

    fn toggled(self) -> Self {
        match self {
            ProfileStorage::Usacloud => ProfileStorage::Keychain,
            ProfileStorage::Keychain => ProfileStorage::Usacloud,
        }
    }
}

/// API ルートの選択肢。環境（本番 / 社内テスト）の切り替えに使う。
#[derive(Debug, Clone)]
pub struct ApiRootChoice {
    pub label: &'static str,
    pub url: String,
}

/// 資格情報の作成フォーム。
#[derive(Debug, Clone, Default)]
pub struct ProfileForm {
    pub name: String,
    pub token: String,
    pub secret: String,
    /// 選べるゾーン。API から取れていればそれを、無ければ既知の一覧を使う。
    pub zones: Vec<Zone>,
    pub zone_index: usize,
    pub api_roots: Vec<ApiRootChoice>,
    pub api_root_index: usize,
    pub storage: ProfileStorage,
    pub field: usize,
    /// 検証中はキー入力を受け付けない。
    pub verifying: bool,
}

/// AI Engine専用アカウントトークンの登録フォーム。
#[derive(Clone, Default)]
pub struct AiEngineTokenForm {
    pub entries: Vec<crate::config::AiEngineTokenEntry>,
    pub index: usize,
    pub adding: bool,
    pub name: String,
    pub token: String,
    pub field: usize,
    pub verifying: bool,
}

/// IAMサービスプリンシパルの登録フォーム。
#[derive(Clone, Default)]
pub struct IamCredentialForm {
    pub service_principal_id: String,
    pub key_id: String,
    pub private_key: String,
    pub field: usize,
    pub verifying: bool,
}

impl IamCredentialForm {
    pub const FIELDS: usize = 3;

    fn credentials(&self) -> IamCredentials {
        IamCredentials {
            service_principal_id: self.service_principal_id.trim().to_string(),
            key_id: self.key_id.trim().to_string(),
            private_key: self.private_key.trim().to_string(),
        }
    }
}

impl std::fmt::Debug for IamCredentialForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IamCredentialForm")
            .field("service_principal_id", &self.service_principal_id)
            .field("key_id", &self.key_id)
            .field("private_key", &"<redacted>")
            .field("field", &self.field)
            .field("verifying", &self.verifying)
            .finish()
    }
}

impl std::fmt::Debug for AiEngineTokenForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiEngineTokenForm")
            .field("token", &"<redacted>")
            .field("entries", &self.entries)
            .field("index", &self.index)
            .field("adding", &self.adding)
            .field("name", &self.name)
            .field("field", &self.field)
            .field("verifying", &self.verifying)
            .finish()
    }
}

impl ProfileForm {
    /// 入力欄の数（末尾の 3 つは選択式）。
    pub const FIELDS: usize = 6;
    /// ゾーンを選ぶ欄の位置。
    pub const ZONE_FIELD: usize = 3;
    /// API ルートを選ぶ欄の位置。
    pub const ROOT_FIELD: usize = 4;
    /// 保存先を選ぶ欄の位置。
    pub const STORAGE_FIELD: usize = 5;

    pub fn label(index: usize) -> &'static str {
        match index {
            0 => "名前",
            1 => "アクセストークン",
            2 => "シークレット",
            3 => "既定ゾーン",
            4 => "接続先",
            _ => "保存先",
        }
    }

    /// 選択中の API ルート。
    pub fn api_root(&self) -> &ApiRootChoice {
        &self.api_roots[self.api_root_index.min(self.api_roots.len() - 1)]
    }

    /// 接続先を切り替える。
    ///
    /// 環境ごとにゾーン名が違うので、ゾーンの選択肢も合わせて入れ替える。
    fn cycle_api_root(&mut self, delta: i32) {
        let len = self.api_roots.len() as i32;
        self.api_root_index = ((self.api_root_index as i32 + delta).rem_euclid(len)) as usize;
        self.zones = crate::iaas::known_zones_for(&self.api_root().url);
        self.zone_index = 0;
    }

    /// 文字入力を受け付ける欄か。
    fn is_text(index: usize) -> bool {
        index < Self::ZONE_FIELD
    }

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.token,
            2 => &self.secret,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.token),
            2 => Some(&mut self.secret),
            _ => None,
        }
    }

    /// 選択中のゾーン。
    pub fn zone(&self) -> &Zone {
        &self.zones[self.zone_index.min(self.zones.len().saturating_sub(1))]
    }

    fn cycle_zone(&mut self, delta: i32) {
        let len = self.zones.len() as i32;
        self.zone_index = ((self.zone_index as i32 + delta).rem_euclid(len)) as usize;
    }

    /// トークンとシークレットは伏せ字にする。
    pub fn is_secret(index: usize) -> bool {
        matches!(index, 1 | 2)
    }
}

/// レジストリへのログインフォーム。
#[derive(Debug, Clone)]
pub struct LoginForm {
    pub host: String,
    pub username: String,
    pub password: String,
    pub save: bool,
    pub field: usize,
}

impl LoginForm {
    pub const FIELDS: usize = 3;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryFormMode {
    Create,
    Edit,
}

/// レジストリの作成・編集フォーム。
#[derive(Debug, Clone)]
pub struct RegistryForm {
    pub mode: RegistryFormMode,
    /// 編集時の対象。作成時は `None`。
    pub target: Option<ContainerRegistry>,
    pub name: String,
    /// `<subdomain>.sakuracr.jp` の左側。作成時のみ指定できる。
    pub subdomain: String,
    pub description: String,
    pub virtual_domain: String,
    pub field: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IamResourceFormMode {
    Create,
    Edit,
}

#[derive(Clone)]
pub struct IamResourceForm {
    pub mode: IamResourceFormMode,
    pub resource_type: String,
    pub target_id: Option<String>,
    pub name: String,
    pub code: String,
    pub password: String,
    pub description: String,
    /// ユーザーはメール、プロジェクトは親フォルダID、SPはプロジェクトID。
    pub extra: String,
    pub field: usize,
}

impl std::fmt::Debug for IamResourceForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IamResourceForm")
            .field("mode", &self.mode)
            .field("resource_type", &self.resource_type)
            .field("target_id", &self.target_id)
            .field("name", &self.name)
            .field("code", &self.code)
            .field("password", &"<redacted>")
            .field("description", &self.description)
            .field("extra", &self.extra)
            .field("field", &self.field)
            .finish()
    }
}

impl IamResourceForm {
    pub fn labels(&self) -> &'static [&'static str] {
        match (self.mode, self.resource_type.as_str()) {
            (IamResourceFormMode::Create, "ユーザー") => {
                &["名前", "ユーザーコード", "パスワード", "説明", "メール"]
            }
            (IamResourceFormMode::Create, "プロジェクト") => {
                &["名前", "プロジェクトコード", "説明", "親フォルダID"]
            }
            (IamResourceFormMode::Create, "サービスプリンシパル") => {
                &["名前", "説明", "プロジェクトID"]
            }
            (IamResourceFormMode::Edit, "ユーザー") => &["名前", "パスワード", "説明"],
            _ => &["名前", "説明"],
        }
    }

    pub fn value(&self, index: usize) -> &str {
        match (self.mode, self.resource_type.as_str(), index) {
            (IamResourceFormMode::Create, "ユーザー", 0) => &self.name,
            (IamResourceFormMode::Create, "ユーザー", 1) => &self.code,
            (IamResourceFormMode::Create, "ユーザー", 2) => &self.password,
            (IamResourceFormMode::Create, "ユーザー", 3) => &self.description,
            (IamResourceFormMode::Create, "ユーザー", 4) => &self.extra,
            (IamResourceFormMode::Create, "プロジェクト", 0) => &self.name,
            (IamResourceFormMode::Create, "プロジェクト", 1) => &self.code,
            (IamResourceFormMode::Create, "プロジェクト", 2) => &self.description,
            (IamResourceFormMode::Create, "プロジェクト", 3) => &self.extra,
            (IamResourceFormMode::Create, "サービスプリンシパル", 0) => &self.name,
            (IamResourceFormMode::Create, "サービスプリンシパル", 1) => &self.description,
            (IamResourceFormMode::Create, "サービスプリンシパル", 2) => &self.extra,
            (IamResourceFormMode::Edit, "ユーザー", 0) => &self.name,
            (IamResourceFormMode::Edit, "ユーザー", 1) => &self.password,
            (IamResourceFormMode::Edit, "ユーザー", 2) => &self.description,
            (IamResourceFormMode::Edit, _, 0) => &self.name,
            (IamResourceFormMode::Edit, _, 1) => &self.description,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match (self.mode, self.resource_type.as_str(), index) {
            (IamResourceFormMode::Create, "ユーザー", 0) => Some(&mut self.name),
            (IamResourceFormMode::Create, "ユーザー", 1) => Some(&mut self.code),
            (IamResourceFormMode::Create, "ユーザー", 2) => Some(&mut self.password),
            (IamResourceFormMode::Create, "ユーザー", 3) => Some(&mut self.description),
            (IamResourceFormMode::Create, "ユーザー", 4) => Some(&mut self.extra),
            (IamResourceFormMode::Create, "プロジェクト", 0) => Some(&mut self.name),
            (IamResourceFormMode::Create, "プロジェクト", 1) => Some(&mut self.code),
            (IamResourceFormMode::Create, "プロジェクト", 2) => Some(&mut self.description),
            (IamResourceFormMode::Create, "プロジェクト", 3) => Some(&mut self.extra),
            (IamResourceFormMode::Create, "サービスプリンシパル", 0) => {
                Some(&mut self.name)
            }
            (IamResourceFormMode::Create, "サービスプリンシパル", 1) => {
                Some(&mut self.description)
            }
            (IamResourceFormMode::Create, "サービスプリンシパル", 2) => {
                Some(&mut self.extra)
            }
            (IamResourceFormMode::Edit, "ユーザー", 0) => Some(&mut self.name),
            (IamResourceFormMode::Edit, "ユーザー", 1) => Some(&mut self.password),
            (IamResourceFormMode::Edit, "ユーザー", 2) => Some(&mut self.description),
            (IamResourceFormMode::Edit, _, 0) => Some(&mut self.name),
            (IamResourceFormMode::Edit, _, 1) => Some(&mut self.description),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IamRoleForm {
    pub grant: bool,
    pub project_id: String,
    pub principal_type: String,
    pub principal_id: String,
    pub role_id: String,
    pub field: usize,
}

impl IamRoleForm {
    pub const LABELS: [&'static str; 4] = [
        "プロジェクトID",
        "プリンシパル種別",
        "プリンシパルID",
        "ロールID",
    ];
    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.project_id,
            1 => &self.principal_type,
            2 => &self.principal_id,
            3 => &self.role_id,
            _ => "",
        }
    }
    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.project_id),
            1 => Some(&mut self.principal_type),
            2 => Some(&mut self.principal_id),
            3 => Some(&mut self.role_id),
            _ => None,
        }
    }
}

impl RegistryForm {
    /// モードごとの入力欄（ラベル, 値の取り出し）。
    pub fn labels(&self) -> &'static [&'static str] {
        match self.mode {
            RegistryFormMode::Create => &["名前", "サブドメイン", "説明"],
            RegistryFormMode::Edit => &["名前", "説明", "独自ドメイン"],
        }
    }

    pub fn value(&self, index: usize) -> &str {
        match (self.mode, index) {
            (RegistryFormMode::Create, 0) | (RegistryFormMode::Edit, 0) => &self.name,
            (RegistryFormMode::Create, 1) => &self.subdomain,
            (RegistryFormMode::Create, 2) | (RegistryFormMode::Edit, 1) => &self.description,
            (RegistryFormMode::Edit, 2) => &self.virtual_domain,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match (self.mode, index) {
            (RegistryFormMode::Create, 0) | (RegistryFormMode::Edit, 0) => Some(&mut self.name),
            (RegistryFormMode::Create, 1) => Some(&mut self.subdomain),
            (RegistryFormMode::Create, 2) | (RegistryFormMode::Edit, 1) => {
                Some(&mut self.description)
            }
            (RegistryFormMode::Edit, 2) => Some(&mut self.virtual_domain),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchFormMode {
    Create,
    Edit,
}

/// スイッチの作成・編集フォーム。
#[derive(Debug, Clone)]
pub struct SwitchForm {
    pub mode: SwitchFormMode,
    /// 編集時の対象。作成時は `None`。
    pub target: Option<Switch>,
    pub name: String,
    pub description: String,
    pub field: usize,
}

impl SwitchForm {
    pub const LABELS: [&'static str; 2] = ["名前", "説明"];

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.description,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.description),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsRecordFormMode {
    Add,
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsZoneFormMode {
    Create,
    Edit,
}

/// DNSゾーンの作成・説明編集フォーム。
#[derive(Debug, Clone)]
pub struct DnsZoneForm {
    pub mode: DnsZoneFormMode,
    pub target: Option<DnsZone>,
    pub name: String,
    pub description: String,
    pub field: usize,
}

impl DnsZoneForm {
    pub const LABELS: [&'static str; 2] = ["ゾーン名", "説明"];

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.description,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match (self.mode, index) {
            (DnsZoneFormMode::Create, 0) => Some(&mut self.name),
            (_, 1) => Some(&mut self.description),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleMonitorFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct SimpleMonitorForm {
    pub mode: SimpleMonitorFormMode,
    pub target_monitor: Option<SimpleMonitor>,
    pub target: String,
    pub description: String,
    pub protocol: usize,
    pub port: String,
    pub path: String,
    pub expected_status: String,
    pub delay_loop: String,
    pub timeout: String,
    pub enabled: bool,
    pub notify_email: bool,
    pub field: usize,
}

impl SimpleMonitorForm {
    pub const PROTOCOLS: [&'static str; 4] = ["ping", "tcp", "http", "https"];
    pub const FIELDS: usize = 10;

    pub fn protocol(&self) -> &'static str {
        Self::PROTOCOLS[self.protocol]
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match (self.mode, index) {
            (SimpleMonitorFormMode::Create, 0) => Some(&mut self.target),
            (_, 1) => Some(&mut self.description),
            (_, 3) => Some(&mut self.port),
            (_, 4) => Some(&mut self.path),
            (_, 5) => Some(&mut self.expected_status),
            (_, 6) => Some(&mut self.delay_loop),
            (_, 7) => Some(&mut self.timeout),
            _ => None,
        }
    }
}

/// DNSレコードの追加・編集フォーム。
#[derive(Debug, Clone)]
pub struct DnsRecordForm {
    pub mode: DnsRecordFormMode,
    pub zone: DnsZone,
    pub original: Option<DnsRecord>,
    pub name: String,
    pub record_type: String,
    pub data: String,
    pub ttl: String,
    pub field: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultFormMode {
    Create,
    Edit,
}

/// Vault の作成・編集フォーム。
#[derive(Debug, Clone)]
pub struct VaultForm {
    pub mode: VaultFormMode,
    pub target: Option<Vault>,
    pub name: String,
    pub description: String,
    pub kms_key_id: String,
    /// カンマ区切りで入力する。
    pub tags: String,
    pub field: usize,
}

impl VaultForm {
    pub const LABELS: [&'static str; 4] = ["名前", "説明", "KMS鍵ID", "タグ"];

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.description,
            2 => &self.kms_key_id,
            3 => &self.tags,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match (self.mode, index) {
            (_, 0) => Some(&mut self.name),
            (_, 1) => Some(&mut self.description),
            (VaultFormMode::Create, 2) => Some(&mut self.kms_key_id),
            (_, 3) => Some(&mut self.tags),
            _ => None,
        }
    }

    fn tags(&self) -> Vec<String> {
        self.tags
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_string)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretFormMode {
    Create,
    Update,
}

/// 値を扱うため `Debug` は必ず伏せる。
#[derive(Clone)]
pub struct SecretForm {
    pub mode: SecretFormMode,
    pub vault: Vault,
    pub name: String,
    pub value: String,
    pub field: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertProjectFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct AlertProjectForm {
    pub mode: AlertProjectFormMode,
    pub target: Option<AlertProject>,
    pub name: String,
    pub description: String,
    pub field: usize,
}

impl AlertProjectForm {
    pub const LABELS: [&'static str; 2] = ["名前", "説明"];

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.description,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.description),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertRuleFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct AlertRuleForm {
    pub mode: AlertRuleFormMode,
    pub project: AlertProject,
    pub target: Option<AlertRule>,
    pub metrics_storage_id: String,
    pub name: String,
    pub query: String,
    pub warning_enabled: bool,
    pub threshold_warning: String,
    pub duration_warning: String,
    pub critical_enabled: bool,
    pub threshold_critical: String,
    pub duration_critical: String,
    pub field: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMeasureRuleFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct LogMeasureRuleForm {
    pub mode: LogMeasureRuleFormMode,
    pub project: AlertProject,
    pub target: Option<LogMeasureRule>,
    pub log_storage_id: String,
    pub metrics_storage_id: String,
    pub name: String,
    pub description: String,
    pub rule_json: String,
    pub field: usize,
}

impl LogMeasureRuleForm {
    pub const FIELDS: usize = 5;

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.log_storage_id),
            1 => Some(&mut self.metrics_storage_id),
            2 => Some(&mut self.name),
            3 => Some(&mut self.description),
            4 => Some(&mut self.rule_json),
            _ => None,
        }
    }

    fn input(&self) -> Result<LogMeasureRuleInput, String> {
        let log_storage_id = self
            .log_storage_id
            .trim()
            .parse::<i64>()
            .map_err(|_| "ログストレージIDは数値で入力してください".to_string())?;
        let metrics_storage_id = self
            .metrics_storage_id
            .trim()
            .parse::<i64>()
            .map_err(|_| "メトリクスストレージIDは数値で入力してください".to_string())?;
        if log_storage_id <= 0 || metrics_storage_id <= 0 {
            return Err("ログ／メトリクスストレージIDを入力してください".to_string());
        }
        if self.name.trim().is_empty() {
            return Err("ルール名を入力してください".to_string());
        }
        let rule: serde_json::Value = serde_json::from_str(self.rule_json.trim())
            .map_err(|err| format!("ルールJSONが不正です: {err}"))?;
        if rule.get("version").and_then(serde_json::Value::as_str) != Some("v1") {
            return Err("ルールJSONの version は v1 を指定してください".to_string());
        }
        if !rule
            .pointer("/query/matchers")
            .is_some_and(serde_json::Value::is_array)
        {
            return Err("ルールJSONには query.matchers 配列が必要です".to_string());
        }
        Ok(LogMeasureRuleInput {
            log_storage_id,
            metrics_storage_id,
            name: self.name.trim().to_string(),
            description: self.description.trim().to_string(),
            rule,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogRoutingFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct LogRoutingForm {
    pub mode: LogRoutingFormMode,
    pub target: Option<LogRouting>,
    pub publisher_code: String,
    pub variant: String,
    pub resource_id: String,
    pub log_storage_id: String,
    pub publishers: Vec<Publisher>,
    pub publisher_index: usize,
    pub variant_index: usize,
    pub field: usize,
}

impl LogRoutingForm {
    pub const FIELDS: usize = 4;

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.publisher_code),
            1 => Some(&mut self.variant),
            2 => Some(&mut self.resource_id),
            3 => Some(&mut self.log_storage_id),
            _ => None,
        }
    }

    fn input(&self) -> Result<LogRoutingInput, String> {
        let publisher_code = self
            .publishers
            .get(self.publisher_index)
            .map(|publisher| publisher.code.as_str())
            .unwrap_or(&self.publisher_code);
        let variant = self
            .publishers
            .get(self.publisher_index)
            .and_then(|publisher| publisher.variants.get(self.variant_index))
            .map(|variant| variant.name.as_str())
            .unwrap_or(&self.variant);
        if publisher_code.trim().is_empty() || variant.trim().is_empty() {
            return Err("パブリッシャーコードとバリアントを入力してください".to_string());
        }
        let resource_id = if self.resource_id.trim().is_empty() {
            None
        } else {
            Some(
                self.resource_id
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| "リソースIDは数値で入力してください".to_string())?,
            )
        };
        let log_storage_id = self
            .log_storage_id
            .trim()
            .parse::<i64>()
            .map_err(|_| "ログストレージIDは数値で入力してください".to_string())?;
        if log_storage_id <= 0 {
            return Err("ログストレージIDを入力してください".to_string());
        }
        Ok(LogRoutingInput {
            publisher_code: publisher_code.trim().to_string(),
            resource_id,
            variant: variant.trim().to_string(),
            log_storage_id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsRoutingFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct MetricsRoutingForm {
    pub mode: MetricsRoutingFormMode,
    pub target: Option<MetricsRouting>,
    pub publisher_code: String,
    pub variant: String,
    pub resource_id: String,
    pub metrics_storage_id: String,
    pub publishers: Vec<Publisher>,
    pub publisher_index: usize,
    pub variant_index: usize,
    pub field: usize,
}

impl MetricsRoutingForm {
    pub const FIELDS: usize = 4;

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.publisher_code),
            1 => Some(&mut self.variant),
            2 => Some(&mut self.resource_id),
            3 => Some(&mut self.metrics_storage_id),
            _ => None,
        }
    }

    fn input(&self) -> Result<MetricsRoutingInput, String> {
        let publisher_code = self
            .publishers
            .get(self.publisher_index)
            .map(|publisher| publisher.code.as_str())
            .unwrap_or(&self.publisher_code);
        let variant = self
            .publishers
            .get(self.publisher_index)
            .and_then(|publisher| publisher.variants.get(self.variant_index))
            .map(|variant| variant.name.as_str())
            .unwrap_or(&self.variant);
        let resource_id = if self.resource_id.trim().is_empty() {
            None
        } else {
            Some(
                self.resource_id
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| "リソースIDは数値で入力してください".to_string())?,
            )
        };
        let metrics_storage_id = self
            .metrics_storage_id
            .trim()
            .parse::<i64>()
            .map_err(|_| "メトリクスストレージIDは数値で入力してください".to_string())?;
        if publisher_code.trim().is_empty() || variant.trim().is_empty() {
            return Err("パブリッシャーとバリアントを選択してください".to_string());
        }
        Ok(MetricsRoutingInput {
            publisher_code: publisher_code.trim().to_string(),
            resource_id,
            variant: variant.trim().to_string(),
            metrics_storage_id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct DashboardForm {
    pub mode: DashboardFormMode,
    pub target: Option<DashboardProject>,
    pub name: String,
    pub description: String,
    pub field: usize,
}

impl DashboardForm {
    pub const FIELDS: usize = 2;
    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.description),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationTargetFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct NotificationTargetForm {
    pub mode: NotificationTargetFormMode,
    pub project: AlertProject,
    pub target: Option<NotificationTarget>,
    pub service_type: usize,
    pub url: String,
    pub description: String,
    pub field: usize,
}

impl NotificationTargetForm {
    pub const SERVICE_TYPES: [&'static str; 2] = ["SAKURA_SIMPLE_NOTICE", "SAKURA_EVENT_BUS"];
    pub const FIELDS: usize = 3;

    pub fn service_type(&self) -> &'static str {
        Self::SERVICE_TYPES[self.service_type]
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            1 => Some(&mut self.url),
            2 => Some(&mut self.description),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationRoutingFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct NotificationRoutingForm {
    pub mode: NotificationRoutingFormMode,
    pub project: AlertProject,
    pub target: Option<NotificationRouting>,
    pub targets: Vec<NotificationTarget>,
    pub target_index: usize,
    pub resend_interval: String,
    pub match_labels: String,
    pub field: usize,
}

impl NotificationRoutingForm {
    pub const FIELDS: usize = 3;

    pub fn selected_target(&self) -> Option<&NotificationTarget> {
        self.targets.get(self.target_index)
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            1 => Some(&mut self.resend_interval),
            2 => Some(&mut self.match_labels),
            _ => None,
        }
    }
}

impl AlertRuleForm {
    pub const FIELDS: usize = 9;

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.metrics_storage_id),
            1 => Some(&mut self.name),
            2 => Some(&mut self.query),
            4 => Some(&mut self.threshold_warning),
            5 => Some(&mut self.duration_warning),
            7 => Some(&mut self.threshold_critical),
            8 => Some(&mut self.duration_critical),
            _ => None,
        }
    }

    fn input(&self) -> Result<AlertRuleInput, String> {
        let metrics_storage_id = self
            .metrics_storage_id
            .trim()
            .parse::<i64>()
            .map_err(|_| "メトリクスストレージIDは数値で入力してください".to_string())?;
        let duration_warning = self
            .duration_warning
            .trim()
            .parse::<i64>()
            .map_err(|_| "警告の継続時間は秒数で入力してください".to_string())?;
        let duration_critical = self
            .duration_critical
            .trim()
            .parse::<i64>()
            .map_err(|_| "重大の継続時間は秒数で入力してください".to_string())?;
        if metrics_storage_id <= 0 {
            return Err("メトリクスストレージIDを入力してください".to_string());
        }
        if duration_warning < 0 || duration_critical < 0 {
            return Err("継続時間は0秒以上で入力してください".to_string());
        }
        if self.name.trim().is_empty() || self.query.trim().is_empty() {
            return Err("名前とクエリを入力してください".to_string());
        }
        if self.warning_enabled && self.threshold_warning.trim().is_empty() {
            return Err("警告を有効にする場合はしきい値が必要です".to_string());
        }
        if self.critical_enabled && self.threshold_critical.trim().is_empty() {
            return Err("重大を有効にする場合はしきい値が必要です".to_string());
        }
        Ok(AlertRuleInput {
            metrics_storage_id,
            name: self.name.trim().to_string(),
            query: self.query.trim().to_string(),
            warning_enabled: self.warning_enabled,
            critical_enabled: self.critical_enabled,
            threshold_warning: self.threshold_warning.trim().to_string(),
            threshold_critical: self.threshold_critical.trim().to_string(),
            duration_warning,
            duration_critical,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct StorageForm {
    pub mode: StorageFormMode,
    pub target: Option<Storage>,
    pub kind: StorageKind,
    pub is_system: bool,
    pub classification: usize,
    pub name: String,
    pub description: String,
    pub field: usize,
}

#[derive(Debug, Clone)]
pub struct StorageRetentionForm {
    pub storage: Storage,
    pub days: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageAccessKeyFormMode {
    Create,
    Edit,
}

#[derive(Debug, Clone)]
pub struct StorageAccessKeyForm {
    pub mode: StorageAccessKeyFormMode,
    pub storage: Storage,
    pub target: Option<StorageAccessKey>,
    pub description: String,
}

impl StorageForm {
    pub const KINDS: [StorageKind; 3] =
        [StorageKind::Logs, StorageKind::Metrics, StorageKind::Traces];
    pub const CLASSIFICATIONS: [&'static str; 2] = ["shared", "dedicated"];
    pub const FIELDS: usize = 5;

    pub fn classification(&self) -> &'static str {
        Self::CLASSIFICATIONS[self.classification]
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            3 => Some(&mut self.name),
            4 => Some(&mut self.description),
            _ => None,
        }
    }
}

impl std::fmt::Debug for SecretForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretForm")
            .field("mode", &self.mode)
            .field("vault_id", &self.vault.id)
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .field("field", &self.field)
            .finish()
    }
}

impl SecretForm {
    pub const FIELDS: usize = 2;

    pub fn new(mode: SecretFormMode, vault: Vault, name: String) -> Self {
        Self {
            mode,
            vault,
            name,
            value: String::new(),
            field: if mode == SecretFormMode::Create { 0 } else { 1 },
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match (self.mode, index) {
            (SecretFormMode::Create, 0) => Some(&mut self.name),
            (_, 1) => Some(&mut self.value),
            _ => None,
        }
    }
}

impl DnsRecordForm {
    pub const LABELS: [&'static str; 4] = ["名前", "種別", "値", "TTL"];

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.record_type,
            2 => &self.data,
            3 => &self.ttl,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.record_type),
            2 => Some(&mut self.data),
            3 => Some(&mut self.ttl),
            _ => None,
        }
    }
}

/// 確認ダイアログで実行する操作。
#[derive(Debug, Clone)]
pub enum ConfirmAction {
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
            Service::PacketFilter => Some(CloudResourceKind::PacketFilter),
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
            Service::Server => Pane::Servers,
            Service::Switch => Pane::Switches,
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::PacketFilter
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
                // モデル一覧はマネージドリソースの枠をそのまま使う。
                AiEngineTab::Models => Pane::ManagedResources,
                AiEngineTab::Documents => Pane::AiEngineDocuments,
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
            Service::Switch => self.switch_ensure_loaded(),
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::PacketFilter
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

    /// 保存済みのログイン情報があれば自動でクライアントを作る。
    ///
    /// パスワードの取り出しはキーチェーンに触るため別スレッドで行う。
    /// UI スレッドで呼ぶと、OS の確認ダイアログが出ている間 TUI が固まる。
    fn try_auto_login(&mut self, host: &str) {
        // 一度試したホストは再試行しない。
        //
        // `ensure_loaded` はキー入力とメッセージのたびに走るため、ここで印を
        // 付けないと失敗するたびに読み直してしまう。キーチェーンは読むたびに
        // OS の確認ダイアログを出しうるので、それが延々と繰り返される。
        if !self.registry.auto_login_tried.insert(host.to_string()) {
            return;
        }
        if !self.config.registries.contains_key(host) {
            return;
        }
        self.registry
            .repositories
            .insert(host.to_string(), Loadable::Loading);
        self.inflight += 1;
        let config = self.config.clone();
        let tx = self.tx.clone();
        let host = host.to_string();
        tokio::task::spawn_blocking(move || {
            let login = config.registry_login(&host);
            let _ = tx.send(Message::SavedLogin { host, login });
        });
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
                        self.server.server_state.select(None);
                        self.server
                            .servers
                            .insert(zone.clone(), Loadable::Ready(servers));
                        if zone == self.zone {
                            self.set_status(
                                format!("{zone} のサーバー {count} 件"),
                                StatusKind::Info,
                            );
                        }
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
            Service::Switch => self.on_key_switch(key),
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::PacketFilter
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
            | Service::EnhancedDb
            | Service::AutoBackup => {}
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

    fn open_iam_credential_form(&mut self) {
        let credentials = match crate::config::load_iam_credentials(&self.credential_source) {
            Ok(credentials) => credentials,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                None
            }
        };
        self.overlay = Some(Overlay::IamCredentialForm(match credentials {
            Some(credentials) => IamCredentialForm {
                service_principal_id: credentials.service_principal_id,
                key_id: credentials.key_id,
                private_key: credentials.private_key,
                ..IamCredentialForm::default()
            },
            None => IamCredentialForm::default(),
        }));
    }

    fn submit_iam_credentials(&mut self, mut form: IamCredentialForm) {
        let credentials = form.credentials();
        if credentials.service_principal_id.is_empty()
            || credentials.key_id.is_empty()
            || credentials.private_key.is_empty()
        {
            self.set_status(
                "リソースID、キーID、RSA秘密鍵をすべて入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::IamCredentialForm(form));
            return;
        }
        if !credentials.private_key.contains("-----BEGIN")
            || !credentials.private_key.contains("PRIVATE KEY-----")
        {
            self.set_status("PEM形式のRSA秘密鍵を貼り付けてください", StatusKind::Error);
            self.overlay = Some(Overlay::IamCredentialForm(form));
            return;
        }
        form.verifying = true;
        self.overlay = Some(Overlay::IamCredentialForm(form.clone()));
        self.inflight += 1;
        self.set_status("IAMサービスプリンシパルを検証しています…", StatusKind::Info);
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .verify_iam_credentials(&credentials)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::IamCredentialsVerified {
                form: Box::new(form),
                result,
            });
        });
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
                    Service::Switch => sacloud.count_switches(&zone).await,
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
                    Service::PacketFilter => {
                        sacloud
                            .count_cloud_resources(&zone, CloudResourceKind::PacketFilter)
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
            Service::Switch => self.switch.switches.get(&self.zone)?.ready()?.len(),
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::PacketFilter
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
        match self.active_pane() {
            Pane::Registries | Pane::None => host(),
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

    fn open_profile_picker(&mut self) {
        let sources = crate::config::available_credential_sources();
        let index = sources
            .iter()
            .position(|s| *s == self.credential_source)
            .unwrap_or(0);
        let sources = sources
            .into_iter()
            .map(|source| {
                let zone = source.zone();
                (source, zone)
            })
            .collect();
        self.overlay = Some(Overlay::ProfilePicker { sources, index });
    }

    /// 認証情報が無い初回起動を、既存のプロファイル作成フォームへつなぐ。
    pub fn start_credential_setup(&mut self) {
        self.set_status(
            "認証情報が見つかりません。アプリ内で新しいプロファイルを作成してください",
            StatusKind::Info,
        );
        self.open_profile_form();
    }

    /// キーチェーンに預けた資格情報の削除を確認する。
    fn confirm_delete_credential(&mut self, source: &CredentialSource) {
        let CredentialSource::Keychain(name) = source else {
            self.set_status(
                "削除できるのはキーチェーンに保存したものだけです（usacloud のプロファイルは他のツールも使うため消しません）",
                StatusKind::Info,
            );
            return;
        };
        if *source == self.credential_source {
            self.set_status("使用中の資格情報は削除できません", StatusKind::Info);
            return;
        }
        self.overlay = Some(Overlay::Confirm {
            title: "資格情報の削除".to_string(),
            body: format!(
                "「{name}」をキーチェーンと設定ファイルから削除します。\n\
                 アクセストークン自体はさくらのコントロールパネルに残ります。"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteCredential { name: name.clone() },
        });
    }

    /// 資格情報の作成フォームを開く。
    fn open_profile_form(&mut self) {
        // 接続先は本番と社内テストから選ぶ。他の環境は --api-root で指定する。
        let current = self.api_root.clone();
        let mut api_roots = vec![
            ApiRootChoice {
                label: "本番 (cloud)",
                url: crate::config::DEFAULT_API_ROOT.to_string(),
            },
            ApiRootChoice {
                label: "テスト (cloud-test)",
                url: crate::config::TEST_API_ROOT.to_string(),
            },
        ];
        // 起動時に別の接続先を指定していれば、それも選べるようにする。
        if !api_roots.iter().any(|r| r.url == current) {
            api_roots.push(ApiRootChoice {
                label: "起動時の指定",
                url: current.clone(),
            });
        }
        let api_root_index = api_roots.iter().position(|r| r.url == current).unwrap_or(0);

        // ゾーンは接続先に対応するものを出す。
        // 既に API から取れていればそちらを優先する（環境の実態に一番近い）。
        let zones = match self.zones.ready() {
            Some(zones) if !zones.is_empty() => zones.clone(),
            _ => crate::iaas::known_zones_for(&current),
        };
        let zone_index = zones.iter().position(|z| z.name == self.zone).unwrap_or(0);

        self.overlay = Some(Overlay::ProfileForm(ProfileForm {
            zones,
            zone_index,
            api_roots,
            api_root_index,
            ..ProfileForm::default()
        }));
    }

    fn open_ai_engine_token_form(&mut self) {
        let entries = match crate::config::list_ai_engine_tokens(&self.credential_source) {
            Ok(entries) => entries,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                Vec::new()
            }
        };
        let index = entries.iter().position(|entry| entry.active).unwrap_or(0);
        self.overlay = Some(Overlay::AiEngineTokenForm(AiEngineTokenForm {
            entries,
            index,
            ..AiEngineTokenForm::default()
        }));
    }

    fn submit_ai_engine_token(&mut self, mut form: AiEngineTokenForm) {
        if let Err(err) = crate::config::validate_ai_engine_token_name(&form.name) {
            self.set_status(fmt_error(err), StatusKind::Error);
            self.overlay = Some(Overlay::AiEngineTokenForm(form));
            return;
        }
        let name = form.name.trim().to_string();
        let valid_shape = form
            .token
            .split_once(':')
            .is_some_and(|(id, secret)| !id.trim().is_empty() && !secret.trim().is_empty());
        if !valid_shape {
            self.set_status(
                "アカウントトークンを UUID:シークレット の形式で入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::AiEngineTokenForm(form));
            return;
        }
        let token = form.token.trim().to_string();
        let client = match AiEngineClient::new(token.clone()) {
            Ok(client) => Arc::new(client),
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                self.overlay = Some(Overlay::AiEngineTokenForm(form));
                return;
            }
        };
        form.verifying = true;
        self.overlay = Some(Overlay::AiEngineTokenForm(form));
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_models().await.map_err(fmt_error);
            let _ = tx.send(Message::AiEngineTokenVerified {
                name,
                token,
                result,
            });
        });
    }

    fn select_ai_engine_token(&mut self, name: &str) {
        let token = match crate::config::select_ai_engine_token(&self.credential_source, name) {
            Ok(Some(token)) => token,
            Ok(None) => {
                self.set_status("選択したトークンを読み出せませんでした", StatusKind::Error);
                return;
            }
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                return;
            }
        };
        match AiEngineClient::new(token) {
            Ok(client) => {
                self.ai_engine_client = Some(Arc::new(client));
                self.managed_resources
                    .items
                    .remove(&ManagedResourceKind::AiEngine);
                self.ai_engine_reset_rag();
                self.overlay = None;
                self.set_status(
                    format!("AI Engineトークン「{name}」へ切り替えました"),
                    StatusKind::Success,
                );
                self.managed_resources_ensure_loaded();
            }
            Err(err) => self.set_status(fmt_error(err), StatusKind::Error),
        }
    }

    fn confirm_delete_ai_engine_token(&mut self, name: String) {
        self.overlay = Some(Overlay::Confirm {
            title: "AI Engineトークンの削除".to_string(),
            body: format!(
                "このPCのキーチェーンからAI Engineトークン「{name}」を削除します。\n\
                 AI Engine側のトークンは失効しません。失効はコントロールパネルで行ってください。"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteAiEngineToken { name },
        });
    }

    fn copy_ai_engine_token(&mut self, name: &str, form: AiEngineTokenForm) {
        match crate::config::load_named_ai_engine_token(&self.credential_source, name) {
            Ok(Some(token)) => match copy_to_clipboard(&token) {
                Ok(()) => self.set_status(
                    format!("AI Engineトークン「{name}」をクリップボードへコピーしました"),
                    StatusKind::Success,
                ),
                Err(err) => self.set_status(
                    format!("クリップボードへコピーできませんでした: {err}"),
                    StatusKind::Error,
                ),
            },
            Ok(None) => {
                self.set_status("コピーできる保存済みトークンがありません", StatusKind::Info)
            }
            Err(err) => self.set_status(fmt_error(err), StatusKind::Error),
        }
        self.overlay = Some(Overlay::AiEngineTokenForm(form));
    }

    /// 入力内容を検証してから保存する。
    ///
    /// 打ち間違えたトークンを保存してしまわないよう、実際に API を 1 回叩いて
    /// 通ることを確かめてから書き出す。
    fn submit_profile_form(&mut self, mut form: ProfileForm) {
        if let Err(err) = crate::config::validate_profile_name(&form.name) {
            self.set_status(fmt_error(err), StatusKind::Error);
            self.overlay = Some(Overlay::ProfileForm(form));
            return;
        }
        // 見えない文字が混ざっていると 401 になるので、ここで落としてから使う。
        form.token = crate::config::clean_secret(&form.token);
        form.secret = crate::config::clean_secret(&form.secret);
        if form.token.is_empty() || form.secret.is_empty() {
            self.set_status(
                "アクセストークンとシークレットを入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::ProfileForm(form));
            return;
        }

        let credentials = crate::config::ApiCredentials {
            token: form.token.clone(),
            secret: form.secret.clone(),
            source: CredentialSource::Env,
            zone: Some(form.zone().name.clone()),
            api_root: Some(form.api_root().url.clone()),
        };
        let client = match SacloudClient::new(&credentials) {
            Ok(client) => client,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                self.overlay = Some(Overlay::ProfileForm(form));
                return;
            }
        };

        form.verifying = true;
        self.overlay = Some(Overlay::ProfileForm(form.clone()));
        self.inflight += 1;
        self.set_status("トークンを検証しています…", StatusKind::Info);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            // 「自分が誰か」を返すだけの auth-status で確かめる。
            // ゾーン一覧やリソース一覧は権限設定によっては読めないため、
            // 有効なキーでも失敗してしまう。
            let result = match client.billing_identity().await {
                // 認証が通ったら、その環境に実在するゾーンも拾っておく。
                // 環境ごとにゾーン名が違うため、以降はこれを使う。
                Ok(_) => Ok(client.list_zones().await.unwrap_or_default()),
                Err(err) => Err(fmt_error(err)),
            };
            let _ = tx.send(Message::ProfileVerified {
                form: Box::new(form),
                result,
            });
        });
    }

    /// 検証が通った資格情報を保存する。
    fn save_verified_profile(&mut self, form: ProfileForm) {
        let saved = match form.storage {
            ProfileStorage::Usacloud => crate::config::create_usacloud_profile(
                &form.name,
                &form.token,
                &form.secret,
                &form.zone().name,
                &form.api_root().url,
            ),
            ProfileStorage::Keychain => crate::config::create_keychain_credential(
                &form.name,
                &form.token,
                &form.secret,
                &form.zone().name,
                &form.api_root().url,
            ),
        };
        match saved {
            Ok(path) => {
                // 保存先の設定を読み直してから、一覧に反映した状態でピッカーへ戻る。
                if form.storage == ProfileStorage::Keychain
                    && let Ok(config) = Config::load()
                {
                    self.config = config;
                }
                let created_message = format!(
                    "{} を作成しました（{}）: {}",
                    form.name,
                    form.storage.title(),
                    path.display()
                );
                if !self.has_credentials {
                    let source = match form.storage {
                        ProfileStorage::Usacloud => CredentialSource::Profile(form.name.clone()),
                        ProfileStorage::Keychain => CredentialSource::Keychain(form.name.clone()),
                    };
                    let zone = form.zone().name.clone();
                    let api_root = form.api_root().url.clone();
                    let credentials = ApiCredentials {
                        token: form.token,
                        secret: form.secret,
                        source: source.clone(),
                        zone: Some(zone),
                        api_root: Some(api_root),
                    };
                    if self.apply_credentials(source, credentials) {
                        self.set_status(created_message, StatusKind::Success);
                        self.open_initial_service_picker();
                    }
                } else {
                    self.set_status(created_message, StatusKind::Success);
                    self.open_profile_picker();
                }
            }
            Err(err) => {
                self.pending_form = Some(Box::new(form));
                self.overlay = Some(Overlay::Message {
                    title: "作成に失敗しました".to_string(),
                    body: format!(
                        "{}\n\n閉じると入力内容を残したままフォームに戻ります。",
                        fmt_error(err)
                    ),
                    kind: StatusKind::Error,
                    scroll: 0,
                });
            }
        }
    }

    /// 認証情報に割り当てる色を順に切り替えて保存する。
    ///
    /// dev と prod のように名前が似ている契約を、自分で決めた色で
    /// 見分けられるようにするためのもの。既定色に戻すところまで一巡する。
    fn cycle_profile_color(&mut self, source: &CredentialSource) {
        let palette = crate::ui::PROFILE_COLORS;
        let next = match self.config.profile_color(source) {
            None => Some(palette[0].to_string()),
            Some(current) => palette
                .iter()
                .position(|c| *c == current)
                .map(|i| i + 1)
                .filter(|i| *i < palette.len())
                .map(|i| palette[i].to_string()),
        };
        self.config.set_profile_color(source, next.clone());

        match self.config.save() {
            Ok(_) => {
                let label = next.as_deref().unwrap_or("既定");
                self.set_status(
                    format!("{} の色を {label} にしました", source.label()),
                    StatusKind::Success,
                );
            }
            Err(err) => self.set_status(
                format!("設定の保存に失敗しました: {}", fmt_error(err)),
                StatusKind::Error,
            ),
        }
    }

    /// 認証情報の読み込みを別スレッドで始める。
    ///
    /// キーチェーンの読み出しは OS が確認ダイアログを出すことがあり、
    /// その間ブロックする。UI スレッドで呼ぶと TUI ごと固まるため切り離す。
    fn switch_credentials(&mut self, source: CredentialSource) {
        if source == self.credential_source {
            return;
        }
        self.inflight += 1;
        self.set_status(
            format!("{} の認証情報を読み込んでいます…", source.label()),
            StatusKind::Info,
        );
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = crate::config::load_credentials_from(&source).map_err(fmt_error);
            let _ = tx.send(Message::CredentialsLoaded {
                source: Box::new(source),
                result: Box::new(result),
            });
        });
    }

    /// 読み込めた認証情報に切り替え、クラウド API 側のキャッシュを捨てて読み直す。
    ///
    /// レジストリへのログインはホスト単位でクラウドの契約とは独立なので保持する。
    fn apply_credentials(&mut self, source: CredentialSource, credentials: ApiCredentials) -> bool {
        let was_configured = self.has_credentials;
        // 世代を進める。前の資格情報で投げた通信の結果は、
        // これ以降に届いても画面に入らない。
        self.epoch += 1;
        self.tx.epoch = self.epoch;
        // 各サービスのクライアントを作り直す。
        let clients = (
            SacloudClient::new(&credentials),
            AppRunClient::new(&credentials),
            DedicatedClient::new(&credentials),
            MonitoringClient::new(&credentials),
            ApiGatewayClient::new(&credentials),
        );
        let (sacloud, apprun, dedicated, monitoring, api_gateway) = match clients {
            (Ok(a), Ok(b), Ok(c), Ok(d), Ok(e)) => (a, b, c, d, e),
            _ => {
                self.show_error(
                    "クライアントを初期化できませんでした",
                    format!("{} への切り替えを中止しました", source.label()),
                );
                return false;
            }
        };

        self.api_root = credentials.api_root().to_string();
        // ゾーン名は環境ごとに違う（本番の is1a は cloud-test には無い）。
        // 切り替え先の既定ゾーンに合わせないと、ゾーン依存のサービスが全て 404 になる。
        self.zone = sacloud.default_zone().to_string();

        self.sacloud = Arc::new(sacloud);
        self.apprun_client = Arc::new(apprun);
        self.dedicated_client = Arc::new(dedicated);
        self.monitoring_client = Arc::new(monitoring);
        self.api_gateway_client = Arc::new(api_gateway);
        self.ai_engine_client = None;
        self.credential_source = source;
        self.has_credentials = true;

        // 契約が変われば、取得済みのものは全て別アカウントのもの。
        // どれか一つでも残すと、切り替えたのに前の内容が見える。
        self.zones = Loadable::Idle;
        self.zone_counts.clear();
        self.service_counts.clear();
        self.invalidate_all();
        self.registry.registries = Loadable::Idle;
        self.registry_clients = RegistryClients::default();
        self.filters = Filters::default();

        self.set_status(
            format!(
                "{} に切り替えました（ゾーン {}）",
                self.credential_source.label(),
                self.zone
            ),
            StatusKind::Info,
        );
        // 表示中のサービスを読み直す。レジストリだけ読むと、他のサービスに
        // 移ったときに前のアカウントの内容が残って見える。
        if was_configured {
            self.ensure_loaded();
        }
        true
    }

    /// 現在のビューのキャッシュを捨てて読み直す。
    fn refresh(&mut self) {
        match self.service {
            Service::Registry => self.registry_refresh(),
            Service::AppRun => self.apprun_refresh(),
            Service::Dedicated => self.dedicated_refresh(),
            Service::Server => self.server_refresh(),
            Service::Switch => self.switch_refresh(),
            Service::Disk
            | Service::Archive
            | Service::IsoImage
            | Service::Internet
            | Service::PacketFilter
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

    fn open_login(&mut self) {
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let host = registry.host().to_string();
        if host.is_empty() {
            self.set_status(
                "このレジストリにはホスト名が割り当てられていません",
                StatusKind::Error,
            );
            return;
        }
        self.registry.tab = Tab::Images;
        self.registry.focus = Focus::Detail;
        let accounts = self.config.registry_account_names(&host);
        if accounts.is_empty() {
            self.overlay = Some(Overlay::Login(LoginForm {
                username: String::new(),
                password: String::new(),
                save: false,
                host,
                field: 0,
            }));
        } else {
            self.overlay = Some(Overlay::LoginPicker {
                host,
                accounts,
                index: 0,
            });
        }
    }

    /// 保存済みのユーザー名を選んでログインする。パスワードの取り出しは
    /// キーチェーンに触るため別スレッドで行う。
    fn login_with_saved_account(&mut self, host: String, username: String) {
        // これから試すので、前に「試した」印がついていても関係ない。
        self.registry.auto_login_tried.insert(host.clone());
        self.set_status(format!("{host} に接続中…"), StatusKind::Info);
        let config = self.config.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        tokio::task::spawn_blocking(move || {
            let login = config.registry_user_login(&host, &username);
            let _ = tx.send(Message::SavedLogin { host, login });
        });
    }

    fn confirm_forget_login(&mut self) {
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let host = registry.host().to_string();
        if self.registry_clients.get(&host).is_none() && !self.config.registries.contains_key(&host)
        {
            return;
        }
        self.overlay = Some(Overlay::Confirm {
            title: "ログイン情報の削除".to_string(),
            body: format!(
                "{host} のログイン情報を破棄します。\n設定ファイルに保存済みの場合はそこからも削除されます。"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::ForgetLogin { host },
        });
    }

    fn submit_login(&mut self, form: LoginForm) {
        if form.username.is_empty() || form.password.is_empty() {
            self.set_status(
                "ユーザー名とパスワードを入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::Login(form));
            return;
        }
        let login = RegistryLogin {
            username: form.username,
            password: form.password,
        };
        let client = match self.registry_clients.insert(&form.host, login.clone()) {
            Ok(client) => client,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                return;
            }
        };
        let host = form.host;
        let save = form.save;
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status(format!("{host} に接続中…"), StatusKind::Info);
        tokio::spawn(async move {
            let result = client.verify().await.map_err(fmt_error);
            let _ = tx.send(Message::LoginVerified {
                host,
                login,
                save,
                result,
            });
        });
    }

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

fn edit_user_form(form: &mut UserForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % UserForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + UserForm::FIELDS - 1) % UserForm::FIELDS
        }
        KeyCode::Left if form.field == 2 => {
            form.permission = (form.permission + Permission::ALL.len() - 1) % Permission::ALL.len()
        }
        KeyCode::Right | KeyCode::Char(' ') if form.field == 2 => {
            form.permission = (form.permission + 1) % Permission::ALL.len()
        }
        KeyCode::Backspace => match form.field {
            // 編集モードではユーザー名を変更できない。
            0 if form.mode == UserFormMode::Add => {
                form.username.pop();
            }
            1 => {
                form.password.pop();
            }
            _ => {}
        },
        KeyCode::Char(c) => match form.field {
            0 if form.mode == UserFormMode::Add => form.username.push(c),
            1 => form.password.push(c),
            _ => {}
        },
        _ => {}
    }
}

fn edit_registry_form(form: &mut RegistryForm, key: KeyEvent) {
    let fields = form.labels().len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_iam_resource_form(form: &mut IamResourceForm, key: KeyEvent) {
    let fields = form.labels().len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_iam_role_form(form: &mut IamRoleForm, key: KeyEvent) {
    let fields = IamRoleForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_switch_form(form: &mut SwitchForm, key: KeyEvent) {
    let fields = SwitchForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_dns_record_form(form: &mut DnsRecordForm, key: KeyEvent) {
    let fields = DnsRecordForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_dns_zone_form(form: &mut DnsZoneForm, key: KeyEvent) {
    let fields = DnsZoneForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_simple_monitor_form(form: &mut SimpleMonitorForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % SimpleMonitorForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + SimpleMonitorForm::FIELDS - 1) % SimpleMonitorForm::FIELDS
        }
        KeyCode::Left if form.field == 2 => {
            form.protocol = (form.protocol + SimpleMonitorForm::PROTOCOLS.len() - 1)
                % SimpleMonitorForm::PROTOCOLS.len()
        }
        KeyCode::Right | KeyCode::Char(' ') if form.field == 2 => {
            form.protocol = (form.protocol + 1) % SimpleMonitorForm::PROTOCOLS.len()
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 8 => {
            form.enabled = !form.enabled
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 9 => {
            form.notify_email = !form.notify_email
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_vault_form(form: &mut VaultForm, key: KeyEvent) {
    let fields = VaultForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_alert_project_form(form: &mut AlertProjectForm, key: KeyEvent) {
    let fields = AlertProjectForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_alert_rule_form(form: &mut AlertRuleForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % AlertRuleForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + AlertRuleForm::FIELDS - 1) % AlertRuleForm::FIELDS
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 3 => {
            form.warning_enabled = !form.warning_enabled
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 6 => {
            form.critical_enabled = !form.critical_enabled
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_log_measure_rule_form(form: &mut LogMeasureRuleForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % LogMeasureRuleForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + LogMeasureRuleForm::FIELDS - 1) % LogMeasureRuleForm::FIELDS
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_log_routing_form(form: &mut LogRoutingForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % LogRoutingForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + LogRoutingForm::FIELDS - 1) % LogRoutingForm::FIELDS
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            if form.field == 0 && !form.publishers.is_empty() =>
        {
            let delta = if key.code == KeyCode::Left {
                form.publishers.len() - 1
            } else {
                1
            };
            form.publisher_index = (form.publisher_index + delta) % form.publishers.len();
            form.variant_index = 0;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            if form.field == 1 && !form.publishers.is_empty() =>
        {
            let len = form.publishers[form.publisher_index].variants.len();
            if len > 0 {
                let delta = if key.code == KeyCode::Left {
                    len - 1
                } else {
                    1
                };
                form.variant_index = (form.variant_index + delta) % len;
            }
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_metrics_routing_form(form: &mut MetricsRoutingForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % MetricsRoutingForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + MetricsRoutingForm::FIELDS - 1) % MetricsRoutingForm::FIELDS
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            if form.field == 0 && !form.publishers.is_empty() =>
        {
            let delta = if key.code == KeyCode::Left {
                form.publishers.len() - 1
            } else {
                1
            };
            form.publisher_index = (form.publisher_index + delta) % form.publishers.len();
            form.variant_index = 0;
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            if form.field == 1 && !form.publishers.is_empty() =>
        {
            let len = form.publishers[form.publisher_index].variants.len();
            if len > 0 {
                let delta = if key.code == KeyCode::Left {
                    len - 1
                } else {
                    1
                };
                form.variant_index = (form.variant_index + delta) % len;
            }
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_dashboard_form(form: &mut DashboardForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % DashboardForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + DashboardForm::FIELDS - 1) % DashboardForm::FIELDS
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_notification_target_form(form: &mut NotificationTargetForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            form.field = (form.field + 1) % NotificationTargetForm::FIELDS
        }
        KeyCode::BackTab | KeyCode::Up => {
            form.field =
                (form.field + NotificationTargetForm::FIELDS - 1) % NotificationTargetForm::FIELDS
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 0 => {
            form.service_type =
                (form.service_type + 1) % NotificationTargetForm::SERVICE_TYPES.len()
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_notification_routing_form(form: &mut NotificationRoutingForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => {
            form.field = (form.field + 1) % NotificationRoutingForm::FIELDS
        }
        KeyCode::BackTab | KeyCode::Up => {
            form.field =
                (form.field + NotificationRoutingForm::FIELDS - 1) % NotificationRoutingForm::FIELDS
        }
        KeyCode::Left if form.field == 0 && !form.targets.is_empty() => {
            form.target_index = (form.target_index + form.targets.len() - 1) % form.targets.len()
        }
        KeyCode::Right | KeyCode::Char(' ') if form.field == 0 && !form.targets.is_empty() => {
            form.target_index = (form.target_index + 1) % form.targets.len()
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_storage_form(form: &mut StorageForm, key: KeyEvent) {
    let fields = if form.mode == StorageFormMode::Create {
        StorageForm::FIELDS
    } else {
        2
    };
    match key.code {
        KeyCode::Tab | KeyCode::Down if form.mode == StorageFormMode::Create => {
            form.field = (form.field + 1) % fields
        }
        KeyCode::BackTab | KeyCode::Up if form.mode == StorageFormMode::Create => {
            form.field = (form.field + fields - 1) % fields
        }
        KeyCode::Tab | KeyCode::Down => form.field = if form.field == 3 { 4 } else { 3 },
        KeyCode::BackTab | KeyCode::Up => form.field = if form.field == 3 { 4 } else { 3 },
        KeyCode::Left if form.mode == StorageFormMode::Create && form.field == 0 => {
            let index = StorageForm::KINDS
                .iter()
                .position(|kind| *kind == form.kind)
                .unwrap_or(0);
            form.kind = StorageForm::KINDS
                [(index + StorageForm::KINDS.len() - 1) % StorageForm::KINDS.len()];
            if form.kind == StorageKind::Traces {
                form.is_system = false;
            }
        }
        KeyCode::Right | KeyCode::Char(' ')
            if form.mode == StorageFormMode::Create && form.field == 0 =>
        {
            let index = StorageForm::KINDS
                .iter()
                .position(|kind| *kind == form.kind)
                .unwrap_or(0);
            form.kind = StorageForm::KINDS[(index + 1) % StorageForm::KINDS.len()];
            if form.kind == StorageKind::Traces {
                form.is_system = false;
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            if form.mode == StorageFormMode::Create
                && form.field == 1
                && form.kind != StorageKind::Traces =>
        {
            form.is_system = !form.is_system
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            if form.mode == StorageFormMode::Create
                && form.field == 2
                && form.kind != StorageKind::Metrics =>
        {
            form.classification = (form.classification + 1) % StorageForm::CLASSIFICATIONS.len()
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_secret_form(form: &mut SecretForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down | KeyCode::BackTab | KeyCode::Up
            if form.mode == SecretFormMode::Update =>
        {
            form.field = 1
        }
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % SecretForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + SecretForm::FIELDS - 1) % SecretForm::FIELDS
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Some(value) = form.value_mut(form.field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_profile_form(form: &mut ProfileForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % ProfileForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + ProfileForm::FIELDS - 1) % ProfileForm::FIELDS
        }
        KeyCode::Left if form.field == ProfileForm::ZONE_FIELD => form.cycle_zone(-1),
        KeyCode::Right | KeyCode::Char(' ') if form.field == ProfileForm::ZONE_FIELD => {
            form.cycle_zone(1)
        }
        KeyCode::Left if form.field == ProfileForm::ROOT_FIELD => form.cycle_api_root(-1),
        KeyCode::Right | KeyCode::Char(' ') if form.field == ProfileForm::ROOT_FIELD => {
            form.cycle_api_root(1)
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            if form.field == ProfileForm::STORAGE_FIELD =>
        {
            form.storage = form.storage.toggled()
        }
        KeyCode::Backspace => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.pop();
            }
        }
        // 選択欄では文字入力を受け付けない。
        KeyCode::Char(c) if ProfileForm::is_text(form.field) => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

fn edit_login_form(form: &mut LoginForm, key: KeyEvent) {
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % LoginForm::FIELDS,
        KeyCode::BackTab | KeyCode::Up => {
            form.field = (form.field + LoginForm::FIELDS - 1) % LoginForm::FIELDS
        }
        KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if form.field == 2 => {
            form.save = !form.save
        }
        KeyCode::Backspace => match form.field {
            0 => {
                form.username.pop();
            }
            1 => {
                form.password.pop();
            }
            _ => {}
        },
        KeyCode::Char(c) => match form.field {
            0 => form.username.push(c),
            1 => form.password.push(c),
            _ => {}
        },
        _ => {}
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
        let switch = Service::ALL
            .iter()
            .position(|service| *service == Service::Switch)
            .unwrap();
        let internet = Service::ALL
            .iter()
            .position(|service| *service == Service::Internet)
            .unwrap();
        assert_eq!(move_service_within_category(switch, 1), internet);
        assert_eq!(
            Service::ALL[move_service_within_category(switch, -1)],
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
