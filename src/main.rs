//! さくらインターネットのサービスをターミナルから操作する TUI。
//!
//! 現時点ではさくらのクラウドのコンテナレジストリに対応している。

mod app;
mod apprun;
mod apprun_dedicated;
mod commonservice;
mod config;
mod http;
mod iaas;
mod monitoring;
mod registry;
mod sacloud;
mod secretmanager;
mod ui;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;

use crate::app::App;
use crate::sacloud::SacloudClient;

/// スピナーを回すための描画間隔。
const TICK: Duration = Duration::from_millis(120);

/// `--help` に出す使い方。
const USAGE: &str = "さくらインターネットのサービスをターミナルから操作する TUI

使い方:
  sakura-tui [オプション]

オプション:
  -p, --profile <名前>   使う usacloud プロファイル
  -z, --zone <ゾーン>     ゾーン依存のサービスで使うゾーン (例: is1a)
  -s, --service <名前>    起動時に開くサービス
                         registry / apprun / dedicated / server /
                         dns / monitor / secrets / monitoring
      --trace            APIリクエストを標準エラーに記録する
  -h, --help             このヘルプ
  -V, --version          バージョン

環境変数:
  SAKURA_ACCESS_TOKEN / SAKURA_ACCESS_TOKEN_SECRET   APIキー
  SAKURA_PROFILE                                     usacloud プロファイル名
  SAKURA_TUI_CONFIG                                  設定ファイルのパス
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
                println!("{USAGE}");
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

    // 認証情報の不足は TUI に入る前にプレーンなエラーとして出す。
    let credentials = match config::load_api_credentials(args.profile.is_some()) {
        Ok(credentials) => credentials,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let sacloud = Arc::new(SacloudClient::new(&credentials)?);
    let apprun_client = Arc::new(apprun::AppRunClient::new(&credentials)?);
    let dedicated_client = Arc::new(apprun_dedicated::DedicatedClient::new(&credentials)?);
    let monitoring_client = Arc::new(monitoring::MonitoringClient::new(&credentials)?);
    let settings = match config::Config::load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("警告: 設定ファイルを読めませんでした: {err}");
            config::Config::default()
        }
    };

    let terminal = ratatui::init();
    let result = run(
        terminal,
        Clients {
            sacloud,
            apprun: apprun_client,
            dedicated: dedicated_client,
            monitoring: monitoring_client,
        },
        settings,
        credentials.source,
        args,
    )
    .await;
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

async fn run(
    mut terminal: ratatui::DefaultTerminal,
    clients: Clients,
    settings: config::Config,
    credential_source: config::CredentialSource,
    args: Args,
) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = App::new(clients, tx, settings, credential_source);
    if let Some(zone) = args.zone {
        app.zone = zone;
    }
    if let Some(service) = args.service {
        app.service = service;
    }
    app.ensure_loaded();
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
                Event::Resize(_, _) => true,
                _ => false,
            },
            Some(message) = rx.recv() => {
                app.on_message(message);
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
