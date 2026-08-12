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

use crate::config::{Config, RegistryLogin};
use crate::registry::{RegistryClients, TagInfo};
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
}

/// 遅延ロードするデータの状態。
#[derive(Debug, Clone)]
pub enum Loadable<T> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Registries,
    Detail,
}

/// イメージタブ内で選択中のペイン。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePane {
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
        action: ConfirmAction,
    },
    UserForm(UserForm),
    Login(LoginForm),
}

pub struct App {
    sacloud: Arc<SacloudClient>,
    tx: UnboundedSender<Message>,
    pub config: Config,
    pub registry_clients: RegistryClients,
    pub credential_source: String,

    pub should_quit: bool,
    /// 実行中の非同期リクエスト数（スピナー表示用）。
    pub inflight: usize,
    pub tick: u64,

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

    pub overlay: Option<Overlay>,
    pub status: Option<(String, StatusKind)>,
}

impl App {
    pub fn new(
        sacloud: Arc<SacloudClient>,
        tx: UnboundedSender<Message>,
        config: Config,
        credential_source: String,
    ) -> Self {
        let mut app = Self {
            sacloud,
            tx,
            config,
            registry_clients: RegistryClients::default(),
            credential_source,
            should_quit: false,
            inflight: 0,
            tick: 0,
            registries: Loadable::Idle,
            registry_state: TableState::default(),
            tab: Tab::Overview,
            focus: Focus::Registries,
            users: HashMap::new(),
            user_state: ListState::default(),
            image_pane: ImagePane::Repositories,
            repositories: HashMap::new(),
            repository_state: ListState::default(),
            tags: HashMap::new(),
            tag_state: ListState::default(),
            overlay: None,
            status: None,
        };
        app.load_registries();
        app
    }

    // --- 選択中の要素 ---

    pub fn selected_registry(&self) -> Option<&ContainerRegistry> {
        let items = self.registries.ready()?;
        items.get(self.registry_state.selected()?)
    }

    pub fn selected_user(&self) -> Option<&RegistryUser> {
        let registry = self.selected_registry()?;
        let users = self.users.get(&registry.id)?.ready()?;
        users.get(self.user_state.selected()?)
    }

    pub fn selected_repository(&self) -> Option<&str> {
        let host = self.selected_registry()?.host();
        let repos = self.repositories.get(host)?.ready()?;
        repos.get(self.repository_state.selected()?).map(|s| &**s)
    }

