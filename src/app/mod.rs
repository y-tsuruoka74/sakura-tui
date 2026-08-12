//! アプリケーションの状態遷移。
//!
//! 描画は `ui` モジュールが担当し、ここでは状態・キー入力・非同期処理の結果反映を扱う。
//! API 呼び出しは全て `tokio::spawn` して `Message` として結果を受け取るため、
//! 通信中も UI がブロックしない。

use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::{ListState, TableState};
use tokio::sync::mpsc::UnboundedSender;

mod apprun;
mod dedicated;
mod server;

pub use apprun::{AppRunPane, AppRunView};
pub use dedicated::{DedicatedFocus, DedicatedTab, DedicatedView};
pub use server::ServerView;

use crate::apprun::{Application, ApplicationDetail, AppRunClient, Traffic, Version};
use crate::apprun_dedicated::{
    self as ded, Cluster, DedicatedClient,
};
use crate::config::{Config, CredentialSource, RegistryLogin};
use crate::iaas::{PowerAction, Server, Zone};
use crate::registry::{RegistryClients, TagDetail, TagInfo};
use crate::sacloud::{ContainerRegistry, Permission, RegistryUser, ResourceId, SacloudClient};

/// 非同期処理の結果。
#[derive(Debug)]
pub enum Message {
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
    Zones(Result<Vec<Zone>, String>),
    Servers {
        zone: String,
        result: Result<Vec<Server>, String>,
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

/// TUI が扱うサービス。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Service {
    #[default]
    Registry,
    AppRun,
    Dedicated,
    Server,
}

impl Service {
    pub const ALL: [Service; 4] = [
        Service::Registry,
        Service::AppRun,
        Service::Dedicated,
        Service::Server,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Service::Registry => "コンテナレジストリ",
            Service::AppRun => "AppRun",
            Service::Dedicated => "AppRun専有型",
            Service::Server => "サーバー",
        }
    }

