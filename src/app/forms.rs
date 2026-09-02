//! 入力フォームの定義と編集操作。
//!
//! どのフォームも「ラベルの配列」と「添字で値を返す/借りる」だけの素朴な作りで、
//! 描画は `src/ui/overlay.rs`、送信は `App` 側が持つ。ここには状態を持ち込まない。

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{Loadable, matches};
use crate::commonservice::{DnsRecord, DnsZone, SimpleMonitor};
use crate::config::IamCredentials;
use crate::iaas::{DiskPlan, Nic, ServerPlan, StartupScript, Zone};
use crate::monitoring::{
    AlertProject, AlertRule, AlertRuleInput, DashboardProject, LogMeasureRule, LogMeasureRuleInput,
    LogRouting, LogRoutingInput, MetricsRouting, MetricsRoutingInput, NotificationRouting,
    NotificationTarget, Publisher, Storage, StorageAccessKey, StorageKind,
};
use crate::packet_filter::PacketFilterRule;
use crate::pubkey::PublicKey;
use crate::sacloud::{ContainerRegistry, Permission, ResourceId};
use crate::secretmanager::Vault;
use crate::switch::Switch;

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

    pub(super) fn credentials(&self) -> IamCredentials {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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
    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.description),
            _ => None,
        }
    }
}

/// RAGドキュメントのアップロードフォーム。
///
/// 名前・モデル・分割サイズは空ならサービス側の既定に任せるので、
/// 必須はファイルのパスだけにしてある。
#[derive(Debug, Clone, Default)]
pub struct RagUploadForm {
    pub path: String,
    pub name: String,
    pub tags: String,
    pub model: String,
    pub chunk_size: String,
    pub field: usize,
}

impl RagUploadForm {
    pub const LABELS: [&'static str; 5] = [
        "ファイルのパス",
        "名前（任意）",
        "タグ（カンマ区切り・任意）",
        "モデル（任意）",
        "分割サイズ（任意）",
    ];

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.path,
            1 => &self.name,
            2 => &self.tags,
            3 => &self.model,
            4 => &self.chunk_size,
            _ => "",
        }
    }

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.path),
            1 => Some(&mut self.name),
            2 => Some(&mut self.tags),
            3 => Some(&mut self.model),
            4 => Some(&mut self.chunk_size),
            _ => None,
        }
    }

    pub(super) fn tag_list(&self) -> Vec<String> {
        split_tags(&self.tags)
    }
}

/// 作成フォームの既定のディスクサイズ（20GB）。
const DEFAULT_DISK_SIZE_MB: u32 = 20480;

/// サーバー作成フォームの入力欄。
///
/// NIC の繋ぎ先によって出る欄が変わるので、添字ではなくこの並びで扱う。
/// 添字で分岐していると、欄を1つ足しただけで別の欄の処理がずれる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerField {
    Name,
    Description,
    Cpu,
    Memory,
    Os,
    DiskSize,
    Nic,
    IpAddress,
    MaskLen,
    Gateway,
    PacketFilter,
    StartupScript,
    HostName,
    Password,
    SshKey,
    Boot,
}

impl ServerField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "名前",
            Self::Description => "説明",
            Self::Cpu => "CPU",
            Self::Memory => "メモリ",
            Self::Os => "OS",
            Self::DiskSize => "ディスクサイズ",
            Self::Nic => "NIC",
            Self::IpAddress => "IPアドレス",
            Self::MaskLen => "マスク長",
            Self::Gateway => "ゲートウェイ",
            Self::PacketFilter => "パケットフィルタ",
            Self::StartupScript => "スタートアップ",
            Self::HostName => "ホスト名",
            Self::Password => "パスワード",
            Self::SshKey => "SSH公開鍵",
            Self::Boot => "作成後に起動",
        }
    }

    /// 左右キーで選ぶ欄か。文字が入らない欄でもある。
    pub fn is_choice(self) -> bool {
        matches!(
            self,
            Self::Cpu
                | Self::Memory
                | Self::Os
                | Self::DiskSize
                | Self::Nic
                | Self::PacketFilter
                | Self::StartupScript
                | Self::Boot
        )
    }
}

/// eth0 の繋ぎ先。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NicChoice {
    Shared,
    Switch(ResourceId, String),
    None,
}

impl NicChoice {
    pub fn label(&self) -> String {
        match self {
            Self::Shared => "共有セグメント".to_string(),
            Self::Switch(_, name) => format!("スイッチ: {name}"),
            Self::None => "接続しない".to_string(),
        }
    }
}

/// 作成フォームで選べるもの。ゾーンごとに違うのでまとめて受け取る。
#[derive(Debug, Clone, Default)]
pub struct ServerChoices {
    pub plans: Vec<ServerPlan>,
    pub disk_sizes: Vec<u32>,
    /// 先頭が共有セグメント、末尾が「接続しない」。
    pub nics: Vec<NicChoice>,
    /// 先頭は「なし」を表す None。
    pub packet_filters: Vec<Option<(ResourceId, String)>>,
    pub startup_scripts: Vec<Option<StartupScript>>,
}

impl ServerChoices {
    pub fn nic(&self, index: usize) -> NicChoice {
        self.nics.get(index).cloned().unwrap_or(NicChoice::Shared)
    }

    pub fn packet_filter(&self, index: usize) -> Option<(ResourceId, String)> {
        self.packet_filters.get(index).cloned().flatten()
    }

    pub fn startup_script(&self, index: usize) -> Option<StartupScript> {
        self.startup_scripts.get(index).cloned().flatten()
    }
}

/// サーバー作成フォーム。
///
/// 選べる値はゾーンごとにAPIから引くので、フォームは選んだ値そのものを持ち、
/// 一覧の添字は持たない（一覧が入れ替わっても指すものがずれない）。
/// ただし OS・NIC・フィルタ・スクリプトは一覧の並びが安定しているので添字で持つ。
#[derive(Debug, Clone, Default)]
pub struct ServerCreateForm {
    pub name: String,
    pub description: String,
    pub host_name: String,
    pub password: String,
    pub ssh_public_key: String,
    pub ip_address: String,
    pub mask_len: String,
    pub gateway: String,
    /// コア数。プラン一覧が届くまでは 0。
    pub cpu: u32,
    pub memory_mb: u32,
    pub os: usize,
    pub disk_size_mb: u32,
    pub nic: usize,
    pub packet_filter: usize,
    pub startup_script: usize,
    pub boot_after_create: bool,
    pub field: usize,
}

impl ServerCreateForm {
    /// 今出ている欄。NIC をスイッチに繋ぐときだけ IP の欄が増える。
    pub fn fields(&self, choices: &ServerChoices) -> Vec<ServerField> {
        let mut fields = vec![
            ServerField::Name,
            ServerField::Description,
            ServerField::Cpu,
            ServerField::Memory,
            ServerField::Os,
            ServerField::DiskSize,
            ServerField::Nic,
        ];
        if matches!(choices.nic(self.nic), NicChoice::Switch(..)) {
            // スイッチには DHCP が無いので、IP は自分で決める。
            fields.extend([
                ServerField::IpAddress,
                ServerField::MaskLen,
                ServerField::Gateway,
            ]);
        }
        fields.extend([
            ServerField::PacketFilter,
            ServerField::StartupScript,
            ServerField::HostName,
            ServerField::Password,
            ServerField::SshKey,
            ServerField::Boot,
        ]);
        fields
    }

    /// 今えらんでいる欄。範囲外なら先頭に戻す。
    pub fn current(&self, choices: &ServerChoices) -> ServerField {
        let fields = self.fields(choices);
        fields.get(self.field).copied().unwrap_or(ServerField::Name)
    }

