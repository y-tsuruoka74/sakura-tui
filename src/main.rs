//! さくらインターネットのサービスをターミナルから操作する TUI。
//!
//! コンピュート、ネットワーク、コンテナ、監視など複数サービスを扱う。

mod account;
mod app;
mod apprun;
mod apprun_dedicated;
mod billing;
mod cloud_resources;
mod commonservice;
mod config;
mod http;
mod iaas;
mod keychain;
mod managed_resources;
mod monitoring;
mod registry;
mod sacloud;
mod secretmanager;
mod switch;
mod ui;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;

use crate::app::App;
use crate::config::{ApiCredentials, CredentialSource};
use crate::sacloud::SacloudClient;

/// スピナーを回すための描画間隔。
const TICK: Duration = Duration::from_millis(120);

/// `--service` に渡せる名前を分類ごとに並べる。
///
/// 一覧を手で書くとサービスを増やしたときに書き漏れるので、
/// `Service::ALL` から組み立てる。
fn service_names() -> String {
    let mut out = String::new();
    for category in app::Category::ALL {
        let names: Vec<&str> = category.services().map(|svc| svc.arg_name()).collect();
        if names.is_empty() {
            continue;
        }
        // 全角を含むので、文字数ではなく表示幅で揃える。
        out.push_str(&format!(
            "\n                         {} {}",
            ui::pad(category.title(), 22),
            names.join(" / ")
        ));
    }
    out
}

/// `--help` に出す使い方。
fn usage() -> String {
    USAGE.replace("{サービス名}", &service_names())
}

const USAGE: &str = "さくらインターネットのサービスをターミナルから操作する TUI

使い方:
  sakura-tui [オプション]

オプション:
  -p, --profile <名前>   使う usacloud プロファイル
  -z, --zone <ゾーン>     ゾーン依存のサービスで使うゾーン (例: is1a)
  -s, --service <名前>    起動時に開くサービス{サービス名}
      --api-root <URL>   接続先の API ルート（既定: 本番）
                         社内テスト環境なら
                         https://secure.sakura.ad.jp/cloud-test/zone
      --trace            APIリクエストを標準エラーに記録する
  -h, --help             このヘルプ
  -V, --version          バージョン

環境変数:
  SAKURA_ACCESS_TOKEN / SAKURA_ACCESS_TOKEN_SECRET   APIキー
  SAKURA_PROFILE                                     usacloud プロファイル名
  SAKURA_TUI_CONFIG                                  設定ファイルのパス
  SAKURA_API_ROOT_URL                                --api-root と同じ
  SAKURA_TUI_TRACE                                   --trace と同じ";

/// コマンドライン引数。
#[derive(Debug, Default)]
struct Args {
    profile: Option<String>,
    zone: Option<String>,
    service: Option<app::Service>,
}

