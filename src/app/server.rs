//! サーバー画面の状態と操作。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{
    App, ConfirmAction, Loadable, Message, NicChoice, Overlay, Pane, ServerChoices,
    ServerCreateForm, ServerPlanForm, SshKeyReturn, SshKeySource, SshKeyStage, StatusKind,
    fmt_error, matches,
};
use crate::cloud_resources::CloudResource;
use crate::iaas::{
    DiskPlan, NicPlan, OS_CHOICES, PowerAction, PowerStatus, Server, ServerCreateInput, ServerPlan,
    SshKey, StartupScript,
};
use crate::pubkey::PublicKey;
use crate::switch::Switch;

/// スイッチに繋ぐ NIC の指定。IP が無ければ作らせない。
fn switch_nic(id: crate::sacloud::ResourceId, form: &ServerCreateForm) -> Option<NicPlan> {
    let ip_address = form.ip_address.trim().to_string();
    let mask_len: u32 = form.mask_len.trim().parse().ok()?;
    if ip_address.is_empty() || !(1..=32).contains(&mask_len) {
        return None;
    }
    Some(NicPlan::Switch {
        id,
        ip_address,
        mask_len,
        gateway: form.gateway.trim().to_string(),
    })
}

/// 確認に出す NIC の説明。
fn nic_summary(plan: &NicPlan, choice: &NicChoice) -> String {
    match plan {
        NicPlan::Switch {
            ip_address,
            mask_len,
            ..
        } => format!("{} ({ip_address}/{mask_len})", choice.label()),
        _ => choice.label(),
    }
}

/// 登録済みの鍵を一覧に出せる形にする。
///
/// 名前は空にできるので、その場合は見分けられるよう指紋を使う。
fn sacloud_key_choice(key: SshKey) -> PublicKey {
    let label = if key.name.trim().is_empty() {
        key.fingerprint.clone()
    } else {
        key.name.clone()
    };
    PublicKey {
        label,
        key: key.public_key,
    }
}

#[derive(Debug, Default)]
pub struct ServerView {
    /// ゾーンごとのサーバー一覧。
    pub servers: HashMap<String, Loadable<Vec<Server>>>,
    pub server_state: TableState,
    /// 作成フォームで使う選択肢。ゾーンごとに違うので都度引く。
    pub plans: Loadable<Vec<ServerPlan>>,
    pub disk_plans: Loadable<Vec<DiskPlan>>,
    pub switches: Loadable<Vec<Switch>>,
    pub packet_filters: Loadable<Vec<CloudResource>>,
    pub startup_scripts: Loadable<Vec<StartupScript>>,
}

impl ServerView {
    /// 作成フォームで選べるディスクサイズ（MB）。SSD を既定にする。
    pub fn disk_sizes(&self) -> Vec<u32> {
        self.disk_plans
            .ready()
            .and_then(|plans| plans.iter().find(|p| p.is_ssd()))
            .map(|p| p.sizes_mb.clone())
            .unwrap_or_default()
    }

    /// SSD のプラン ID。まだ引けていなければ公式SDKと同じ既定値。
    pub fn ssd_plan_id(&self) -> u32 {
        self.disk_plans
            .ready()
            .and_then(|plans| plans.iter().find(|p| p.is_ssd()))
            .map(|p| p.id)
            .unwrap_or(4)
    }
}