    pub fn value(&self, field: ServerField) -> &str {
        match field {
            ServerField::Name => &self.name,
            ServerField::Description => &self.description,
            ServerField::IpAddress => &self.ip_address,
            ServerField::MaskLen => &self.mask_len,
            ServerField::Gateway => &self.gateway,
            ServerField::HostName => &self.host_name,
            ServerField::Password => &self.password,
            ServerField::SshKey => &self.ssh_public_key,
            _ => "",
        }
    }

    fn value_mut(&mut self, field: ServerField) -> Option<&mut String> {
        match field {
            ServerField::Name => Some(&mut self.name),
            ServerField::Description => Some(&mut self.description),
            ServerField::IpAddress => Some(&mut self.ip_address),
            ServerField::MaskLen => Some(&mut self.mask_len),
            ServerField::Gateway => Some(&mut self.gateway),
            ServerField::HostName => Some(&mut self.host_name),
            ServerField::Password => Some(&mut self.password),
            ServerField::SshKey => Some(&mut self.ssh_public_key),
            _ => None,
        }
    }

    /// ホスト名は省略時にサーバー名を使う。
    pub fn effective_host_name(&self) -> &str {
        if self.host_name.is_empty() {
            &self.name
        } else {
            &self.host_name
        }
    }

    /// プラン一覧とディスクサイズが届いた時点で、まだ選んでいない欄を埋める。
    ///
    /// 一覧は非同期に届くので、フォームを開いた時点では空のことがある。
    pub fn apply_defaults(&mut self, plans: &[ServerPlan], disk_sizes: &[u32]) {
        if self.cpu == 0
            && let Some(plan) = plans.first()
        {
            self.cpu = plan.cpu;
            self.memory_mb = plan.memory_mb;
        }
        if self.disk_size_mb == 0 {
            // 既定は 20GB。無ければ一番小さいもの。
            self.disk_size_mb = disk_sizes
                .iter()
                .copied()
                .find(|mb| *mb == DEFAULT_DISK_SIZE_MB)
                .or_else(|| disk_sizes.first().copied())
                .unwrap_or(0);
        }
    }
}

/// 一覧から選ぶ画面に出す1件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRow {
    /// 元の一覧での位置。フォームに書き戻すのに使う。
    pub position: usize,
    pub label: String,
    /// 種類や所有者など、名前の右に出す短い情報。
    pub detail: String,
    /// 何をするものかの説明。選んでいる行にだけ出す。
    pub note: String,
}

impl ChoiceRow {
    /// 絞り込みで見る文字列。
    pub fn haystack(&self) -> [&str; 3] {
        [&self.label, &self.detail, &self.note]
    }
}

/// 候補が多い欄を、絞り込みながら選ぶ画面。
///
/// 左右キーで1件ずつ送るのは、スタートアップスクリプトのように数十件ある欄では
/// 現実的でないため、名前で絞れるようにする。
#[derive(Debug, Clone)]
pub struct ServerChoicePicker {
    /// 選び終えたら戻すフォーム。
    pub form: Box<ServerCreateForm>,
    /// どの欄を選んでいるか。
    pub target: ServerField,
    pub filter: String,
    /// 絞り込んだあとの一覧での位置。
    pub index: usize,
}

impl ServerChoicePicker {
    pub fn title(&self) -> String {
        format!("{}を選ぶ", self.target.label())
    }

    /// 絞り込んだ候補。
    pub fn visible(&self, choices: &ServerChoices) -> Vec<ChoiceRow> {
        choices
            .rows(self.target)
            .into_iter()
            .filter(|row| matches(&self.filter, &row.haystack()))
            .collect()
    }

    pub fn move_selection(&mut self, forward: bool, len: usize) {
        self.index = cycle(self.index, len, forward);
    }

    /// 絞り込みを変えたら、選択位置を先頭に戻す。
    /// そのままだと、絞った結果の見えていない行を選んだままになる。
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.index = 0;
    }
}

impl ServerChoices {
    /// その欄で選べるものを、一覧に出す形で返す。
    pub fn rows(&self, target: ServerField) -> Vec<ChoiceRow> {
        match target {
            ServerField::Nic => self
                .nics
                .iter()
                .enumerate()
                .map(|(position, nic)| ChoiceRow {
                    position,
                    label: nic.label(),
                    detail: match nic {
                        NicChoice::Switch(id, _) => id.to_string(),
                        _ => String::new(),
                    },
                    note: String::new(),
                })
                .collect(),
            ServerField::PacketFilter => self
                .packet_filters
                .iter()
                .enumerate()
                .map(|(position, filter)| match filter {
                    None => ChoiceRow {
                        position,
                        label: "なし".to_string(),
                        detail: String::new(),
                        note: String::new(),
                    },
                    Some((id, name)) => ChoiceRow {
                        position,
                        label: name.clone(),
                        detail: id.to_string(),
                        note: String::new(),
                    },
                })
                .collect(),
            ServerField::StartupScript => self
                .startup_scripts
                .iter()
                .enumerate()
                .map(|(position, script)| match script {
                    None => ChoiceRow {
                        position,
                        label: "なし".to_string(),
                        detail: String::new(),
                        note: String::new(),
                    },
                    Some(script) => ChoiceRow {
                        position,
                        label: script.name.clone(),
                        detail: format!(
                            "{} · {}",
                            if script.is_own() { "自分" } else { "共有" },
                            script.class
                        ),
                        // 説明とタグを検索の手がかりにする。
                        note: [script.description.clone(), script.tags.join(" ")]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("  "),
                    },
                })
                .collect(),
            // 候補が短い欄は左右キーで足りるので一覧を出さない。
            _ => Vec::new(),
        }
    }

    /// その欄が一覧から選ぶ形式か。
    pub fn is_list_field(target: ServerField) -> bool {
        matches!(
            target,
            ServerField::Nic | ServerField::PacketFilter | ServerField::StartupScript
        )
    }
}

impl ServerCreateForm {
    /// 一覧で選んだものを欄に書き戻す。
    pub fn take_choice(&mut self, target: ServerField, position: usize) {
        match target {
            ServerField::Nic => self.nic = position,
            ServerField::PacketFilter => self.packet_filter = position,
            ServerField::StartupScript => self.startup_script = position,
            _ => {}
        }
    }

    /// その欄で今選んでいる位置。
    pub fn choice_position(&self, target: ServerField) -> usize {
        match target {
            ServerField::Nic => self.nic,
            ServerField::PacketFilter => self.packet_filter,
            ServerField::StartupScript => self.startup_script,
            _ => 0,
        }
    }
}

pub(super) fn edit_server_choice_picker(
    picker: &mut ServerChoicePicker,
    key: KeyEvent,
    visible: usize,
) {
    match key.code {
        KeyCode::Down => picker.move_selection(true, visible),
        KeyCode::Up => picker.move_selection(false, visible),
        KeyCode::Backspace => {
            let mut filter = picker.filter.clone();
            filter.pop();
            picker.set_filter(filter);
        }
        KeyCode::Char(c) => {
            let filter = format!("{}{c}", picker.filter);
            picker.set_filter(filter);
        }
        _ => {}
    }
}

/// NIC の繋ぎ先かパケットフィルタを、絞り込みながら選ぶ画面。
///
/// 候補は作成フォームと同じ一覧を使う。
#[derive(Debug, Clone)]
pub struct NicPicker {
    pub target: NicTarget,
    pub server_name: String,
    pub nic: Nic,
    pub filter: String,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicTarget {
    Connection,
    PacketFilter,
}

impl NicTarget {
    /// 候補の出どころ。作成フォームの同じ欄を使い回す。
    fn field(self) -> ServerField {
        match self {
            Self::Connection => ServerField::Nic,
            Self::PacketFilter => ServerField::PacketFilter,
        }
    }
}

impl NicPicker {
    pub fn title(&self) -> String {
        let what = match self.target {
            NicTarget::Connection => "接続先",
            NicTarget::PacketFilter => "パケットフィルタ",
        };
        format!("{} の{what} — {}", self.nic.name(), self.server_name)
    }

