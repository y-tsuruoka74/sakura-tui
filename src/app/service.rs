//! サービスの分類と一覧、およびサービス選択画面での移動計算。
//!
//! サービスを増やすときに触るのはこのファイルだけで済むようにしてある。
//! `Service::ALL` は分類順に並んでいる前提なので、並べ替えるときは
//! `Category::ALL` との対応を崩さないこと（`mod.rs` のテストが見張っている）。

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
pub(super) fn availability_reason(error: &str) -> &'static str {
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
    Delivery,
    Storage,
    Security,
    Ops,
    Account,
}

impl Category {
    /// 表示順。サービス一覧の並びもこの順に揃える。
    pub const ALL: [Category; 10] = [
        Category::Compute,
        Category::Container,
        Category::Ai,
        Category::Integration,
        Category::Network,
        Category::Delivery,
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
            Category::Delivery => "負荷分散・配信",
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
    SshKey,
    Switch,
    NetworkMap,
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
    pub const ALL: [Service; 45] = [
        // コンピュート
        Service::Server,
        Service::SshKey,
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
        Service::NetworkMap,
        Service::Switch,
        Service::Internet,
        Service::PacketFilter,
        Service::Bridge,
        Service::VpcRouter,
        Service::LocalRouter,
        Service::MobileGateway,
        Service::Seg,
        Service::NetworkingSuite,
        // 負荷分散・配信
        Service::LoadBalancer,
        Service::EnhancedLoadBalancer,
        Service::Gslb,
        Service::Dns,
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
            Service::SshKey => ServiceMeta {
                category: Category::Compute,
                title: "SSH公開鍵",
                arg_name: "ssh-key",
                countable_label: Some("公開鍵"),
                count_label: Some("件"),
                // アカウント共通で、ゾーンを切り替えても同じものが見える。
                zoned: false,
            },
            Service::NetworkMap => ServiceMeta {
                category: Category::Network,
                title: "接続マップ",
                arg_name: "network-map",
                countable_label: None,
                count_label: None,
                zoned: true,
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
                category: Category::Delivery,
                title: "ロードバランサ",
                arg_name: "loadbalancer",
                countable_label: Some("ロードバランサ"),
                count_label: Some("台"),
                zoned: true,
            },
            Service::EnhancedLoadBalancer => ServiceMeta {
                category: Category::Delivery,
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
                category: Category::Delivery,
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
                category: Category::Delivery,
                title: "ウェブアクセラレータ",
                arg_name: "webaccel",
                countable_label: None,
                count_label: Some("サイト"),
                zoned: false,
            },
            Service::Dns => ServiceMeta {
                category: Category::Delivery,
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

pub(super) fn category_service_indices(category: Category) -> Vec<usize> {
    Service::ALL
        .iter()
        .enumerate()
        .filter_map(|(index, service)| (service.category() == category).then_some(index))
        .collect()
}

pub(super) fn move_service_within_category(index: usize, delta: i32) -> usize {
    let indices = category_service_indices(Service::ALL[index].category());
    let position = indices
        .iter()
        .position(|candidate| *candidate == index)
        .unwrap_or(0) as i32;
    indices[(position + delta).rem_euclid(indices.len() as i32) as usize]
}

pub(super) fn move_service_category(index: usize, delta: i32) -> usize {
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