    /// ゾーンを選ぶ意味があるサービスか。
    pub fn is_zoned(self) -> bool {
        matches!(self, Service::Server)
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
    DeleteRegistry {
        id: ResourceId,
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
    PowerAction {
        id: ResourceId,
        zone: String,
        name: String,
        action: PowerAction,
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
    Login(LoginForm),
    /// 認証情報（usacloud プロファイル / 環境変数）の切り替え。
    ProfilePicker {
        sources: Vec<CredentialSource>,
        index: usize,
    },
    /// ゾーンの切り替え。
    ZonePicker {
        zones: Vec<Zone>,
        index: usize,
    },
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
}

pub struct App {
    sacloud: Arc<SacloudClient>,
    apprun_client: Arc<AppRunClient>,
    dedicated_client: Arc<DedicatedClient>,
    tx: UnboundedSender<Message>,
    pub config: Config,
    pub registry_clients: RegistryClients,
    pub credential_source: CredentialSource,

    pub mode: Mode,
    pub should_quit: bool,
    /// 実行中の非同期リクエスト数（スピナー表示用）。
    pub inflight: usize,
    pub tick: u64,

    /// 表示中のサービス。
    pub service: Service,
    /// ゾーンに属するリソース（サーバーなど）を見るときのゾーン。
    pub zone: String,
    pub zones: Loadable<Vec<Zone>>,

    /// コンテナレジストリ画面の状態。
    pub registry: RegistryView,
    /// AppRun（共用型）画面の状態。
    pub apprun: AppRunView,
    /// AppRun（専有型）画面の状態。
    pub dedicated: DedicatedView,
    /// サーバー画面の状態。
    pub server: ServerView,

    /// ペインごとの絞り込み。
    pub filters: Filters,
    /// 絞り込み文字列を編集中かどうか。
    pub filtering: bool,

    pub overlay: Option<Overlay>,
    pub status: Option<(String, StatusKind)>,
}

impl App {
    pub fn new(
        sacloud: Arc<SacloudClient>,
        apprun_client: Arc<AppRunClient>,
        dedicated_client: Arc<DedicatedClient>,
        tx: UnboundedSender<Message>,
        config: Config,
        credential_source: CredentialSource,
    ) -> Self {
        let default_zone = sacloud.default_zone().to_string();
        let mut app = Self {
            sacloud,
            apprun_client,
            dedicated_client,
            tx,
            config,
            registry_clients: RegistryClients::default(),
            credential_source,
            // 事故を防ぐため、既定は読み取り専用。
            mode: Mode::ReadOnly,
            should_quit: false,
            inflight: 0,
            tick: 0,
            service: Service::Registry,
            zone: default_zone,
            zones: Loadable::Idle,
            registry: RegistryView::default(),
            apprun: AppRunView::default(),
            dedicated: DedicatedView::default(),
            server: ServerView::default(),
            filters: Filters::default(),
            filtering: false,
            overlay: None,
            status: None,
        };
        app.load_registries();
        app
    }

    // --- 表示中の要素（絞り込み適用後） ---
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
                .filter(|u| matches(self.filters.get(Pane::Users), &[&u.username, u.permission.as_str()]))
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
        self.registry.repositories.insert(host.clone(), Loadable::Loading);
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
        self.registry.tags
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
        self.registry.tag_details.insert(key.clone(), Loadable::Loading);
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .tag_detail(&key.1, &key.2)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::TagDetails { key, result });
        });
    }

    /// 現在表示中のビューに必要なデータをまだ読んでいなければ読む。
    pub fn ensure_loaded(&mut self) {
        match self.service {
            Service::Registry => self.registry_ensure_loaded(),
            Service::AppRun => self.apprun_ensure_loaded(),
            Service::Dedicated => self.dedicated_ensure_loaded(),
            Service::Server => self.server_ensure_loaded(),
        }
    }

    fn registry_ensure_loaded(&mut self) {
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
                if self.registry.repositories.get(&host).is_none_or(Loadable::is_idle) {
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
                    && self.registry.tag_details.get(&key).is_none_or(Loadable::is_idle)
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

    /// 設定ファイルにログイン情報があれば自動でクライアントを作る。
    fn try_auto_login(&mut self, host: &str) {
        let Some(login) = self.config.registries.get(host).cloned() else {
            return;
        };
        match self.registry_clients.insert(host, login) {
            Ok(_) => {
                self.registry.repositories.insert(host.to_string(), Loadable::Idle);
                self.load_repositories(host.to_string());
            }
            Err(err) => {
                self.registry.repositories
                    .insert(host.to_string(), Loadable::Failed(fmt_error(err)));
            }
        }
    }

    // --- 非同期処理の結果反映 ---

    pub fn on_message(&mut self, message: Message) {
        self.inflight = self.inflight.saturating_sub(1);
        match message {
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
                        self.registry.user_state
                            .select(if users.is_empty() { None } else { Some(0) });
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
                        self.registry.repository_state
                            .select(if repos.is_empty() { None } else { Some(0) });
                        self.registry.repositories.insert(host, Loadable::Ready(repos));
                        self.ensure_loaded();
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.registry.repositories.insert(host, Loadable::Failed(err));
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
                        self.registry.tag_state
                            .select(if tags.is_empty() { None } else { Some(0) });
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
                        self.config.registries.insert(host.clone(), login);
                        match self.config.save() {
                            Ok(path) => self.set_status(
                                format!("{host} にログインしました（{} に保存）", path.display()),
                                StatusKind::Success,
                            ),
                            Err(err) => self.set_status(
                                format!("ログインしましたが設定の保存に失敗: {}", fmt_error(err)),
                                StatusKind::Error,
                            ),
                        }
                    } else {
                        self.set_status(format!("{host} にログインしました"), StatusKind::Success);
                    }
                    self.registry.repositories.insert(host.clone(), Loadable::Idle);
                    self.load_repositories(host);
                }
                Err(err) => {
                    self.registry_clients.remove(&host);
                    self.overlay = Some(Overlay::Message {
                        title: "ログイン失敗".to_string(),
                        body: err.clone(),
                        kind: StatusKind::Error,
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
            Message::Zones(Ok(zones)) => self.zones = Loadable::Ready(zones),
            Message::Zones(Err(err)) => {
                self.zones = Loadable::Failed(err.clone());
                self.set_status(err, StatusKind::Error);
            }
            Message::Servers { zone, result } => {
                match result {
                    Ok(servers) => {
                        let count = servers.len();
                        self.server.server_state.select(None);
                        self.server.servers.insert(zone.clone(), Loadable::Ready(servers));
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
            Message::ServerAction { zone, label, result } => match result {
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
                    self.registry.tag_details
                        .retain(|(h, r, _), _| h != &host || r != &repository);
                    self.registry.tag_state.select(None);
                    self.load_tags(host, repository);
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
            Message::UserAction { id, label, result } => match result {
                Ok(()) => {
                    self.set_status(format!("{label}しました"), StatusKind::Success);
                    self.load_users(id);
                }
                Err(err) => {
                    self.overlay = Some(Overlay::Message {
                        title: format!("{label}に失敗しました"),
                        body: err.clone(),
                        kind: StatusKind::Error,
                    });
                    self.set_status(err, StatusKind::Error);
                }
            },
        }
    }

    /// 取得結果を `Loadable` に変換しつつ、失敗ならステータス行にも出す。
    fn store_result<T>(&mut self, result: Result<Vec<T>, String>) -> Loadable<Vec<T>> {
        match result {
            Ok(items) => Loadable::Ready(items),
            Err(err) => {
                self.set_status(err.clone(), StatusKind::Error);
                Loadable::Failed(err)
            }
        }
    }

    pub fn set_status(&mut self, text: impl Into<String>, kind: StatusKind) {
        self.status = Some((text.into(), kind));
    }

    // --- キー入力 ---

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
        }
    }

    /// どのサービスでも同じ意味を持つキー。処理したら true。
    fn on_key_common(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.overlay = Some(Overlay::Help),
            KeyCode::Char('s') => self.cycle_service(1),
            KeyCode::Char('S') => self.cycle_service(-1),
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

    // --- サービスの切り替え ---

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
            }
            Loadable::Loading => self.set_status("ゾーン一覧を取得中です…", StatusKind::Info),
            _ => {
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

    fn switch_zone(&mut self, zone: String) {
        if zone == self.zone {
            return;
        }
        self.zone = zone;
        self.server.server_state.select(None);
        self.set_status(format!("ゾーンを {} に切り替えました", self.zone), StatusKind::Info);
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
        // 絞り込みでレジストリやリポジトリが変わると下位の選択も無効になる。
        match pane {
            Pane::Registries => {
                self.registry.user_state.select(None);
                self.registry.repository_state.select(None);
                self.registry.tag_state.select(None);
            }
            Pane::Repositories => self.registry.tag_state.select(None),
            _ => {}
        }
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
            Pane::Clusters => self.visible_clusters().len(),
            Pane::DedicatedApplications => {
                self.visible_dedicated_applications().ready().map_or(0, Vec::len)
            }
            Pane::ScalingGroups => self.visible_scaling_groups().ready().map_or(0, Vec::len),
            Pane::Certificates => self.visible_certificates().ready().map_or(0, Vec::len),
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
            Pane::Clusters => Some(&mut self.dedicated.cluster_state),
            Pane::DedicatedApplications => Some(&mut self.dedicated.application_state),
            Pane::ScalingGroups => Some(&mut self.dedicated.scaling_group_state),
            Pane::Certificates => Some(&mut self.dedicated.certificate_state),
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
            Pane::Clusters => self.selected_cluster().map(|c| c.id.clone()),
            Pane::DedicatedApplications => self
                .visible_dedicated_applications()
                .ready()
                .and_then(|apps| {
                    apps.get(self.dedicated.application_state.selected()?)
                        .map(|app| app.name.clone())
                }),
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
        }
    }

    // --- 認証情報の切り替え ---

    fn open_profile_picker(&mut self) {
        let sources = crate::config::available_credential_sources();
        if sources.len() < 2 {
            self.set_status(
                "切り替え先の認証情報がありません（usacloud のプロファイルは1つだけです）",
                StatusKind::Info,
            );
            return;
        }
        let index = sources
            .iter()
            .position(|s| *s == self.credential_source)
            .unwrap_or(0);
        self.overlay = Some(Overlay::ProfilePicker { sources, index });
    }

    /// 認証情報を切り替え、クラウド API 側のキャッシュを捨てて読み直す。
    ///
    /// レジストリへのログインはホスト単位でクラウドの契約とは独立なので保持する。
    fn switch_credentials(&mut self, source: CredentialSource) {
        if source == self.credential_source {
            return;
        }
        let credentials = match crate::config::load_credentials_from(&source) {
            Ok(credentials) => credentials,
            Err(err) => {
                self.set_status(
                    format!("{} に切り替えられません: {}", source.label(), fmt_error(err)),
                    StatusKind::Error,
                );
                return;
            }
        };
        let client = match SacloudClient::new(&credentials) {
            Ok(client) => Arc::new(client),
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                return;
            }
        };

        self.sacloud = client;
        self.credential_source = source;
        // ユーザーはレジストリIDに紐づくので、契約が変われば無効。
        self.registry.users.clear();
        self.registry.registry_state.select(None);
        self.registry.user_state.select(None);
        self.registry.repository_state.select(None);
        self.registry.tag_state.select(None);
        self.filters = Filters::default();
        self.set_status(
            format!("{} に切り替えました", self.credential_source.label()),
            StatusKind::Info,
        );
        self.load_registries();
    }

    /// 現在のビューのキャッシュを捨てて読み直す。
    fn refresh(&mut self) {
        match self.service {
            Service::Registry => self.registry_refresh(),
            Service::AppRun => self.apprun_refresh(),
            Service::Dedicated => self.dedicated_refresh(),
            Service::Server => self.server_refresh(),
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

    fn invalidate_all(&mut self) {
        self.registry.users.clear();
        self.registry.repositories.clear();
        self.registry.tags.clear();
        self.registry.tag_details.clear();
        self.apprun_invalidate();
        self.dedicated_invalidate();
        self.server_invalidate();
    }

    fn set_tab(&mut self, tab: Tab) {
        self.registry.tab = tab;
        self.registry.focus = Focus::Detail;
    }

    fn cycle_tab(&mut self, delta: i32) {
        let current = Tab::ALL.iter().position(|t| *t == self.registry.tab).unwrap_or(0) as i32;
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
        if self.service != Service::Registry {
            return;
        }
        match (self.registry.focus, self.registry.tab, self.registry.image_pane) {
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
        let Some((id, name)) = self
            .selected_registry()
            .map(|registry| (registry.id, registry.name.clone()))
        else {
            return;
        };
        self.registry.tab = Tab::Users;
        self.registry.focus = Focus::Detail;
        self.overlay = Some(Overlay::UserForm(UserForm {
            registry: id,
            registry_name: name,
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
        let (id, name) = (registry.id, registry.name.clone());
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
                let (username, password) = (form.username, form.password);
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client
                        .add_user(id, &username, &password, permission)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::UserAction { id, label, result });
                });
            }
            UserFormMode::Edit => {
                let label = format!("ユーザー「{}」を更新", form.username);
                let username = form.username;
                // パスワードが空欄なら現在のパスワードを維持する。
                let password = (!form.password.is_empty()).then_some(form.password);
                self.inflight += 1;
                tokio::spawn(async move {
                    let result = client
                        .update_user(id, &username, password.as_deref(), permission)
                        .await
                        .map_err(fmt_error);
                    let _ = tx.send(Message::UserAction { id, label, result });
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
            ConfirmAction::PowerAction {
                id,
                zone,
                name,
                action,
            } => self.run_power_action(id, zone, name, action),
            ConfirmAction::ForgetLogin { host } => {
                self.registry_clients.remove(&host);
                self.registry.repositories.remove(&host);
                self.registry.tags.retain(|(h, _), _| h != &host);
                let removed = self.config.registries.remove(&host).is_some();
                if removed {
                    match self.config.save() {
                        Ok(_) => self.set_status(
                            format!("{host} のログイン情報を削除しました"),
                            StatusKind::Success,
                        ),
                        Err(err) => self.set_status(
                            format!("設定の保存に失敗: {}", fmt_error(err)),
                            StatusKind::Error,
                        ),
                    }
                } else {
                    self.set_status(format!("{host} からログアウトしました"), StatusKind::Success);
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
        let saved = self.config.registries.get(&host).cloned();
        self.registry.tab = Tab::Images;
        self.registry.focus = Focus::Detail;
        self.overlay = Some(Overlay::Login(LoginForm {
            username: saved.as_ref().map(|l| l.username.clone()).unwrap_or_default(),
            password: String::new(),
            save: saved.is_some(),
            host,
            field: 0,
        }));
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
            self.set_status("ユーザー名とパスワードを入力してください", StatusKind::Error);
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
            // 何かキーを押したら閉じる（`take()` 済みなので何もしなくてよい）。
            Overlay::Help | Overlay::Message { .. } => {}
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
            Overlay::ProfilePicker { sources, mut index } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Enter => self.switch_credentials(sources[index].clone()),
                KeyCode::Down | KeyCode::Char('j') => {
                    index = (index + 1) % sources.len();
                    self.overlay = Some(Overlay::ProfilePicker { sources, index });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    index = (index + sources.len() - 1) % sources.len();
                    self.overlay = Some(Overlay::ProfilePicker { sources, index });
                }
                _ => self.overlay = Some(Overlay::ProfilePicker { sources, index }),
            },
            Overlay::RegistryForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_registry_form(form),
                _ => {
                    edit_registry_form(&mut form, key);
                    self.overlay = Some(Overlay::RegistryForm(form));
                }
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
}