    pub fn visible(&self, choices: &ServerChoices) -> Vec<ChoiceRow> {
        choices
            .rows(self.target.field())
            .into_iter()
            .filter(|row| matches(&self.filter, &row.haystack()))
            .collect()
    }

    pub fn move_selection(&mut self, forward: bool, len: usize) {
        self.index = cycle(self.index, len, forward);
    }

    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.index = 0;
    }
}

pub(super) fn edit_nic_picker(picker: &mut NicPicker, key: KeyEvent, visible: usize) {
    match key.code {
        KeyCode::Down => picker.move_selection(true, visible),
        KeyCode::Up => picker.move_selection(false, visible),
        KeyCode::Backspace => {
            let mut filter = picker.filter.clone();
            filter.pop();
            picker.set_filter(filter);
        }
        KeyCode::Char(c) => {
            let filter = format!("{}{c}", picker.filter);
            picker.set_filter(filter);
        }
        _ => {}
    }
}

/// パケットフィルタ本体（名前と説明）のフォーム。
#[derive(Debug, Clone, Default)]
pub struct PacketFilterForm {
    pub mode: PacketFilterFormMode,
    pub id: Option<ResourceId>,
    pub name: String,
    pub description: String,
    pub field: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PacketFilterFormMode {
    #[default]
    Create,
    Edit,
}

impl PacketFilterForm {
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

pub(super) fn edit_packet_filter_form(form: &mut PacketFilterForm, key: KeyEvent) {
    let fields = PacketFilterForm::LABELS.len();
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

/// パケットフィルタのルールのフォーム。
///
/// プロトコルと動作は選択式、残りは入力式。ポートを取らないプロトコルでは
/// ポートの欄を出さない（入れても送られないため、出すと誤解のもとになる）。
#[derive(Debug, Clone)]
pub struct RuleForm {
    pub mode: RuleFormMode,
    /// 編集のとき、元の並びでの位置。
    pub index: Option<usize>,
    pub protocol: usize,
    pub action: usize,
    pub source_network: String,
    pub source_port: String,
    pub destination_port: String,
    pub description: String,
    pub field: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleFormMode {
    Add,
    Edit,
}

/// ルールの入力欄。プロトコルによって出る欄が変わる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleField {
    Protocol,
    SourceNetwork,
    SourcePort,
    DestinationPort,
    Action,
    Description,
}

impl RuleField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Protocol => "プロトコル",
            Self::SourceNetwork => "送信元ネットワーク",
            Self::SourcePort => "送信元ポート",
            Self::DestinationPort => "宛先ポート",
            Self::Action => "動作",
            Self::Description => "説明",
        }
    }

    pub fn is_choice(self) -> bool {
        matches!(self, Self::Protocol | Self::Action)
    }
}

impl RuleForm {
    pub fn add() -> Self {
        Self {
            mode: RuleFormMode::Add,
            index: None,
            protocol: 0,
            action: 0,
            source_network: String::new(),
            source_port: String::new(),
            destination_port: String::new(),
            description: String::new(),
            field: 0,
        }
    }

    pub fn edit(index: usize, rule: &PacketFilterRule) -> Self {
        Self {
            mode: RuleFormMode::Edit,
            index: Some(index),
            protocol: crate::packet_filter::PROTOCOLS
                .iter()
                .position(|p| *p == rule.protocol)
                .unwrap_or(0),
            action: crate::packet_filter::ACTIONS
                .iter()
                .position(|a| *a == rule.action)
                .unwrap_or(0),
            source_network: rule.source_network.clone(),
            source_port: rule.source_port.clone(),
            destination_port: rule.destination_port.clone(),
            description: rule.description.clone(),
            field: 0,
        }
    }

    pub fn protocol(&self) -> &'static str {
        crate::packet_filter::PROTOCOLS
            [self.protocol.min(crate::packet_filter::PROTOCOLS.len() - 1)]
    }

    pub fn action(&self) -> &'static str {
        crate::packet_filter::ACTIONS[self.action.min(crate::packet_filter::ACTIONS.len() - 1)]
    }

    /// 今出ている欄。ポートを取らないプロトコルではポートの欄を出さない。
    pub fn fields(&self) -> Vec<RuleField> {
        let mut fields = vec![RuleField::Protocol, RuleField::SourceNetwork];
        if PacketFilterRule::takes_port(self.protocol()) {
            fields.extend([RuleField::SourcePort, RuleField::DestinationPort]);
        }
        fields.extend([RuleField::Action, RuleField::Description]);
        fields
    }

    pub fn current(&self) -> RuleField {
        self.fields()
            .get(self.field)
            .copied()
            .unwrap_or(RuleField::Protocol)
    }

    pub fn value(&self, field: RuleField) -> &str {
        match field {
            RuleField::SourceNetwork => &self.source_network,
            RuleField::SourcePort => &self.source_port,
            RuleField::DestinationPort => &self.destination_port,
            RuleField::Description => &self.description,
            _ => "",
        }
    }

    fn value_mut(&mut self, field: RuleField) -> Option<&mut String> {
        match field {
            RuleField::SourceNetwork => Some(&mut self.source_network),
            RuleField::SourcePort => Some(&mut self.source_port),
            RuleField::DestinationPort => Some(&mut self.destination_port),
            RuleField::Description => Some(&mut self.description),
            _ => None,
        }
    }

    pub fn to_rule(&self) -> PacketFilterRule {
        let ported = PacketFilterRule::takes_port(self.protocol());
        PacketFilterRule {
            protocol: self.protocol().to_string(),
            source_network: self.source_network.trim().to_string(),
            // 欄を出していないものは持ち越さない。
            source_port: if ported {
                self.source_port.trim().to_string()
            } else {
                String::new()
            },
            destination_port: if ported {
                self.destination_port.trim().to_string()
            } else {
                String::new()
            },
            action: self.action().to_string(),
            description: self.description.trim().to_string(),
        }
    }

    /// 送る前に形を確かめる。API のエラーは読みづらいので手前で止める。
    pub fn validate(&self) -> Result<(), String> {
        for (label, port) in [
            ("送信元ポート", &self.source_port),
            ("宛先ポート", &self.destination_port),
        ] {
            if PacketFilterRule::takes_port(self.protocol()) && !is_port_spec(port.trim()) {
                return Err(format!("{label}は 80 か 80-89 の形で入れてください"));
            }
        }
        Ok(())
    }
}

/// ポートの指定として使える文字列か。空は「すべて」の意味で通す。
fn is_port_spec(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let valid = |part: &str| part.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n));
    match text.split_once('-') {
        None => valid(text),
        Some((from, to)) => {
            valid(from)
                && valid(to)
                && from.parse::<u32>().unwrap_or(0) <= to.parse::<u32>().unwrap_or(0)
        }
    }
}

pub(super) fn edit_rule_form(form: &mut RuleForm, key: KeyEvent) {
    let count = form.fields().len();
    let current = form.current();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % count,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + count - 1) % count,
        KeyCode::Left | KeyCode::Right => {
            let forward = key.code == KeyCode::Right;
            match current {
                RuleField::Protocol => {
                    form.protocol = cycle(
                        form.protocol,
                        crate::packet_filter::PROTOCOLS.len(),
                        forward,
                    );
                    // ポートの欄が増減するので、選択位置をプロトコルの行に留める。
                    form.field = 0;
                }
                RuleField::Action => {
                    form.action = cycle(form.action, crate::packet_filter::ACTIONS.len(), forward)
                }
                _ => {}
            }
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(current) {
                value.pop();
            }
        }
        KeyCode::Char(c) if !current.is_choice() => {
            if let Some(value) = form.value_mut(current) {
                value.push(c);
            }
        }
        _ => {}
    }
}

