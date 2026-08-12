//! さくらインターネットのサービスをターミナルから操作する TUI。
//!
//! 現時点ではさくらのクラウドのコンテナレジストリに対応している。

mod app;
mod apprun;
mod apprun_dedicated;
mod config;
mod iaas;
mod registry;
mod sacloud;
mod ui;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyEventKind};
use futures::StreamExt;

use crate::app::App;
use crate::sacloud::SacloudClient;

/// スピナーを回すための描画間隔。
const TICK: Duration = Duration::from_millis(120);

#[tokio::main]
async fn main() -> Result<()> {
    // 認証情報の不足は TUI に入る前にプレーンなエラーとして出す。
    let credentials = match config::load_api_credentials() {
        Ok(credentials) => credentials,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let sacloud = Arc::new(SacloudClient::new(&credentials)?);
    let apprun_client = Arc::new(apprun::AppRunClient::new(&credentials)?);
    let dedicated_client = Arc::new(apprun_dedicated::DedicatedClient::new(&credentials)?);
    let settings = match config::Config::load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("警告: 設定ファイルを読めませんでした: {err}");
            config::Config::default()
        }
    };

    let terminal = ratatui::init();
    let result = run(terminal, sacloud, apprun_client, dedicated_client, settings, credentials.source).await;
    ratatui::restore();
    result
}

async fn run(
    mut terminal: ratatui::DefaultTerminal,
    sacloud: Arc<SacloudClient>,
    apprun_client: Arc<apprun::AppRunClient>,
    dedicated_client: Arc<apprun_dedicated::DedicatedClient>,
    settings: config::Config,
    credential_source: config::CredentialSource,
) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = App::new(sacloud, apprun_client, dedicated_client, tx, settings, credential_source);
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
