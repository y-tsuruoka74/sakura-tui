//! ディスク画面の書き込み操作。
//!
//! 一覧と詳細は `src/cloud_resources.rs` の共通ブラウザが担当する。
//! ここは作成・削除・サーバーへの接続と切断だけを持つ。

use crossterm::event::{KeyCode, KeyEvent};

use super::{
    App, ConfirmAction, DiskCreateForm, DiskServerPicker, Loadable, Message, Overlay, StatusKind,
    fmt_error,
};
use crate::cloud_resources::CloudResource;
use crate::iaas::{DiskCreateInput, DiskPlan, PowerStatus};
use crate::sacloud::ResourceId;

#[derive(Debug, Default)]
pub struct DiskView {
    /// 作成フォームで使う選択肢。ゾーンごとに違うので都度引く。
    pub plans: Loadable<Vec<DiskPlan>>,
}

impl App {
    pub fn disk_plan_choices(&self) -> Vec<DiskPlan> {
        self.disk.plans.ready().cloned().unwrap_or_default()
    }

    pub(super) fn on_key_disk(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => self.open_disk_create_form(),
            KeyCode::Char('D') => self.confirm_delete_disk(),
            KeyCode::Char('c') => self.open_disk_server_picker(),
            KeyCode::Char('C') => self.confirm_disconnect_disk(),
            _ => {}
        }
    }

    fn load_disk_plans(&mut self) {
        if !self.disk.plans.is_idle() {
            return;
        }
        self.disk.plans = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client.list_disk_plans(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::DiskPlans { result });
        });
    }

    fn open_disk_create_form(&mut self) {
        if !self.require_write() {
            return;
        }
        // 選択肢はゾーンごとに違う。開くたびに引き直す。
        self.load_disk_plans();
        let mut form = DiskCreateForm::default();
        form.apply_defaults(&self.disk_plan_choices());
        self.overlay = Some(Overlay::DiskCreateForm(form));
    }

    /// プラン一覧が届いたときに、開いたままのフォームの既定値を埋める。
    pub(super) fn disk_plans_arrived(&mut self) {
        let plans = self.disk_plan_choices();
        if let Some(Overlay::DiskCreateForm(form)) = &mut self.overlay {
            form.apply_defaults(&plans);
        }
    }

    pub(super) fn submit_disk_create_form(&mut self, form: DiskCreateForm) {
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.overlay = Some(Overlay::DiskCreateForm(form));
            self.set_status("名前を入力してください", StatusKind::Error);
            return;
        }
        let plans = self.disk_plan_choices();
        let sizes = crate::app::forms::sizes_of(&plans, form.plan_id);
        if !sizes.contains(&form.size_mb) {
            self.overlay = Some(Overlay::DiskCreateForm(form));
            self.set_status("サイズを選んでください", StatusKind::Error);
            return;
        }
        let plan_name = plans
            .iter()
            .find(|p| p.id == form.plan_id)
            .map_or_else(String::new, |p| p.name.clone());

        let input = DiskCreateInput {
            name: name.clone(),
            description: form.description.trim().to_string(),
            plan_id: form.plan_id,
            size_mb: form.size_mb,
            os_tags: form.os_tags(),
        };

        // ディスクは作成した時点から課金される。実行前に一度止める。
        self.overlay = Some(Overlay::Confirm {
            title: "ディスクの作成".to_string(),
            body: format!(
                "ディスク「{}」を {} に作成します。\n\
                 {} / {} GB / {}\n\n\
                 ディスクは作成した時点から課金されます。",
                name,
                self.zone,
                plan_name,
                form.size_mb / 1024,
                form.source_label(),
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::CreateDisk {
                zone: self.zone.clone(),
                input: Box::new(input),
            },
        });
    }

    pub(super) fn run_create_disk(&mut self, zone: String, input: Box<DiskCreateInput>) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let name = input.name.clone();
        // OS テンプレートを指定するとコピーが走るので、待ち時間があることを伝える。
        let copying = !input.os_tags.is_empty();
        self.set_status(
            format!("ディスク「{name}」を作成しています…"),
            StatusKind::Info,
        );
        tokio::spawn(async move {
            let result = client.create_disk(&zone, &input).await;
            let _ = tx.send(Message::DiskCreated {
                name,
                copying,
                result: result.map(|_| ()).map_err(fmt_error),
            });
        });
    }

    fn confirm_delete_disk(&mut self) {
        let Some(disk) = self.writable_disk() else {
            return;
        };
        // 繋がったままでは消せない。先に切断させる。
        if let Some((_, server)) = &disk.attached_server {
            self.set_status(
                format!(
                    "{}: サーバー「{server}」に接続中です。先に C で切断してください",
                    disk.name
                ),
                StatusKind::Error,
            );
            return;
        }
        self.overlay = Some(Overlay::Confirm {
            title: "ディスクの削除".to_string(),
            body: format!(
                "ディスク「{}」({}) を削除します。\n\
                 中のデータごと消え、元に戻せません。実行するにはディスク名を入力してください。",
                disk.name, self.zone
            ),
            verify: Some(disk.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteDisk {
                zone: self.zone.clone(),
                id: disk.id,
                name: disk.name,
            },
        });
    }

    pub(super) fn run_delete_disk(&mut self, zone: String, id: ResourceId, name: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.delete_disk(&zone, id).await;
            let _ = tx.send(Message::DiskChanged {
                what: format!("ディスク「{name}」を削除しました"),
                failed: "ディスクの削除に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    /// 接続先のサーバーを選ぶ画面を開く。
    fn open_disk_server_picker(&mut self) {
        let Some(disk) = self.writable_disk() else {
            return;
        };
        if let Some((_, server)) = &disk.attached_server {
            self.set_status(
                format!(
                    "{}: すでにサーバー「{server}」に接続されています",
                    disk.name
                ),
                StatusKind::Error,
            );
            return;
        }
        self.overlay = Some(Overlay::DiskServerPicker(DiskServerPicker {
            disk_id: disk.id,
            disk_name: disk.name,
            servers: Loadable::Loading,
            index: 0,
        }));
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            // 接続できるのは停止中のサーバーだけ。
            let result = client
                .list_servers(&zone)
                .await
                .map(|servers| {
                    servers
                        .into_iter()
                        .filter(|s| s.power == PowerStatus::Down)
                        .map(|s| (s.id, s.name))
                        .collect()
                })
                .map_err(fmt_error);
            let _ = tx.send(Message::DiskTargetServers { result });
        });
    }

    pub(super) fn disk_target_servers_arrived(
        &mut self,
        result: Result<Vec<(ResourceId, String)>, String>,
    ) {
        // 画面を閉じたあとに届くことがある。その場合は捨てる。
        if !matches!(self.overlay, Some(Overlay::DiskServerPicker(_))) {
            return;
        }
        let servers = self.store_result(result);
        if let Some(Overlay::DiskServerPicker(picker)) = &mut self.overlay {
            picker.servers = servers;
        }
    }

    pub(super) fn submit_disk_server_picker(&mut self, picker: DiskServerPicker) {
        let Some(servers) = picker.servers.ready() else {
            self.overlay = Some(Overlay::DiskServerPicker(picker));
            return;
        };
        let Some((server_id, server_name)) = servers.get(picker.index).cloned() else {
            self.overlay = Some(Overlay::DiskServerPicker(picker));
            self.set_status("接続できるサーバーがありません", StatusKind::Error);
            return;
        };
        self.overlay = None;
        self.run_connect_disk(picker.disk_id, picker.disk_name, server_id, server_name);
    }

    fn run_connect_disk(
        &mut self,
        disk_id: ResourceId,
        disk_name: String,
        server_id: ResourceId,
        server_name: String,
    ) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client.connect_disk(&zone, disk_id, server_id).await;
            let _ = tx.send(Message::DiskChanged {
                what: format!("ディスク「{disk_name}」をサーバー「{server_name}」に接続しました"),
                failed: "ディスクの接続に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    fn confirm_disconnect_disk(&mut self) {
        let Some(disk) = self.writable_disk() else {
            return;
        };
        let Some((_, server)) = disk.attached_server.clone() else {
            self.set_status(
                format!("{}: どのサーバーにも接続されていません", disk.name),
                StatusKind::Error,
            );
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "ディスクの切断".to_string(),
            body: format!(
                "ディスク「{}」をサーバー「{server}」から切断します。\n\n\
                 起動中のサーバーからは切断できません。ディスク自体は残ります。",
                disk.name
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DisconnectDisk {
                zone: self.zone.clone(),
                id: disk.id,
                name: disk.name,
                server,
            },
        });
    }

    pub(super) fn run_disconnect_disk(
        &mut self,
        zone: String,
        id: ResourceId,
        name: String,
        server: String,
    ) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.disconnect_disk(&zone, id).await;
            let _ = tx.send(Message::DiskChanged {
                what: format!("ディスク「{name}」をサーバー「{server}」から切断しました"),
                failed: "ディスクの切断に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    /// 書き込みモードで、ディスクが選ばれていること。
    fn writable_disk(&mut self) -> Option<CloudResource> {
        if !self.require_write() {
            return None;
        }
        self.selected_cloud_resource()
    }
}