/// 引数を解析する。`--help` / `--version` はここで出力して終了する。
fn parse_args() -> Result<Args> {
    let mut args = Args::default();
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut value = |name: &str| {
            argv.next()
                .with_context(|| format!("{name} に値が指定されていません"))
        };
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("sakura-tui {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-p" | "--profile" => args.profile = Some(value("--profile")?),
            "-z" | "--zone" => args.zone = Some(value("--zone")?),
            "-s" | "--service" => {
                let name = value("--service")?;
                args.service = Some(app::Service::from_arg(&name).with_context(|| {
                    format!("不明なサービス名です: {name}\n指定できる名前は --help を参照")
                })?);
            }
            "--api-root" => {
                let url = value("--api-root")?;
                // 環境変数と同じ経路に流して、プロファイル読み込みより先に効かせる。
                unsafe { std::env::set_var("SAKURA_API_ROOT_URL", url) };
            }
            "--trace" => unsafe { std::env::set_var("SAKURA_TUI_TRACE", "1") },
            other => bail!("不明なオプションです: {other}\n使い方は --help を参照"),
        }
    }
    Ok(args)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };
    // --profile は環境変数より優先させたいので、読み込み前に反映する。
    if let Some(profile) = &args.profile {
        unsafe { std::env::set_var("SAKURA_PROFILE", profile) };
    }

    // 認証情報が無くても TUI を起動し、アプリ内の作成フォームへ案内する。
    // 空の認証情報で作ったクライアントはオンボーディング中には通信へ使わない。
    let (credentials, has_credentials) = match config::load_api_credentials(args.profile.is_some())
    {
        Ok(credentials) => (credentials, true),
        Err(_) => (
            ApiCredentials {
                token: String::new(),
                secret: String::new(),
                source: CredentialSource::Env,
                zone: args.zone.clone(),
                api_root: std::env::var("SAKURA_API_ROOT_URL").ok(),
            },
            false,
        ),
    };
    let clients = Clients::new(&credentials)?;
    let mut settings = match config::Config::load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("警告: 設定ファイルを読めませんでした: {err}");
            config::Config::default()
        }
    };
    if let Err(err) = keychain::availability() {
        eprintln!(
            "警告: {err}\n\
             レジストリのログイン情報は保存されず、起動中のみ有効になります。"
        );
    }
    // 以前のバージョンが平文で保存したパスワードがあれば、キーチェーンへ移す。
    match settings.migrate_plaintext_passwords() {
        Ok(0) => {}
        Ok(moved) => {
            eprintln!("設定ファイルにあった {moved} 件のパスワードをOSのキーチェーンへ移しました。")
        }
        Err(err) => eprintln!(
            "警告: 平文パスワードをキーチェーンへ移せませんでした: {err}\n\
             設定ファイルに平文のまま残っています。"
        ),
    }

    let terminal = ratatui::init();
    // 長いトークンを貼り付けられるようにする。1 文字ずつのキー入力として
    // 届くと取りこぼしやすいので、まとめて 1 イベントで受け取る。
    let paste_enabled =
        crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste).is_ok();
    let result = run(
        terminal,
        clients,
        settings,
        credentials.source,
        has_credentials,
        args,
    )
    .await;
    if paste_enabled {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    }
    ratatui::restore();
    result
}

/// 各サービスの API クライアント一式。
pub struct Clients {
    pub sacloud: Arc<SacloudClient>,
    pub apprun: Arc<apprun::AppRunClient>,
    pub dedicated: Arc<apprun_dedicated::DedicatedClient>,
    pub monitoring: Arc<monitoring::MonitoringClient>,
}

impl Clients {
    fn new(credentials: &ApiCredentials) -> Result<Self> {
        Ok(Self {
            sacloud: Arc::new(SacloudClient::new(credentials)?),
            apprun: Arc::new(apprun::AppRunClient::new(credentials)?),
            dedicated: Arc::new(apprun_dedicated::DedicatedClient::new(credentials)?),
            monitoring: Arc::new(monitoring::MonitoringClient::new(credentials)?),
        })
    }
}

async fn run(
    mut terminal: ratatui::DefaultTerminal,
    clients: Clients,
    settings: config::Config,
    credential_source: config::CredentialSource,
    has_credentials: bool,
    args: Args,
) -> Result<()> {
    let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = App::new(
        clients,
        app::Tx::new(sender),
        settings,
        credential_source,
        has_credentials,
    );
    if let Some(zone) = args.zone {
        app.zone = zone;
    }
    if let Some(service) = args.service {
        app.service = service;
        if has_credentials {
            app.ensure_loaded();
        }
    } else if has_credentials {
        app.open_initial_service_picker();
    }
    if !has_credentials {
        app.start_credential_setup();
    }
    let mut events = EventStream::new();
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    terminal.draw(|frame| ui::draw(frame, &mut app))?;

    while !app.should_quit {
        // 入力・非同期処理の完了・スピナー更新のいずれかで再描画する。
        let redraw = tokio::select! {
            Some(event) = events.next() => match event? {
                // Windows では離した瞬間にもイベントが来るので押下だけ拾う。
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.on_key(key);
                    true
                }
                Event::Paste(text) => {
                    app.on_paste(&text);
                    true
                }
                Event::Resize(_, _) => true,
                _ => false,
            },
            Some((epoch, message)) = rx.recv() => {
                app.on_message(epoch, message);
                true
            }
            _ = ticker.tick() => {
                app.tick = app.tick.wrapping_add(1);
                // 通信中だけスピナーのために再描画する。
                app.inflight > 0
            }
        };

        if redraw {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
        }
    }
    Ok(())
}