/// ディスクの作成フォーム。
///
/// プラン（SSD / HDD）とサイズとソースは選択式。選べるサイズはプランごとに
/// 違うので、サーバー作成のコア数とメモリと同じく値そのものを持つ。
#[derive(Debug, Clone, Default)]
pub struct DiskCreateForm {
    pub name: String,
    pub description: String,
    /// ディスクプランの ID。一覧が届くまでは 0。
    pub plan_id: u32,
    pub size_mb: u32,
    /// ソースの添字。0 はブランク、以降は [`crate::iaas::OS_CHOICES`]。
    pub source: usize,
    pub field: usize,
}

impl DiskCreateForm {
    pub const LABELS: [&'static str; 5] = ["名前", "説明", "プラン", "サイズ", "ソース"];
    pub const CHOICE_FIELDS: [usize; 3] = [2, 3, 4];
    /// ブランクを先頭に置くぶん、OS の選択肢は1つずれる。
    pub const SOURCE_COUNT: usize = crate::iaas::OS_CHOICES.len() + 1;

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

    pub fn is_choice(index: usize) -> bool {
        Self::CHOICE_FIELDS.contains(&index)
    }

    /// 選んだソースの表示名。
    pub fn source_label(&self) -> &'static str {
        match self.source.checked_sub(1) {
            None => "ブランク（空のディスク）",
            Some(os) => crate::iaas::OS_CHOICES[os.min(crate::iaas::OS_CHOICES.len() - 1)].label,
        }
    }

    /// OS テンプレートのタグ。ブランクなら空。
    pub fn os_tags(&self) -> Vec<String> {
        match self.source.checked_sub(1) {
            None => Vec::new(),
            Some(os) => crate::iaas::OS_CHOICES[os.min(crate::iaas::OS_CHOICES.len() - 1)]
                .tags
                .iter()
                .map(|t| t.to_string())
                .collect(),
        }
    }

    /// プラン一覧が届いた時点で、まだ選んでいない欄を埋める。
    pub fn apply_defaults(&mut self, plans: &[DiskPlan]) {
        if self.plan_id == 0 {
            // SSD を既定にする。無ければ先頭。
            let Some(plan) = plans.iter().find(|p| p.is_ssd()).or_else(|| plans.first()) else {
                return;
            };
            self.plan_id = plan.id;
        }
        if self.size_mb == 0 {
            self.size_mb = default_disk_size(sizes_of(plans, self.plan_id));
        }
    }
}

/// そのプランで選べるサイズ（MB）。
pub fn sizes_of(plans: &[DiskPlan], plan_id: u32) -> &[u32] {
    plans
        .iter()
        .find(|p| p.id == plan_id)
        .map_or(&[][..], |p| &p.sizes_mb)
}

/// 既定のサイズ。20GB があればそれ、無ければ一番小さいもの。
fn default_disk_size(sizes: &[u32]) -> u32 {
    sizes
        .iter()
        .copied()
        .find(|mb| *mb == DEFAULT_DISK_SIZE_MB)
        .or_else(|| sizes.first().copied())
        .unwrap_or(0)
}

pub(super) fn edit_disk_create_form(form: &mut DiskCreateForm, key: KeyEvent, plans: &[DiskPlan]) {
    let fields = DiskCreateForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Left | KeyCode::Right => {
            let forward = key.code == KeyCode::Right;
            match form.field {
                2 => {
                    let ids: Vec<u32> = plans.iter().map(|p| p.id).collect();
                    form.plan_id = step(&ids, form.plan_id, forward);
                    // プランごとに選べるサイズが違うので、近いものへ寄せ直す。
                    let sizes = sizes_of(plans, form.plan_id);
                    if !sizes.contains(&form.size_mb) {
                        form.size_mb = sizes
                            .iter()
                            .copied()
                            .min_by_key(|mb| mb.abs_diff(form.size_mb))
                            .unwrap_or(form.size_mb);
                    }
                }
                3 => form.size_mb = step(sizes_of(plans, form.plan_id), form.size_mb, forward),
                4 => form.source = cycle(form.source, DiskCreateForm::SOURCE_COUNT, forward),
                _ => {}
            }
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(form.field) {
                value.pop();
            }
        }
        KeyCode::Char(c) if !DiskCreateForm::is_choice(form.field) => {
            let field = form.field;
            if let Some(value) = form.value_mut(field) {
                value.push(c);
            }
        }
        _ => {}
    }
}

/// ディスクの接続先サーバーを選ぶ画面。
///
/// 接続できるのは停止中のサーバーだけなので、絞ったものを持つ。
#[derive(Debug, Clone)]
pub struct DiskServerPicker {
    pub disk_id: ResourceId,
    pub disk_name: String,
    pub servers: Loadable<Vec<(ResourceId, String)>>,
    pub index: usize,
}

impl DiskServerPicker {
    /// 選択位置を上下に動かす。読み込み中は何もしない。
    pub fn move_selection(&mut self, forward: bool) {
        let len = self.servers.ready().map_or(0, Vec::len);
        self.index = cycle(self.index, len, forward);
    }
}

/// サーバーのプラン変更フォーム。
///
/// 変えられるのはコア数とメモリだけ。作成フォームと同じ規則で選ぶ。
#[derive(Debug, Clone)]
pub struct ServerPlanForm {
    pub server_id: ResourceId,
    pub server_name: String,
    /// 変更前の構成。確認の文言と「変わらない」判定に使う。
    pub original_cpu: u32,
    pub original_memory_mb: u32,
    pub cpu: u32,
    pub memory_mb: u32,
    pub field: usize,
}

impl ServerPlanForm {
    pub const LABELS: [&'static str; 2] = ["CPU", "メモリ"];

    /// 変更前と同じ構成か。同じなら API を叩く意味がない。
    pub fn is_unchanged(&self) -> bool {
        self.cpu == self.original_cpu && self.memory_mb == self.original_memory_mb
    }
}

pub(super) fn edit_server_plan_form(
    form: &mut ServerPlanForm,
    key: KeyEvent,
    plans: &[ServerPlan],
) {
    let fields = ServerPlanForm::LABELS.len();
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % fields,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + fields - 1) % fields,
        KeyCode::Left | KeyCode::Right => {
            let forward = key.code == KeyCode::Right;
            match form.field {
                0 => step_cpu(&mut form.cpu, &mut form.memory_mb, forward, plans),
                _ => step_memory(form.cpu, &mut form.memory_mb, forward, plans),
            }
        }
        _ => {}
    }
}

/// SSH 公開鍵の取得元。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshKeySource {
    /// さくらのクラウドに登録済みの鍵。
    Sacloud,
    /// 手元の `~/.ssh/*.pub`。
    Local,
    /// GitHub が公開している鍵。
    Github,
}

impl SshKeySource {
    pub const ALL: [SshKeySource; 3] = [Self::Sacloud, Self::Local, Self::Github];
    /// 登録フォームから開いたときの一覧。
    /// 登録済みの鍵をもう一度登録しても意味がないので外す。
    pub const WITHOUT_SACLOUD: [SshKeySource; 2] = [Self::Local, Self::Github];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sacloud => "さくらのクラウドに登録済みの公開鍵",
            Self::Local => "このパソコンの公開鍵",
            Self::Github => "GitHub のユーザー名から",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Sacloud => "アカウントの SSH 公開鍵",
            Self::Local => "~/.ssh/*.pub",
            Self::Github => "github.com/<名前>.keys",
        }
    }
}