    /// 現在選択中のレジストリのユーザー一覧の状態。
    pub fn current_users(&self) -> Loadable<Vec<RegistryUser>> {
        self.selected_registry()
            .and_then(|r| self.users.get(&r.id))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn current_repositories(&self) -> Loadable<Vec<String>> {
        self.selected_registry()
            .and_then(|r| self.repositories.get(r.host()))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn current_tags(&self) -> Loadable<Vec<TagInfo>> {
        let Some(registry) = self.selected_registry() else {
            return Loadable::Idle;
        };
        let Some(repository) = self.selected_repository() else {
            return Loadable::Idle;
        };
        self.tags
            .get(&(registry.host().to_string(), repository.to_string()))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    /// 選択中レジストリにログイン済みかどうか。
    pub fn is_logged_in(&self) -> bool {
        self.selected_registry()
            .is_some_and(|r| self.registry_clients.get(r.host()).is_some())
    }

    // --- 非同期処理の起動 ---

    fn load_registries(&mut self) {
        self.registries = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_registries().await.map_err(fmt_error);
            let _ = tx.send(Message::Registries(result));
        });
    }

    fn load_users(&mut self, id: ResourceId) {
        self.users.insert(id, Loadable::Loading);
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
        self.repositories.insert(host.clone(), Loadable::Loading);
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
        self.tags
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

    /// 現在表示中のビューに必要なデータをまだ読んでいなければ読む。
    pub fn ensure_loaded(&mut self) {
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let id = registry.id;
        let host = registry.host().to_string();
        // 先に選択位置を整えてから、その選択に紐づくデータの要否を判断する。
        self.normalize_selection();

        match self.tab {
            Tab::Overview => {}
            Tab::Users => {
                if self.users.get(&id).is_none_or(Loadable::is_idle) {
                    self.load_users(id);
                }
            }
            // 未ログインならユーザーが L を押すまで何もしない。
            Tab::Images if self.registry_clients.get(&host).is_none() => {
                self.try_auto_login(&host);
            }
            Tab::Images => {
                if self.repositories.get(&host).is_none_or(Loadable::is_idle) {
                    self.load_repositories(host.clone());
                }
                if let Some(repository) = self.selected_repository().map(str::to_string) {
                    let key = (host.clone(), repository.clone());
                    if self.tags.get(&key).is_none_or(Loadable::is_idle) {
                        self.load_tags(host, repository);
                    }
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

        let users = self.current_users().ready().map_or(0, Vec::len);
        fill(&mut self.user_state, users);
        let repositories = self.current_repositories().ready().map_or(0, Vec::len);
        fill(&mut self.repository_state, repositories);
        let tags = self.current_tags().ready().map_or(0, Vec::len);
        fill(&mut self.tag_state, tags);
    }

    /// 設定ファイルにログイン情報があれば自動でクライアントを作る。
    fn try_auto_login(&mut self, host: &str) {
        let Some(login) = self.config.registries.get(host).cloned() else {
            return;
        };
        match self.registry_clients.insert(host, login) {
            Ok(_) => {
                self.repositories.insert(host.to_string(), Loadable::Idle);
                self.load_repositories(host.to_string());
            }
            Err(err) => {
                self.repositories
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
                self.registries = Loadable::Ready(items);
                self.registry_state.select(index);
                self.set_status(format!("コンテナレジストリ {count} 件"), StatusKind::Info);
                self.ensure_loaded();
            }
            Message::Registries(Err(err)) => {
                self.registries = Loadable::Failed(err.clone());
                self.set_status(err, StatusKind::Error);
            }
            Message::Users { id, result } => {
                match result {
                    Ok(users) => {
                        self.user_state
                            .select(if users.is_empty() { None } else { Some(0) });
                        self.users.insert(id, Loadable::Ready(users));
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.users.insert(id, Loadable::Failed(err));
                    }
                };
            }
            Message::Repositories { host, result } => {
                match result {
                    Ok(repos) => {
                        self.repository_state
                            .select(if repos.is_empty() { None } else { Some(0) });
                        self.repositories.insert(host, Loadable::Ready(repos));
                        self.ensure_loaded();
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.repositories.insert(host, Loadable::Failed(err));
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
                        self.tag_state
                            .select(if tags.is_empty() { None } else { Some(0) });
                        self.tags.insert(key, Loadable::Ready(tags));
                    }
                    Err(err) => {
                        self.set_status(err.clone(), StatusKind::Error);
                        self.tags.insert(key, Loadable::Failed(err));
                    }
                };
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
                    self.repositories.insert(host.clone(), Loadable::Idle);
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
        } else {
            self.on_key_main(key);
        }
        self.ensure_loaded();
    }

    fn on_key_main(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('?') => self.overlay = Some(Overlay::Help),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('R') => {
                self.invalidate_all();
                self.load_registries();
            }
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('1') => self.set_tab(Tab::Overview),
            KeyCode::Char('2') => self.set_tab(Tab::Users),
            KeyCode::Char('3') => self.set_tab(Tab::Images),
            KeyCode::Char('a') => self.open_add_user(),
            KeyCode::Char('e') => self.open_edit_user(),
            KeyCode::Char('d') => self.confirm_delete_user(),
            KeyCode::Char('L') => self.open_login(),
            KeyCode::Char('O') => self.confirm_forget_login(),
            KeyCode::Left | KeyCode::Char('h') => self.focus_left(),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => self.focus_right(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => self.jump_selection(true),
            KeyCode::End | KeyCode::Char('G') => self.jump_selection(false),
            _ => {}
        }
    }

    /// 現在のビューのキャッシュを捨てて読み直す。
    fn refresh(&mut self) {
        let Some(registry) = self.selected_registry() else {
            self.load_registries();
            return;
        };
        let id = registry.id;
        let host = registry.host().to_string();
        match self.tab {
            Tab::Overview => self.load_registries(),
            Tab::Users => self.load_users(id),
            Tab::Images => match self.image_pane {
                ImagePane::Repositories => {
                    self.tags.retain(|(h, _), _| h != &host);
                    self.load_repositories(host);
                }
                ImagePane::Tags => {
                    if let Some(repository) = self.selected_repository().map(str::to_string) {
                        self.load_tags(host, repository);
                    }
                }
            },
        }
    }

    fn invalidate_all(&mut self) {
        self.users.clear();
        self.repositories.clear();
        self.tags.clear();
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.focus = Focus::Detail;
    }

    fn cycle_tab(&mut self, delta: i32) {
        let current = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0) as i32;
        let len = Tab::ALL.len() as i32;
        self.tab = Tab::ALL[(current + delta).rem_euclid(len) as usize];
        self.focus = Focus::Detail;
    }

    fn focus_left(&mut self) {
        if self.focus == Focus::Detail
            && self.tab == Tab::Images
            && self.image_pane == ImagePane::Tags
        {
            self.image_pane = ImagePane::Repositories;
            return;
        }
        self.focus = Focus::Registries;
    }

    fn focus_right(&mut self) {
        if self.focus == Focus::Registries {
            self.focus = Focus::Detail;
            return;
        }
        if self.tab == Tab::Images && self.image_pane == ImagePane::Repositories {
            self.image_pane = ImagePane::Tags;
        }
    }

    /// 現在フォーカスしているリストの選択を動かす。
    fn move_selection(&mut self, delta: i32) {
        let (state, len) = match self.active_list() {
            Some(v) => v,
            None => return,
        };
        if len == 0 {
            return;
        }
        let current = state.selected().unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, len as i32 - 1) as usize;
        state.select(Some(next));
        self.after_selection_change();
    }

    fn jump_selection(&mut self, to_top: bool) {
        let (state, len) = match self.active_list() {
            Some(v) => v,
            None => return,
        };
        if len == 0 {
            return;
        }
        state.select(Some(if to_top { 0 } else { len - 1 }));
        self.after_selection_change();
    }

    /// レジストリやリポジトリの選択が変わったら、それにぶら下がる選択をリセットする。
    fn after_selection_change(&mut self) {
        match (self.focus, self.tab, self.image_pane) {
            (Focus::Registries, _, _) => {
                self.user_state.select(None);
                self.repository_state.select(None);
                self.tag_state.select(None);
                self.image_pane = ImagePane::Repositories;
            }
            (Focus::Detail, Tab::Images, ImagePane::Repositories) => {
                self.tag_state.select(None);
            }
            _ => {}
        }
    }

    /// 現在フォーカスしているリストの状態と要素数。
    fn active_list(&mut self) -> Option<(&mut dyn SelectableList, usize)> {
        match self.focus {
            Focus::Registries => {
                let len = self.registries.ready().map_or(0, Vec::len);
                Some((&mut self.registry_state, len))
            }
            Focus::Detail => match self.tab {
                Tab::Overview => None,
                Tab::Users => {
                    let len = self.current_users().ready().map_or(0, Vec::len);
                    Some((&mut self.user_state, len))
                }
                Tab::Images => match self.image_pane {
                    ImagePane::Repositories => {
                        let len = self.current_repositories().ready().map_or(0, Vec::len);
                        Some((&mut self.repository_state, len))
                    }
                    ImagePane::Tags => {
                        let len = self.current_tags().ready().map_or(0, Vec::len);
                        Some((&mut self.tag_state, len))
                    }
                },
            },
        }
    }

    // --- ユーザー管理 ---

    fn open_add_user(&mut self) {
        let Some((id, name)) = self
            .selected_registry()
            .map(|registry| (registry.id, registry.name.clone()))
        else {
            return;
        };
        self.tab = Tab::Users;
        self.focus = Focus::Detail;
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
        if self.tab != Tab::Users {
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
        if self.tab != Tab::Users {
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
            ConfirmAction::ForgetLogin { host } => {
                self.registry_clients.remove(&host);
                self.repositories.remove(&host);
                self.tags.retain(|(h, _), _| h != &host);
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
        self.tab = Tab::Images;
        self.focus = Focus::Detail;
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
                action,
            } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.run_confirmed(action)
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => {}
                _ => {
                    self.overlay = Some(Overlay::Confirm {
                        title,
                        body,
                        action,
                    })
                }
            },
            Overlay::UserForm(mut form) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit_user_form(form),
                _ => {
                    edit_user_form(&mut form, key);
                    self.overlay = Some(Overlay::UserForm(form));
                }
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

/// `anyhow::Error` を原因も含めた 1 つの文字列にする。
fn fmt_error(err: anyhow::Error) -> String {
    let mut parts = vec![err.to_string()];
    parts.extend(err.chain().skip(1).map(|c| c.to_string()));
    parts.join(": ")
}