impl App {
    pub fn visible_servers(&self) -> Loadable<Vec<Server>> {
        let loadable = self
            .server
            .servers
            .get(&self.zone)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(servers) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::Servers);
        Loadable::Ready(
            servers
                .into_iter()
                .filter(|s| {
                    let ips = s.ip_addresses.join(" ");
                    matches(filter, &[&s.name, &s.host_name, &ips, &s.plan_name])
                })
                .collect(),
        )
    }

    pub fn selected_server(&self) -> Option<Server> {
        let index = self.server.server_state.selected()?;
        self.visible_servers().ready()?.get(index).cloned()
    }

    pub(super) fn load_servers(&mut self, zone: String) {
        self.server.servers.insert(zone.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_servers(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::Servers { zone, result });
        });
    }

    pub(super) fn server_ensure_loaded(&mut self) {
        let zone = self.zone.clone();
        if self.server.servers.get(&zone).is_none_or(Loadable::is_idle) {
            self.load_servers(zone);
            return;
        }
        if !self
            .visible_servers()
            .ready()
            .is_none_or(|servers| servers.is_empty())
            && self.server.server_state.selected().is_none()
        {
            self.server.server_state.select(Some(0));
        }
    }

    pub(super) fn server_refresh(&mut self) {
        let zone = self.zone.clone();
        self.load_servers(zone);
    }

    /// 作成フォームで選べるプランの一覧。まだ届いていなければ空。
    pub fn server_plan_choices(&self) -> Vec<ServerPlan> {
        self.server.plans.ready().cloned().unwrap_or_default()
    }

    /// 作成フォームで選べるもの一式。届いていないものは空になる。
    pub fn server_choices(&self) -> ServerChoices {
        // 共有セグメントを先頭、接続しないを末尾に置き、間にスイッチを並べる。
        let mut nics = vec![NicChoice::Shared];
        nics.extend(
            self.server
                .switches
                .ready()
                .into_iter()
                .flatten()
                .map(|s| NicChoice::Switch(s.id, s.name.clone())),
        );
        nics.push(NicChoice::None);

        // 「なし」を先頭に置く。
        let mut packet_filters = vec![None];
        packet_filters.extend(
            self.server
                .packet_filters
                .ready()
                .into_iter()
                .flatten()
                .map(|f| Some((f.id, f.name.clone()))),
        );
        let mut startup_scripts = vec![None];
        startup_scripts.extend(
            self.server
                .startup_scripts
                .ready()
                .into_iter()
                .flatten()
                .cloned()
                .map(Some),
        );

        ServerChoices {
            plans: self.server_plan_choices(),
            disk_sizes: self.server.disk_sizes(),
            nics,
            packet_filters,
            startup_scripts,
        }
    }

    fn open_server_create_form(&mut self) {
        if !self.require_write() {
            return;
        }
        // 選択肢はゾーンごとに違う。開くたびに引き直す。
        self.load_server_plans();
        self.load_server_attachments();
        let mut form = ServerCreateForm {
            boot_after_create: true,
            ..ServerCreateForm::default()
        };
        // もう一覧が手元にあれば今すぐ埋める。無ければ届いた時に埋める。
        form.apply_defaults(&self.server_plan_choices(), &self.server.disk_sizes());
        self.overlay = Some(Overlay::ServerCreateForm(form));
    }

    /// プラン一覧が届いたときに、開いたままのフォームの既定値を埋める。
    pub(super) fn server_plans_arrived(&mut self) {
        let plans = self.server_plan_choices();
        let sizes = self.server.disk_sizes();
        match &mut self.overlay {
            Some(Overlay::ServerCreateForm(form)) => form.apply_defaults(&plans, &sizes),
            Some(Overlay::SshKeyPicker { back, .. }) => {
                if let SshKeyReturn::ServerCreate(form) = back.as_mut() {
                    form.apply_defaults(&plans, &sizes);
                }
            }
            _ => {}
        }
    }

    fn load_server_plans(&mut self) {
        if !self.server.plans.is_idle() && !self.server.disk_plans.is_idle() {
            return;
        }
        self.server.plans = Loadable::Loading;
        self.server.disk_plans = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let plans = client.list_server_plans(&zone).await.map_err(fmt_error);
            let disks = client.list_disk_plans(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::ServerPlans { plans, disks });
        });
    }

    /// NIC の繋ぎ先・パケットフィルタ・スタートアップスクリプトの一覧を引く。
    ///
    /// どれも作成フォームでしか使わないので、フォームを開くときにまとめて引く。
    fn load_server_attachments(&mut self) {
        if !self.server.switches.is_idle() {
            return;
        }
        self.server.switches = Loadable::Loading;
        self.server.packet_filters = Loadable::Loading;
        self.server.startup_scripts = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let switches = client.list_switches(&zone).await.map_err(fmt_error);
            let filters = client
                .list_cloud_resources(
                    &zone,
                    crate::cloud_resources::CloudResourceKind::PacketFilter,
                )
                .await
                .map_err(fmt_error);
            let scripts = client.list_startup_scripts(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::ServerAttachments {
                switches,
                filters,
                scripts,
            });
        });
    }

    /// 公開鍵を選ぶ画面を開く。呼び出し元のフォームは預かって、選び終えたら戻す。
    pub(super) fn open_ssh_key_picker(&mut self, back: SshKeyReturn) {
        self.overlay = Some(Overlay::SshKeyPicker {
            back: Box::new(back),
            stage: SshKeyStage::Source { index: 0 },
        });
    }

    /// 取得元が決まったので鍵を集める。
    pub(super) fn choose_ssh_key_source(&mut self, back: Box<SshKeyReturn>, source: SshKeySource) {
        match source {
            // ユーザー名を聞かないと取りに行けない。
            SshKeySource::Github => {
                self.overlay = Some(Overlay::SshKeyPicker {
                    back,
                    stage: SshKeyStage::GithubUser {
                        user: String::new(),
                    },
                });
            }
            // 手元のファイルなので待たせる必要がない。
            SshKeySource::Local => match crate::pubkey::from_local_ssh_dir() {
                Ok(keys) => self.show_ssh_keys(back, source.label().to_string(), keys),
                Err(err) => self.fail_ssh_key_picker(*back, fmt_error(err)),
            },
            SshKeySource::Sacloud => {
                let from = source.label().to_string();
                self.overlay = Some(Overlay::SshKeyPicker {
                    back,
                    stage: SshKeyStage::Loading { from: from.clone() },
                });
                self.inflight += 1;
                let client = self.sacloud.clone();
                let tx = self.tx.clone();
                let zone = self.zone.clone();
                tokio::spawn(async move {
                    let result = client
                        .list_ssh_keys(&zone)
                        .await
                        .map(|keys| keys.into_iter().map(sacloud_key_choice).collect())
                        .map_err(fmt_error);
                    let _ = tx.send(Message::SshKeys { from, result });
                });
            }
        }
    }

    /// GitHub のユーザー名が決まったので取りに行く。
    pub(super) fn submit_github_ssh_user(&mut self, back: Box<SshKeyReturn>, user: String) {
        let from = format!("GitHub: {}", user.trim());
        self.overlay = Some(Overlay::SshKeyPicker {
            back,
            stage: SshKeyStage::Loading { from: from.clone() },
        });
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = crate::pubkey::from_github(&user).await.map_err(fmt_error);
            let _ = tx.send(Message::SshKeys { from, result });
        });
    }

    pub(super) fn ssh_keys_arrived(
        &mut self,
        from: String,
        result: Result<Vec<PublicKey>, String>,
    ) {
        // 画面を閉じたあとに届くことがある。その場合は捨てる。
        let Some(Overlay::SshKeyPicker { back, stage }) = self.overlay.take() else {
            return;
        };
        // 待っているものと別の応答なら、今の画面をそのまま続ける。
        if !matches!(&stage, SshKeyStage::Loading { from: waiting } if *waiting == from) {
            self.overlay = Some(Overlay::SshKeyPicker { back, stage });
            return;
        }
        match result {
            Ok(keys) => self.show_ssh_keys(back, from, keys),
            Err(err) => self.fail_ssh_key_picker(*back, err),
        }
    }

    fn show_ssh_keys(&mut self, back: Box<SshKeyReturn>, from: String, keys: Vec<PublicKey>) {
        if keys.is_empty() {
            self.fail_ssh_key_picker(*back, format!("{from}: 公開鍵が見つかりませんでした"));
            return;
        }
        self.overlay = Some(Overlay::SshKeyPicker {
            back,
            stage: SshKeyStage::Keys {
                from,
                keys,
                index: 0,
            },
        });
    }

    /// 鍵を諦めて呼び出し元のフォームに戻す。
    fn fail_ssh_key_picker(&mut self, back: SshKeyReturn, err: String) {
        self.close_ssh_key_picker(back);
        self.set_status(err, StatusKind::Error);
    }

    /// 呼び出し元のフォームに戻す。
    pub(super) fn close_ssh_key_picker(&mut self, back: SshKeyReturn) {
        self.overlay = Some(match back {
            SshKeyReturn::ServerCreate(form) => Overlay::ServerCreateForm(form),
            SshKeyReturn::Register(form) => Overlay::SshKeyForm(form),
        });
    }

    /// 選んだ鍵をフォームに入れて戻す。
    pub(super) fn take_ssh_key(&mut self, mut back: SshKeyReturn, key: &PublicKey) {
        back.take_key(key);
        let label = key.label.clone();
        self.close_ssh_key_picker(back);
        self.set_status(
            format!("公開鍵「{label}」を入れました"),
            StatusKind::Success,
        );
    }

    pub(super) fn submit_server_create_form(&mut self, form: ServerCreateForm) {
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.overlay = Some(Overlay::ServerCreateForm(form));
            self.set_status("名前を入力してください", StatusKind::Error);
            return;
        }
        let plans = self.server_plan_choices();
        if !crate::iaas::plan_exists(&plans, form.cpu, form.memory_mb) {
            self.overlay = Some(Overlay::ServerCreateForm(form));
            self.set_status(
                "その CPU とメモリの組み合わせは選べません",
                StatusKind::Error,
            );
            return;
        }
        let (cpu, memory_mb) = (form.cpu, form.memory_mb);
        let sizes = self.server.disk_sizes();
        if !sizes.contains(&form.disk_size_mb) {
            self.overlay = Some(Overlay::ServerCreateForm(form));
            self.set_status("ディスクサイズを選んでください", StatusKind::Error);
            return;
        }
        let disk_size_mb = form.disk_size_mb;
        let os = OS_CHOICES[form.os.min(OS_CHOICES.len() - 1)];

        let choices = self.server_choices();
        let nic = match choices.nic(form.nic) {
            NicChoice::Shared => NicPlan::Shared,
            NicChoice::None => NicPlan::None,
            NicChoice::Switch(id, _) => {
                // スイッチには DHCP が無いので、IP が無いと OS から通信できない。
                let Some(plan) = switch_nic(id, &form) else {
                    self.overlay = Some(Overlay::ServerCreateForm(form));
                    self.set_status(
                        "スイッチに繋ぐときは IPアドレスとマスク長を入れてください",
                        StatusKind::Error,
                    );
                    return;
                };
                plan
            }
        };
        let filter = choices.packet_filter(form.packet_filter);
        let script = choices.startup_script(form.startup_script);

        let input = ServerCreateInput {
            name: name.clone(),
            description: form.description.trim().to_string(),
            cpu,
            memory_mb,
            os_tags: os.tags.iter().map(|t| t.to_string()).collect(),
            disk_size_mb,
            disk_plan_id: self.server.ssd_plan_id(),
            host_name: form.effective_host_name().to_string(),
            password: form.password.clone(),
            ssh_public_key: form.ssh_public_key.trim().to_string(),
            disable_password_auth: !form.ssh_public_key.trim().is_empty(),
            nic: nic.clone(),
            packet_filter_id: filter.as_ref().map(|(id, _)| *id),
            startup_script_id: script.as_ref().map(|s| s.id),
            boot_after_create: form.boot_after_create,
        };

        // ディスクは作成した時点から課金される。実行前に一度止める。
        self.overlay = Some(Overlay::Confirm {
            title: "サーバーの作成".to_string(),
            body: format!(
                "サーバー「{}」を {} に作成します。\n\
                 {} コア / {} GB / {} / ディスク {} GB（{}）\n\
                 NIC: {}{}{}\n\n\
                 ディスクは作成した時点から、サーバーは起動した時点から課金されます。",
                name,
                self.zone,
                cpu,
                memory_mb / 1024,
                os.label,
                disk_size_mb / 1024,
                if form.boot_after_create {
                    "作成後に起動する"
                } else {
                    "作成後は停止のまま"
                },
                nic_summary(&nic, &choices.nic(form.nic)),
                filter
                    .map(|(_, n)| format!("　フィルタ: {n}"))
                    .unwrap_or_default(),
                script
                    .map(|s| format!("　スクリプト: {}", s.name))
                    .unwrap_or_default(),
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::CreateServer {
                zone: self.zone.clone(),
                input: Box::new(input),
            },
        });
    }

    pub(super) fn run_create_server(&mut self, zone: String, input: Box<ServerCreateInput>) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let name = input.name.clone();
        self.set_status(
            format!("サーバー「{name}」を作成しています…"),
            StatusKind::Info,
        );
        tokio::spawn(async move {
            let (progress, result) = client.create_server(&zone, &input).await;
            let _ = tx.send(Message::ServerCreated {
                name,
                progress,
                result: result.map(|_| ()).map_err(fmt_error),
            });
        });
    }

    fn confirm_delete_server(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(server) = self.selected_server() else {
            return;
        };
        // 起動中は API 側で拒否されるので、叩く前に止める。
        if server.power != PowerStatus::Down {
            self.set_status(
                format!(
                    "{}: 起動中は削除できません。先に停止してください",
                    server.name
                ),
                StatusKind::Error,
            );
            return;
        }
        let disks = if server.disk_names.is_empty() {
            String::new()
        } else {
            format!(
                "\n接続中のディスク({})も一緒に削除します。",
                server.disk_names.join(", ")
            )
        };
        self.overlay = Some(Overlay::Confirm {
            title: "サーバーの削除".to_string(),
            body: format!(
                "サーバー「{}」({}) を削除します。{}\n\
                 元に戻せません。実行するにはサーバー名を入力してください。",
                server.name, self.zone, disks
            ),
            verify: Some(server.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteServer {
                zone: self.zone.clone(),
                id: server.id,
                name: server.name,
            },
        });
    }

    pub(super) fn run_delete_server(
        &mut self,
        zone: String,
        id: crate::sacloud::ResourceId,
        name: String,
    ) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            // 接続中のディスクは単体では消せないので、まとめて指定する。
            let result = match client.server_disk_ids(&zone, id).await {
                Ok(disks) => client.delete_server(&zone, id, &disks).await,
                Err(err) => Err(err),
            };
            let _ = tx.send(Message::ServerDeleted {
                name,
                result: result.map_err(fmt_error),
            });
        });
    }

    /// プラン変更フォームを開く。
    fn open_server_plan_form(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(server) = self.selected_server() else {
            return;
        };
        // 起動中は API 側で拒否されるので、叩く前に止める。
        if server.power != PowerStatus::Down {
            self.set_status(
                format!(
                    "{}: 起動中はプランを変更できません。先に停止してください",
                    server.name
                ),
                StatusKind::Error,
            );
            return;
        }
        // 選べる値はゾーンごとに違う。開くたびに引き直す。
        self.load_server_plans();
        self.overlay = Some(Overlay::ServerPlanForm(ServerPlanForm {
            server_id: server.id,
            server_name: server.name,
            original_cpu: server.cpu,
            original_memory_mb: server.memory_mb,
            cpu: server.cpu,
            memory_mb: server.memory_mb,
            field: 0,
        }));
    }

    pub(super) fn submit_server_plan_form(&mut self, form: ServerPlanForm) {
        if form.is_unchanged() {
            self.overlay = Some(Overlay::ServerPlanForm(form));
            self.set_status("今と同じ構成です", StatusKind::Error);
            return;
        }
        let plans = self.server_plan_choices();
        if !crate::iaas::plan_exists(&plans, form.cpu, form.memory_mb) {
            self.overlay = Some(Overlay::ServerPlanForm(form));
            self.set_status(
                "その CPU とメモリの組み合わせは選べません",
                StatusKind::Error,
            );
            return;
        }
        // ID が変わるので、外から ID で参照している設定があれば直す必要がある。
        self.overlay = Some(Overlay::Confirm {
            title: "プランの変更".to_string(),
            body: format!(
                "サーバー「{}」({}) のプランを変更します。\n\
                 {} コア / {} GB → {} コア / {} GB\n\n\
                 ディスクと NIC はそのまま引き継がれますが、\
                 サーバーの ID が変わります。",
                form.server_name,
                self.zone,
                form.original_cpu,
                form.original_memory_mb / 1024,
                form.cpu,
                form.memory_mb / 1024,
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::ChangeServerPlan {
                zone: self.zone.clone(),
                id: form.server_id,
                name: form.server_name,
                cpu: form.cpu,
                memory_mb: form.memory_mb,
            },
        });
    }

    pub(super) fn run_change_server_plan(
        &mut self,
        zone: String,
        id: crate::sacloud::ResourceId,
        name: String,
        cpu: u32,
        memory_mb: u32,
    ) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.set_status(
            format!("サーバー「{name}」のプランを変更しています…"),
            StatusKind::Info,
        );
        tokio::spawn(async move {
            let result = client.change_server_plan(&zone, id, cpu, memory_mb).await;
            let _ = tx.send(Message::ServerPlanChanged {
                name,
                result: result.map(|_| ()).map_err(fmt_error),
            });
        });
    }

    pub(super) fn on_key_server(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => self.open_server_create_form(),
            KeyCode::Char('D') => self.confirm_delete_server(),
            KeyCode::Char('P') => self.open_server_plan_form(),
            KeyCode::Char('b') => self.confirm_power(PowerAction::Boot),
            KeyCode::Char('x') => self.confirm_power(PowerAction::Shutdown),
            KeyCode::Char('X') => self.confirm_power(PowerAction::PowerOff),
            KeyCode::Char('B') => self.confirm_power(PowerAction::Reset),
            _ => {}
        }
    }

    fn confirm_power(&mut self, action: PowerAction) {
        if !self.require_write() {
            return;
        }
        let Some(server) = self.selected_server() else {
            return;
        };

        // 現在の電源状態と噛み合わない操作は、API を叩く前に止める。
        let mismatch = match (action, server.power) {
            (PowerAction::Boot, PowerStatus::Up) => Some("すでに起動しています"),
            (PowerAction::Shutdown | PowerAction::PowerOff, PowerStatus::Down) => {
                Some("すでに停止しています")
            }
            (PowerAction::Reset, PowerStatus::Down) => Some("停止中のため再起動できません"),
            _ => None,
        };
        if let Some(reason) = mismatch {
            self.set_status(format!("{}: {reason}", server.name), StatusKind::Info);
            return;
        }

        // 強制停止・強制リセットはデータを失いうるのでサーバー名の入力を求める。
        let verify = action.is_risky().then(|| server.name.clone());
        self.overlay = Some(Overlay::Confirm {
            title: format!("サーバーの{}", action.label()),
            body: format!(
                "サーバー「{}」({}) を{}します。\n{}{}",
                server.name,
                self.zone,
                action.label(),
                action.description(),
                if verify.is_some() {
                    "\n実行するにはサーバー名を入力してください。"
                } else {
                    ""
                }
            ),
            verify,
            typed: String::new(),
            action: ConfirmAction::PowerAction {
                id: server.id,
                zone: self.zone.clone(),
                name: server.name,
                action,
            },
        });
    }

    pub(super) fn run_power_action(
        &mut self,
        id: crate::sacloud::ResourceId,
        zone: String,
        name: String,
        action: PowerAction,
    ) {
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let label = format!("サーバー「{name}」を{}", action.label());
        self.inflight += 1;
        let target_zone = zone.clone();
        tokio::spawn(async move {
            let result = client
                .power_action(&target_zone, id, action)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::ServerAction {
                zone,
                label,
                result,
            });
        });
        self.set_status("送信中…", StatusKind::Info);
    }

    pub(super) fn server_invalidate(&mut self) {
        self.server = ServerView::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ホスト名は省略時にサーバー名を使う。
    #[test]
    fn host_name_falls_back_to_the_server_name() {
        let form = ServerCreateForm {
            name: "web-01".to_string(),
            ..ServerCreateForm::default()
        };
        assert_eq!(form.effective_host_name(), "web-01");

        let explicit = ServerCreateForm {
            name: "web-01".to_string(),
            host_name: "custom".to_string(),
            ..ServerCreateForm::default()
        };
        assert_eq!(explicit.effective_host_name(), "custom");
    }

    /// 選択式の欄と入力式の欄を取り違えないこと。
    /// 取り違えると左右キーで文字が消えたり、入力できなくなる。
    #[test]
    fn choice_fields_are_separated_from_text_fields() {
        use crate::app::ServerField as F;
        for field in [F::Cpu, F::Memory, F::Os, F::DiskSize, F::Nic, F::Boot] {
            assert!(field.is_choice(), "{field:?} は選択式のはず");
            // 選択式の欄は文字列を持たない。
            assert_eq!(ServerCreateForm::default().value(field), "");
        }
        for field in [
            F::Name,
            F::Description,
            F::IpAddress,
            F::MaskLen,
            F::Gateway,
            F::HostName,
            F::Password,
            F::SshKey,
        ] {
            assert!(!field.is_choice(), "{field:?} は入力式のはず");
        }
    }

    /// プラン一覧が届いたら、まだ選んでいない欄が埋まること。
    #[test]
    fn defaults_are_filled_in_when_the_plan_list_arrives() {
        let plans = vec![
            ServerPlan {
                name: "1c1g".to_string(),
                cpu: 1,
                memory_mb: 1024,
                commitment: "standard".to_string(),
                generation: 200,
                availability: "available".to_string(),
            },
            ServerPlan {
                name: "2c4g".to_string(),
                cpu: 2,
                memory_mb: 4096,
                commitment: "standard".to_string(),
                generation: 200,
                availability: "available".to_string(),
            },
        ];
        let mut form = ServerCreateForm::default();
        form.apply_defaults(&plans, &[20480, 40960]);
        assert_eq!((form.cpu, form.memory_mb), (1, 1024));
        assert_eq!(form.disk_size_mb, 20480);

        // 既に選んでいるものは上書きしない。
        let mut chosen = ServerCreateForm {
            cpu: 2,
            memory_mb: 4096,
            disk_size_mb: 40960,
            ..ServerCreateForm::default()
        };
        chosen.apply_defaults(&plans, &[20480, 40960]);
        assert_eq!(
            (chosen.cpu, chosen.memory_mb, chosen.disk_size_mb),
            (2, 4096, 40960)
        );
    }
}