/// 公開鍵を選び終わったあとに戻る先。
///
/// 選択画面はオーバーレイの枠を1つしか使えないので、呼び出し元のフォームを
/// そのまま預かって戻す。
#[derive(Debug, Clone)]
pub enum SshKeyReturn {
    ServerCreate(ServerCreateForm),
    Register(SshKeyForm),
}

impl SshKeyReturn {
    /// この呼び出し元で選べる取得元。
    pub fn sources(&self) -> &'static [SshKeySource] {
        match self {
            Self::ServerCreate(_) => &SshKeySource::ALL,
            Self::Register(_) => &SshKeySource::WITHOUT_SACLOUD,
        }
    }

    /// 選んだ鍵を書き戻す。
    pub fn take_key(&mut self, key: &PublicKey) {
        match self {
            Self::ServerCreate(form) => form.ssh_public_key = key.key.clone(),
            Self::Register(form) => {
                form.public_key = key.key.clone();
                // 名前が空なら、選んだ鍵の名前をそのまま使う。
                if form.name.trim().is_empty() {
                    form.name = key.label.clone();
                }
            }
        }
    }
}

/// 公開鍵を選ぶ画面の進み具合。
#[derive(Debug, Clone)]
pub enum SshKeyStage {
    /// どこから取るかを選ぶ。
    Source { index: usize },
    /// GitHub のユーザー名を入れる。
    GithubUser { user: String },
    /// 取得を待っている。
    Loading { from: String },
    /// 取れた鍵から選ぶ。
    Keys {
        from: String,
        keys: Vec<PublicKey>,
        index: usize,
    },
}

impl SshKeyStage {
    /// 一覧の選択位置を上下に動かす。一覧でなければ何もしない。
    pub fn move_selection(&mut self, forward: bool, sources: usize) {
        let (index, len) = match self {
            Self::Source { index } => (index, sources),
            Self::Keys { keys, index, .. } => {
                let len = keys.len();
                (index, len)
            }
            _ => return,
        };
        *index = cycle(*index, len, forward);
    }
}

/// SSH 公開鍵の登録・編集フォーム。
///
/// 鍵そのものは登録後に変えられないので、編集では名前と説明だけを扱う。
#[derive(Debug, Clone, Default)]
pub struct SshKeyForm {
    pub mode: SshKeyFormMode,
    pub id: Option<ResourceId>,
    pub name: String,
    pub description: String,
    pub public_key: String,
    pub field: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SshKeyFormMode {
    #[default]
    Add,
    Edit,
}

impl SshKeyForm {
    const ADD_LABELS: [&'static str; 3] = ["名前", "説明", "公開鍵"];
    const EDIT_LABELS: [&'static str; 2] = ["名前", "説明"];
    /// 公開鍵の欄。ここでだけ取得元を選べる。
    pub const PUBLIC_KEY_FIELD: usize = 2;

    pub fn labels(&self) -> &'static [&'static str] {
        match self.mode {
            SshKeyFormMode::Add => &Self::ADD_LABELS,
            SshKeyFormMode::Edit => &Self::EDIT_LABELS,
        }
    }

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.description,
            2 => &self.public_key,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.description),
            2 => Some(&mut self.public_key),
            _ => None,
        }
    }
}

pub(super) fn edit_ssh_key_form(form: &mut SshKeyForm, key: KeyEvent) {
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

/// RAGドキュメントの名前とタグの編集フォーム。
///
/// 仕様上これ以外の項目は取り込み時に決まる読み取り専用なので、2項目だけ扱う。
#[derive(Debug, Clone, Default)]
pub struct RagEditForm {
    pub id: String,
    /// 変更前の名前。確認の文言に使う。
    pub original_name: String,
    pub name: String,
    pub tags: String,
    pub field: usize,
}

impl RagEditForm {
    pub const LABELS: [&'static str; 2] = ["名前", "タグ（カンマ区切り）"];

    pub fn value(&self, index: usize) -> &str {
        match index {
            0 => &self.name,
            1 => &self.tags,
            _ => "",
        }
    }

    fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.tags),
            _ => None,
        }
    }

    pub(super) fn tag_list(&self) -> Vec<String> {
        split_tags(&self.tags)
    }
}

/// カンマ区切りのタグを配列にする。空の要素は捨てる。
fn split_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match (self.mode, index) {
            (_, 0) => Some(&mut self.name),
            (_, 1) => Some(&mut self.description),
            (VaultFormMode::Create, 2) => Some(&mut self.kms_key_id),
            (_, 3) => Some(&mut self.tags),
            _ => None,
        }
    }

    pub(super) fn tags(&self) -> Vec<String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.log_storage_id),
            1 => Some(&mut self.metrics_storage_id),
            2 => Some(&mut self.name),
            3 => Some(&mut self.description),
            4 => Some(&mut self.rule_json),
            _ => None,
        }
    }

    pub(super) fn input(&self) -> Result<LogMeasureRuleInput, String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.publisher_code),
            1 => Some(&mut self.variant),
            2 => Some(&mut self.resource_id),
            3 => Some(&mut self.log_storage_id),
            _ => None,
        }
    }

    pub(super) fn input(&self) -> Result<LogRoutingInput, String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.publisher_code),
            1 => Some(&mut self.variant),
            2 => Some(&mut self.resource_id),
            3 => Some(&mut self.metrics_storage_id),
            _ => None,
        }
    }

    pub(super) fn input(&self) -> Result<MetricsRoutingInput, String> {
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
    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            1 => Some(&mut self.resend_interval),
            2 => Some(&mut self.match_labels),
            _ => None,
        }
    }
}

impl AlertRuleForm {
    pub const FIELDS: usize = 9;

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn input(&self) -> Result<AlertRuleInput, String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
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

    pub(super) fn value_mut(&mut self, index: usize) -> Option<&mut String> {
        match index {
            0 => Some(&mut self.name),
            1 => Some(&mut self.record_type),
            2 => Some(&mut self.data),
            3 => Some(&mut self.ttl),
            _ => None,
        }
    }
}

pub(super) fn edit_user_form(form: &mut UserForm, key: KeyEvent) {
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

pub(super) fn edit_registry_form(form: &mut RegistryForm, key: KeyEvent) {
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

pub(super) fn edit_iam_resource_form(form: &mut IamResourceForm, key: KeyEvent) {
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

pub(super) fn edit_iam_role_form(form: &mut IamRoleForm, key: KeyEvent) {
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

/// サーバー作成フォームのキー入力。
///
/// 選べる値はゾーンごとに違うので、一覧をそのまま受け取って隣の値へ動かす。
pub(super) fn edit_server_create_form(
    form: &mut ServerCreateForm,
    key: KeyEvent,
    choices: &ServerChoices,
) {
    let fields = form.fields(choices);
    let count = fields.len();
    let current = form.current(choices);
    match key.code {
        KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % count,
        KeyCode::BackTab | KeyCode::Up => form.field = (form.field + count - 1) % count,
        // 選択式の欄は左右で切り替える。
        KeyCode::Left | KeyCode::Right => {
            let forward = key.code == KeyCode::Right;
            match current {
                ServerField::Cpu => {
                    step_cpu(&mut form.cpu, &mut form.memory_mb, forward, &choices.plans)
                }
                ServerField::Memory => {
                    step_memory(form.cpu, &mut form.memory_mb, forward, &choices.plans)
                }
                ServerField::Os => form.os = cycle(form.os, crate::iaas::OS_CHOICES.len(), forward),
                ServerField::DiskSize => {
                    form.disk_size_mb = step(&choices.disk_sizes, form.disk_size_mb, forward)
                }
                ServerField::Nic => {
                    form.nic = cycle(form.nic, choices.nics.len(), forward);
                    // IP の欄が増減するので、選択位置を NIC の行に留める。
                    form.field = form
                        .fields(choices)
                        .iter()
                        .position(|f| *f == ServerField::Nic)
                        .unwrap_or(form.field);
                }
                ServerField::PacketFilter => {
                    form.packet_filter =
                        cycle(form.packet_filter, choices.packet_filters.len(), forward)
                }
                ServerField::StartupScript => {
                    form.startup_script =
                        cycle(form.startup_script, choices.startup_scripts.len(), forward)
                }
                ServerField::Boot => form.boot_after_create = !form.boot_after_create,
                _ => {}
            }
        }
        KeyCode::Backspace => {
            if let Some(value) = form.value_mut(current) {
                value.pop();
            }
        }
        KeyCode::Char(' ') if current == ServerField::Boot => {
            form.boot_after_create = !form.boot_after_create;
        }
        KeyCode::Char(c) if !current.is_choice() => {
            if let Some(value) = form.value_mut(current) {
                value.push(c);
            }
        }
        _ => {}
    }
}

/// コア数を隣へ動かし、メモリをその構成で選べる値へ寄せ直す。
///
/// 作成フォームとプラン変更フォームで同じ規則を使う。寄せ直しを忘れると
/// 存在しない組み合わせのまま送信してしまう。
fn step_cpu(cpu: &mut u32, memory_mb: &mut u32, forward: bool, plans: &[ServerPlan]) {
    *cpu = step(&crate::iaas::cpu_choices(plans), *cpu, forward);
    *memory_mb = crate::iaas::nearest_memory(plans, *cpu, *memory_mb);
}

/// メモリを、そのコア数で選べる値の中で隣へ動かす。
fn step_memory(cpu: u32, memory_mb: &mut u32, forward: bool, plans: &[ServerPlan]) {
    *memory_mb = step(
        &crate::iaas::memory_choices(plans, cpu),
        *memory_mb,
        forward,
    );
}

/// 昇順に並んだ選択肢の中で、今の値の隣へ動かす。
///
/// 今の値が一覧に無ければ一番近いものから動かす。コア数を変えたあとの
/// メモリのように、一覧そのものが入れ替わることがあるため。
fn step(choices: &[u32], current: u32, forward: bool) -> u32 {
    if choices.is_empty() {
        return current;
    }
    let at = choices
        .iter()
        .position(|v| *v == current)
        .unwrap_or_else(|| {
            choices
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| v.abs_diff(current))
                .map_or(0, |(i, _)| i)
        });
    choices[cycle(at, choices.len(), forward)]
}

/// 選択肢を巡回させる。空なら 0 のまま。
fn cycle(current: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    if forward {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

pub(super) fn edit_rag_edit_form(form: &mut RagEditForm, key: KeyEvent) {
    let fields = RagEditForm::LABELS.len();
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

pub(super) fn edit_rag_upload_form(form: &mut RagUploadForm, key: KeyEvent) {
    let fields = RagUploadForm::LABELS.len();
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

pub(super) fn edit_switch_form(form: &mut SwitchForm, key: KeyEvent) {
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

pub(super) fn edit_dns_record_form(form: &mut DnsRecordForm, key: KeyEvent) {
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

pub(super) fn edit_dns_zone_form(form: &mut DnsZoneForm, key: KeyEvent) {
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

pub(super) fn edit_simple_monitor_form(form: &mut SimpleMonitorForm, key: KeyEvent) {
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

pub(super) fn edit_vault_form(form: &mut VaultForm, key: KeyEvent) {
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

pub(super) fn edit_alert_project_form(form: &mut AlertProjectForm, key: KeyEvent) {
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

pub(super) fn edit_alert_rule_form(form: &mut AlertRuleForm, key: KeyEvent) {
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

pub(super) fn edit_log_measure_rule_form(form: &mut LogMeasureRuleForm, key: KeyEvent) {
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

pub(super) fn edit_log_routing_form(form: &mut LogRoutingForm, key: KeyEvent) {
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

pub(super) fn edit_metrics_routing_form(form: &mut MetricsRoutingForm, key: KeyEvent) {
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

pub(super) fn edit_dashboard_form(form: &mut DashboardForm, key: KeyEvent) {
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

pub(super) fn edit_notification_target_form(form: &mut NotificationTargetForm, key: KeyEvent) {
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

pub(super) fn edit_notification_routing_form(form: &mut NotificationRoutingForm, key: KeyEvent) {
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

pub(super) fn edit_storage_form(form: &mut StorageForm, key: KeyEvent) {
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

pub(super) fn edit_secret_form(form: &mut SecretForm, key: KeyEvent) {
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

pub(super) fn edit_profile_form(form: &mut ProfileForm, key: KeyEvent) {
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

pub(super) fn edit_login_form(form: &mut LoginForm, key: KeyEvent) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iaas::StartupScript;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn plans() -> Vec<ServerPlan> {
        [(1, 1024), (1, 2048), (2, 2048), (2, 4096), (4, 8192)]
            .into_iter()
            .map(|(cpu, memory_mb)| ServerPlan {
                name: format!("{cpu}c{}g", memory_mb / 1024),
                cpu,
                memory_mb,
                commitment: "standard".to_string(),
                generation: 200,
                availability: "available".to_string(),
            })
            .collect()
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// 選択肢は端で折り返す。一覧が長いとき、最大値へは ← 一回で届く。
    #[test]
    fn stepping_wraps_around_at_both_ends() {
        let choices = [1, 2, 4, 8];
        assert_eq!(step(&choices, 2, true), 4);
        assert_eq!(step(&choices, 8, true), 1);
        assert_eq!(step(&choices, 1, false), 8);
    }

    /// 一覧に無い値からでも、一番近いところから動き出す。
    #[test]
    fn stepping_starts_from_the_nearest_value() {
        let choices = [1024, 4096, 8192];
        assert_eq!(step(&choices, 3072, true), 8192);
        assert_eq!(step(&choices, 3072, false), 1024);
        // 一覧が空なら動かない。
        assert_eq!(step(&[], 512, true), 512);
    }

    /// 作成フォームの選択肢。スイッチとフィルタを1つずつ持たせる。
    fn server_choices() -> ServerChoices {
        ServerChoices {
            plans: plans(),
            disk_sizes: vec![20480, 40960],
            nics: vec![
                NicChoice::Shared,
                NicChoice::Switch(ResourceId(7), "sw-01".to_string()),
                NicChoice::None,
            ],
            packet_filters: vec![None, Some((ResourceId(8), "web".to_string()))],
            startup_scripts: vec![None],
        }
    }

    /// 欄の位置を名前で探す。添字を直に書くと欄を足したときに壊れる。
    fn field_at(choices: &ServerChoices, form: &ServerCreateForm, want: ServerField) -> usize {
        form.fields(choices)
            .iter()
            .position(|f| *f == want)
            .unwrap_or_else(|| panic!("{want:?} の欄が無い"))
    }

    /// コア数を変えると、その構成で選べるメモリへ寄る。
    /// 寄せないと存在しない組み合わせのまま作成に進んでしまう。
    #[test]
    fn changing_the_cpu_moves_the_memory_to_a_valid_pair() {
        let choices = server_choices();
        let mut form = ServerCreateForm {
            cpu: 2,
            memory_mb: 4096,
            ..ServerCreateForm::default()
        };
        form.field = field_at(&choices, &form, ServerField::Cpu);
        // 4 コアは 8GB しか選べない。
        edit_server_create_form(&mut form, press(KeyCode::Right), &choices);
        assert_eq!((form.cpu, form.memory_mb), (4, 8192));
        // 1 コアに戻すと 2GB が一番近い。
        edit_server_create_form(&mut form, press(KeyCode::Right), &choices);
        assert_eq!((form.cpu, form.memory_mb), (1, 2048));
    }

    /// メモリはそのコア数で選べるものだけを巡る。
    #[test]
    fn memory_only_cycles_within_the_chosen_cpu() {
        let choices = server_choices();
        let mut form = ServerCreateForm {
            cpu: 1,
            memory_mb: 1024,
            ..ServerCreateForm::default()
        };
        form.field = field_at(&choices, &form, ServerField::Memory);
        edit_server_create_form(&mut form, press(KeyCode::Right), &choices);
        assert_eq!(form.memory_mb, 2048);
        // 1 コアは 2 通りしかないので折り返す。
        edit_server_create_form(&mut form, press(KeyCode::Right), &choices);
        assert_eq!(form.memory_mb, 1024);
    }

    /// 選択式の欄では文字が入らないこと。入ると見えない値が混ざる。
    #[test]
    fn typing_does_nothing_on_a_choice_field() {
        let choices = server_choices();
        let mut form = ServerCreateForm {
            cpu: 1,
            memory_mb: 1024,
            ..ServerCreateForm::default()
        };
        form.field = field_at(&choices, &form, ServerField::Cpu);
        edit_server_create_form(&mut form, press(KeyCode::Char('x')), &choices);
        assert_eq!(form.name, "");
        assert_eq!(form.cpu, 1);
    }

    /// スイッチに繋ぐときだけ IP の欄が出ること。
    /// 共有セグメントでは DHCP が効くので、出しても入れる意味がない。
    #[test]
    fn the_ip_fields_appear_only_for_a_switch() {
        let choices = server_choices();
        let mut form = ServerCreateForm::default();
        let plain = form.fields(&choices);
        assert!(!plain.contains(&ServerField::IpAddress));

        form.field = field_at(&choices, &form, ServerField::Nic);
        edit_server_create_form(&mut form, press(KeyCode::Right), &choices);
        assert!(matches!(choices.nic(form.nic), NicChoice::Switch(..)));
        let with_switch = form.fields(&choices);
        assert!(with_switch.contains(&ServerField::IpAddress));
        assert!(with_switch.contains(&ServerField::MaskLen));
        assert!(with_switch.contains(&ServerField::Gateway));
        // 欄が増えても、選択位置は NIC の行に残る。
        assert_eq!(form.current(&choices), ServerField::Nic);

        // 「接続しない」に進めると IP の欄はまた消える。
        edit_server_create_form(&mut form, press(KeyCode::Right), &choices);
        assert_eq!(choices.nic(form.nic), NicChoice::None);
        assert!(!form.fields(&choices).contains(&ServerField::IpAddress));
        assert_eq!(form.current(&choices), ServerField::Nic);
    }

    fn script_choices() -> ServerChoices {
        let script = |id: u64, name: &str, own: bool, description: &str| {
            Some(StartupScript {
                id: ResourceId(id),
                name: name.to_string(),
                class: "shell".to_string(),
                scope: if own { "user" } else { "shared" }.to_string(),
                description: description.to_string(),
                tags: Vec::new(),
            })
        };
        ServerChoices {
            startup_scripts: vec![
                None,
                script(1, "自分の初期設定", true, "パッケージを入れる"),
                script(2, "WordPress", false, "WordPressを入れる"),
                script(3, "Redmine", false, "Redmineを入れる"),
            ],
            ..server_choices()
        }
    }

    /// 名前でも説明でも絞り込めること。数十件から目当てを探せるようにする。
    #[test]
    fn the_picker_narrows_by_name_and_by_description() {
        let choices = script_choices();
        let mut picker = ServerChoicePicker {
            form: Box::new(ServerCreateForm::default()),
            target: ServerField::StartupScript,
            filter: String::new(),
            index: 0,
        };
        assert_eq!(picker.visible(&choices).len(), 4);

        picker.set_filter("word".to_string());
        let rows = picker.visible(&choices);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "WordPress");
        // 元の一覧での位置を持ち回るので、絞り込んでも正しい値を選べる。
        assert_eq!(rows[0].position, 2);

        // 説明にしか出てこない語でも見つかる。
        picker.set_filter("パッケージ".to_string());
        assert_eq!(picker.visible(&choices)[0].label, "自分の初期設定");
    }

    /// 絞り込みを変えたら選択位置を先頭へ戻すこと。
    /// 残したままだと、見えていない行を選んだまま決定してしまう。
    #[test]
    fn narrowing_resets_the_selection() {
        let mut picker = ServerChoicePicker {
            form: Box::new(ServerCreateForm::default()),
            target: ServerField::StartupScript,
            filter: String::new(),
            index: 3,
        };
        picker.set_filter("word".to_string());
        assert_eq!(picker.index, 0);
    }

    /// 一覧で選んだものが欄に戻ること。
    #[test]
    fn the_picker_writes_the_choice_back_to_the_form() {
        let choices = script_choices();
        let mut form = ServerCreateForm::default();
        assert_eq!(form.choice_position(ServerField::StartupScript), 0);
        form.take_choice(ServerField::StartupScript, 2);
        assert_eq!(form.choice_position(ServerField::StartupScript), 2);
        assert_eq!(
            choices.startup_script(form.startup_script).unwrap().name,
            "WordPress"
        );
    }

    /// 候補が少ない欄では一覧を出さないこと。
    #[test]
    fn only_the_long_lists_get_a_picker() {
        for target in [
            ServerField::Nic,
            ServerField::PacketFilter,
            ServerField::StartupScript,
        ] {
            assert!(ServerChoices::is_list_field(target), "{target:?}");
        }
        for target in [ServerField::Cpu, ServerField::Os, ServerField::Name] {
            assert!(!ServerChoices::is_list_field(target), "{target:?}");
            assert!(server_choices().rows(target).is_empty());
        }
    }

    /// ポートを取らないプロトコルではポートの欄を出さないこと。
    /// 出したままにすると、送られない値を入れさせてしまう。
    #[test]
    fn the_port_fields_appear_only_for_tcp_and_udp() {
        let mut form = RuleForm::add();
        assert_eq!(form.protocol(), "tcp");
        assert!(form.fields().contains(&RuleField::DestinationPort));

        // tcp → udp → icmp と進める。
        edit_rule_form(&mut form, press(KeyCode::Right));
        assert_eq!(form.protocol(), "udp");
        assert!(form.fields().contains(&RuleField::SourcePort));
        edit_rule_form(&mut form, press(KeyCode::Right));
        assert_eq!(form.protocol(), "icmp");
        assert!(!form.fields().contains(&RuleField::SourcePort));
        assert!(!form.fields().contains(&RuleField::DestinationPort));
        // 欄が減っても、選択位置はプロトコルの行に残る。
        assert_eq!(form.current(), RuleField::Protocol);
    }

    /// 欄を出していないポートは送らないこと。
    #[test]
    fn a_hidden_port_is_not_sent() {
        let mut form = RuleForm::add();
        form.destination_port = "22".to_string();
        assert_eq!(form.to_rule().destination_port, "22");
        // icmp にすると、入力済みの値は持ち越さない。
        form.protocol = crate::packet_filter::PROTOCOLS
            .iter()
            .position(|p| *p == "icmp")
            .unwrap();
        assert_eq!(form.to_rule().destination_port, "");
    }

    /// ポートの書き方を送る前に確かめること。
    #[test]
    fn ports_are_checked_before_sending() {
        let ok = |port: &str| {
            let mut form = RuleForm::add();
            form.destination_port = port.to_string();
            form.validate().is_ok()
        };
        assert!(ok(""), "空欄は「すべて」の意味で通す");
        assert!(ok("80"));
        assert!(ok("80-89"));
        assert!(!ok("0"));
        assert!(!ok("70000"));
        assert!(!ok("89-80"), "順序が逆な範囲は弾く");
        assert!(!ok("http"));
    }

    /// 編集では今の値が入った状態で開くこと。
    #[test]
    fn editing_a_rule_starts_from_its_current_values() {
        let rule = PacketFilterRule {
            protocol: "udp".to_string(),
            source_network: "192.0.2.0/24".to_string(),
            destination_port: "53".to_string(),
            action: "deny".to_string(),
            description: "DNS".to_string(),
            ..PacketFilterRule::default()
        };
        let form = RuleForm::edit(3, &rule);
        assert_eq!(form.protocol(), "udp");
        assert_eq!(form.action(), "deny");
        assert_eq!(form.index, Some(3));
        assert_eq!(form.to_rule(), rule);
    }

    /// パケットフィルタとスタートアップスクリプトは「なし」が既定。
    #[test]
    fn attachments_default_to_none() {
        let choices = server_choices();
        let form = ServerCreateForm::default();
        assert!(choices.packet_filter(form.packet_filter).is_none());
        assert!(choices.startup_script(form.startup_script).is_none());
        assert_eq!(choices.nic(form.nic), NicChoice::Shared);
    }

    fn disk_plans() -> Vec<DiskPlan> {
        vec![
            DiskPlan {
                id: 4,
                name: "SSD".to_string(),
                sizes_mb: vec![20480, 40960, 102400],
            },
            DiskPlan {
                id: 2,
                name: "HDD".to_string(),
                sizes_mb: vec![40960, 102400, 256000],
            },
        ]
    }

    /// 既定は SSD の 20GB。
    #[test]
    fn a_new_disk_starts_at_ssd_20gb() {
        let mut form = DiskCreateForm::default();
        form.apply_defaults(&disk_plans());
        assert_eq!((form.plan_id, form.size_mb), (4, 20480));
        // ソースの既定はブランク。
        assert_eq!(form.source, 0);
        assert!(form.os_tags().is_empty());
    }

    /// プランを変えたら、そのプランで選べるサイズへ寄ること。
    /// HDD には 20GB が無いので、そのままだと作成が弾かれる。
    #[test]
    fn changing_the_disk_plan_moves_the_size_into_range() {
        let plans = disk_plans();
        let mut form = DiskCreateForm {
            field: 2,
            ..DiskCreateForm::default()
        };
        form.apply_defaults(&plans);
        edit_disk_create_form(&mut form, press(KeyCode::Right), &plans);
        assert_eq!(form.plan_id, 2);
        assert_eq!(form.size_mb, 40960, "HDD で選べる一番近いサイズへ寄る");
    }

    /// ソースはブランクを先頭に、OS の選択肢が続くこと。
    #[test]
    fn the_source_list_puts_blank_first() {
        let plans = disk_plans();
        let mut form = DiskCreateForm {
            field: 4,
            ..DiskCreateForm::default()
        };
        edit_disk_create_form(&mut form, press(KeyCode::Right), &plans);
        assert_eq!(form.source, 1);
        assert_eq!(form.os_tags(), crate::iaas::OS_CHOICES[0].tags);
        // 端で折り返す。
        edit_disk_create_form(&mut form, press(KeyCode::Left), &plans);
        assert_eq!(form.source_label(), "ブランク（空のディスク）");
    }

    /// プラン変更でもコア数を変えたらメモリが追従すること。
    /// 作成フォームと同じ規則を共用しているかの確認。
    #[test]
    fn the_plan_form_snaps_memory_like_the_create_form() {
        let plans = plans();
        let mut form = ServerPlanForm {
            server_id: crate::sacloud::ResourceId(1),
            server_name: "web-01".to_string(),
            original_cpu: 1,
            original_memory_mb: 1024,
            cpu: 1,
            memory_mb: 1024,
            field: 0,
        };
        assert!(form.is_unchanged());
        // 2 コアの最小は 2GB。
        edit_server_plan_form(&mut form, press(KeyCode::Right), &plans);
        assert_eq!((form.cpu, form.memory_mb), (2, 2048));
        assert!(!form.is_unchanged());
        // 変更前の値は動かない。確認の文言に使うため。
        assert_eq!((form.original_cpu, form.original_memory_mb), (1, 1024));
    }

    /// 一覧の選択位置は上下で折り返す。
    #[test]
    fn the_key_picker_wraps_its_selection() {
        let mut stage = SshKeyStage::Source { index: 0 };
        stage.move_selection(false, SshKeySource::ALL.len());
        let SshKeyStage::Source { index } = stage else {
            panic!("取得元の選択が別の状態になった");
        };
        assert_eq!(index, SshKeySource::ALL.len() - 1);
    }

    /// 登録フォームからは、登録済みの鍵を取得元に出さないこと。
    /// 出しても、すでにある鍵をもう一度登録することになる。
    #[test]
    fn the_register_form_hides_the_already_registered_source() {
        let from_server = SshKeyReturn::ServerCreate(ServerCreateForm::default());
        assert_eq!(from_server.sources().len(), 3);
        assert!(from_server.sources().contains(&SshKeySource::Sacloud));

        let from_register = SshKeyReturn::Register(SshKeyForm::default());
        assert_eq!(from_register.sources().len(), 2);
        assert!(!from_register.sources().contains(&SshKeySource::Sacloud));
    }

    /// 選んだ鍵は呼び出し元ごとに違う欄へ入ること。
    #[test]
    fn a_chosen_key_goes_back_to_the_form_that_asked_for_it() {
        let key = PublicKey {
            label: "id_ed25519.pub".to_string(),
            key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample1".to_string(),
        };

        let mut back = SshKeyReturn::ServerCreate(ServerCreateForm::default());
        back.take_key(&key);
        let SshKeyReturn::ServerCreate(form) = back else {
            panic!("呼び出し元が入れ替わった");
        };
        assert_eq!(form.ssh_public_key, key.key);

        // 登録フォームでは、名前が空なら鍵の名前を借りる。
        let mut back = SshKeyReturn::Register(SshKeyForm::default());
        back.take_key(&key);
        let SshKeyReturn::Register(form) = back else {
            panic!("呼び出し元が入れ替わった");
        };
        assert_eq!(form.public_key, key.key);
        assert_eq!(form.name, "id_ed25519.pub");

        // すでに名前が入っていれば触らない。
        let mut back = SshKeyReturn::Register(SshKeyForm {
            name: "自分でつけた名前".to_string(),
            ..SshKeyForm::default()
        });
        back.take_key(&key);
        let SshKeyReturn::Register(form) = back else {
            panic!("呼び出し元が入れ替わった");
        };
        assert_eq!(form.name, "自分でつけた名前");
    }

    /// 編集では鍵の欄を出さないこと。鍵そのものは API で変えられない。
    #[test]
    fn editing_a_key_only_offers_the_name_and_description() {
        let add = SshKeyForm::default();
        assert_eq!(add.labels().len(), 3);
        let edit = SshKeyForm {
            mode: SshKeyFormMode::Edit,
            ..SshKeyForm::default()
        };
        assert_eq!(edit.labels(), ["名前", "説明"]);
    }

    /// 名前の入力中は上下キーで選択位置が動かないこと。
    #[test]
    fn the_key_picker_ignores_selection_while_typing() {
        let mut stage = SshKeyStage::GithubUser {
            user: "octocat".to_string(),
        };
        stage.move_selection(true, SshKeySource::ALL.len());
        let SshKeyStage::GithubUser { user } = stage else {
            panic!("入力中に別の状態になった");
        };
        assert_eq!(user, "octocat");
    }
}
