//! ディスク画面の書き込み操作。
//!
//! 一覧と詳細は `src/cloud_resources.rs` の共通ブラウザが担当する。
//! ここは作成・削除・サーバーへの接続と切断だけを持つ。

use crossterm::event::{KeyCode, KeyEvent};

use super::{
    App, ArchiveForm, AutoBackupForm, AutoBackupFormMode, ConfirmAction, DiskCreateForm,
    DiskServerPicker, Loadable, Message, Overlay, StatusKind, fmt_error,
};
use crate::cloud_resources::{CloudResource, CloudResourceKind};
use crate::commonservice::AutoBackupInput;
use crate::iaas::{DiskCreateInput, DiskPlan, OsTemplate, PowerStatus};
use crate::managed_resources::ManagedResource;
use crate::sacloud::ResourceId;

#[derive(Debug, Default)]
pub struct DiskView {
    /// 作成フォームで使う選択肢。ゾーンごとに違うので都度引く。
    pub plans: Loadable<Vec<DiskPlan>>,
    /// 作成元に選べる、自分で取ったアーカイブ。
    pub archives: Loadable<Vec<OsTemplate>>,
    /// アーカイブの元にできるディスク。
    pub sources: Loadable<Vec<(ResourceId, String)>>,
}

impl App {
    pub fn disk_plan_choices(&self) -> Vec<DiskPlan> {
        self.disk.plans.ready().cloned().unwrap_or_default()
    }

    /// 作成元に選べるアーカイブ。まだ届いていなければ空。
    pub fn disk_archive_choices(&self) -> Vec<OsTemplate> {
        self.disk.archives.ready().cloned().unwrap_or_default()
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
        self.disk.archives = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client.list_disk_plans(&zone).await.map_err(fmt_error);
            let archives = client.list_own_archives(&zone).await.map_err(fmt_error);
            let _ = tx.send(Message::DiskPlans { result, archives });
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
        let archives = self.disk_archive_choices();
        // 中身を選ぶ種類なのに選べていないなら、空のディスクを作ってしまう前に止める。
        let source_archive = form.source_archive(&archives);
        let os_tags = form.os_tags();
        if form.kind().needs_source() && os_tags.is_empty() && source_archive.is_none() {
            self.overlay = Some(Overlay::DiskCreateForm(form));
            self.set_status("元にするものを選んでください", StatusKind::Error);
            return;
        }
        let source_label = form.source_label(&archives);

        let input = DiskCreateInput {
            name: name.clone(),
            description: form.description.trim().to_string(),
            plan_id: form.plan_id,
            size_mb: form.size_mb,
            os_tags,
            source_archive,
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
                source_label,
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
        // 元にするものがあるとコピーが走るので、待ち時間があることを伝える。
        let copying = !input.os_tags.is_empty() || input.source_archive.is_some();
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

    // --- アーカイブ ---

    pub(super) fn on_key_archive(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => self.open_archive_form(),
            KeyCode::Char('D') => self.confirm_delete_archive(),
            _ => {}
        }
    }

    /// ディスクからアーカイブを取るフォームを開く。
    fn open_archive_form(&mut self) {
        if !self.require_write() {
            return;
        }
        self.load_archive_sources();
        self.overlay = Some(Overlay::ArchiveForm(ArchiveForm::default()));
    }

    /// 元にできるディスクの一覧を引く。
    fn load_archive_sources(&mut self) {
        if !self.disk.sources.is_idle() {
            return;
        }
        self.disk.sources = Loadable::Loading;
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = client
                .list_cloud_resources(&zone, CloudResourceKind::Disk)
                .await
                .map(|disks| disks.into_iter().map(|d| (d.id, d.name)).collect())
                .map_err(fmt_error);
            let _ = tx.send(Message::ArchiveSources { result });
        });
    }

    /// アーカイブの元にできるディスク。
    pub fn archive_source_choices(&self) -> Vec<(ResourceId, String)> {
        self.disk.sources.ready().cloned().unwrap_or_default()
    }

    pub(super) fn submit_archive_form(&mut self, form: ArchiveForm) {
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.overlay = Some(Overlay::ArchiveForm(form));
            self.set_status("名前を入力してください", StatusKind::Error);
            return;
        }
        let sources = self.archive_source_choices();
        let Some((disk_id, disk_name)) = sources.get(form.source).cloned() else {
            self.overlay = Some(Overlay::ArchiveForm(form));
            self.set_status("元にするディスクを選んでください", StatusKind::Error);
            return;
        };
        let description = form.description.trim().to_string();

        // ディスクは動いたまま取ると中身が壊れることがある。手前で注意を出す。
        self.overlay = Some(Overlay::Confirm {
            title: "アーカイブの作成".to_string(),
            body: format!(
                "ディスク「{disk_name}」からアーカイブ「{name}」を作ります。\n\n\
                 コピーが終わるまで数分から数十分かかります。\
                 接続先のサーバーが起動していると、取ったアーカイブの中身が\
                 壊れていることがあります。先に停止してください。"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::CreateArchive {
                zone: self.zone.clone(),
                name,
                description,
                disk_id,
            },
        });
    }

    pub(super) fn run_create_archive(
        &mut self,
        zone: String,
        name: String,
        description: String,
        disk_id: ResourceId,
    ) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        self.set_status(
            format!("アーカイブ「{name}」を作成しています…"),
            StatusKind::Info,
        );
        tokio::spawn(async move {
            let result = client
                .create_archive(&zone, &name, &description, disk_id)
                .await;
            let _ = tx.send(Message::DiskChanged {
                what: format!(
                    "アーカイブ「{name}」の作成を始めました（コピーが終わるまでしばらくかかります）"
                ),
                failed: "アーカイブの作成に失敗しました".to_string(),
                result: result.map(|_| ()).map_err(fmt_error),
            });
        });
    }

    fn confirm_delete_archive(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(archive) = self.selected_cloud_resource() else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "アーカイブの削除".to_string(),
            body: format!(
                "アーカイブ「{}」({}) を削除します。\n\
                 元に戻せません。実行するには名前を入力してください。",
                archive.name, self.zone
            ),
            verify: Some(archive.name.clone()),
            typed: String::new(),
            action: ConfirmAction::DeleteArchive {
                zone: self.zone.clone(),
                id: archive.id,
                name: archive.name,
            },
        });
    }

    pub(super) fn run_delete_archive(&mut self, zone: String, id: ResourceId, name: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.delete_archive(&zone, id).await;
            let _ = tx.send(Message::DiskChanged {
                what: format!("アーカイブ「{name}」を削除しました"),
                failed: "アーカイブの削除に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    // --- 自動バックアップ ---

    pub(super) fn on_key_auto_backup(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => self.open_auto_backup_form(AutoBackupFormMode::Create),
            KeyCode::Char('E') => self.open_auto_backup_form(AutoBackupFormMode::Edit),
            KeyCode::Char('D') => self.confirm_delete_auto_backup(),
            _ => {}
        }
    }

    fn open_auto_backup_form(&mut self, mode: AutoBackupFormMode) {
        if !self.require_write() {
            return;
        }
        let form = match mode {
            AutoBackupFormMode::Create => {
                // 対象ディスクは表示中のゾーンから選ぶ。
                self.load_archive_sources();
                AutoBackupForm::default()
            }
            AutoBackupFormMode::Edit => {
                let Some(item) = self.selected_managed_resource() else {
                    return;
                };
                let Some(current) = self.auto_backup_settings(&item) else {
                    return;
                };
                current
            }
        };
        self.overlay = Some(Overlay::AutoBackupForm(form));
    }

    /// 一覧に出ている値から編集フォームを組み立てる。
    ///
    /// 一覧は詳細を文字列で持っているので、そこから読み戻す。
    fn auto_backup_settings(&self, item: &ManagedResource) -> Option<AutoBackupForm> {
        let detail = |label: &str| {
            item.details
                .iter()
                .find(|(key, _)| key == label)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        let mut weekdays = [false; 7];
        for day in detail("取得曜日").split(',').map(str::trim) {
            if let Some(index) = crate::commonservice::BACKUP_WEEKDAYS
                .iter()
                .position(|d| *d == day)
            {
                weekdays[index] = true;
            }
        }
        Some(AutoBackupForm {
            mode: AutoBackupFormMode::Edit,
            id: item.id.parse().ok().map(ResourceId),
            name: item.name.clone(),
            source: 0,
            weekdays,
            weekday_cursor: 0,
            generations: detail("世代数").parse().unwrap_or(3),
            field: 0,
        })
    }

    pub(super) fn submit_auto_backup_form(&mut self, form: AutoBackupForm) {
        let name = form.name.trim().to_string();
        if name.is_empty() {
            self.overlay = Some(Overlay::AutoBackupForm(form));
            self.set_status("名前を入力してください", StatusKind::Error);
            return;
        }
        let weekdays = form.selected_weekdays();
        if weekdays.is_empty() {
            self.overlay = Some(Overlay::AutoBackupForm(form));
            self.set_status("取得する曜日を1つ以上選んでください", StatusKind::Error);
            return;
        }
        let sources = self.archive_source_choices();
        let disk = match form.mode {
            AutoBackupFormMode::Create => sources.get(form.source).cloned(),
            // 編集では対象ディスクを変えないので、ここでは使わない。
            AutoBackupFormMode::Edit => Some((ResourceId(0), String::new())),
        };
        let Some((disk_id, disk_name)) = disk else {
            self.overlay = Some(Overlay::AutoBackupForm(form));
            self.set_status("対象のディスクを選んでください", StatusKind::Error);
            return;
        };

        let input = AutoBackupInput {
            name: name.clone(),
            description: String::new(),
            disk_id,
            weekdays,
            generations: form.generations,
        };
        self.overlay = None;
        match (form.mode, form.id) {
            (AutoBackupFormMode::Create, _) => self.run_auto_backup(
                None,
                input,
                format!(
                    "ディスク「{disk_name}」の自動バックアップを作りました（{} / {} 世代）",
                    form.weekday_label(),
                    form.generations
                ),
            ),
            (AutoBackupFormMode::Edit, Some(id)) => self.run_auto_backup(
                Some(id),
                input,
                format!(
                    "自動バックアップ「{name}」を変更しました（{} / {} 世代）",
                    form.weekday_label(),
                    form.generations
                ),
            ),
            (AutoBackupFormMode::Edit, None) => {}
        }
    }

    /// 作成と変更は送る中身がほぼ同じなので、1つにまとめる。
    fn run_auto_backup(&mut self, id: Option<ResourceId>, input: AutoBackupInput, done: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        let zone = self.zone.clone();
        tokio::spawn(async move {
            let result = match id {
                None => client.create_auto_backup(&zone, &input).await.map(|_| ()),
                Some(id) => client.update_auto_backup(&zone, id, &input).await,
            };
            let _ = tx.send(Message::DiskChanged {
                what: done,
                failed: "自動バックアップの設定に失敗しました".to_string(),
                result: result.map_err(fmt_error),
            });
        });
    }

    fn confirm_delete_auto_backup(&mut self) {
        if !self.require_write() {
            return;
        }
        let Some(item) = self.selected_managed_resource() else {
            return;
        };
        let Some(id) = item.id.parse().ok().map(ResourceId) else {
            return;
        };
        self.overlay = Some(Overlay::Confirm {
            title: "自動バックアップの削除".to_string(),
            body: format!(
                "自動バックアップ「{}」を削除します。\n\n\
                 これから先の取得が止まるだけで、\
                 すでに取ったアーカイブは残ります。",
                item.name
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteAutoBackup {
                zone: self.zone.clone(),
                id,
                name: item.name,
            },
        });
    }

    pub(super) fn run_delete_auto_backup(&mut self, zone: String, id: ResourceId, name: String) {
        self.inflight += 1;
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.delete_auto_backup(&zone, id).await;
            let _ = tx.send(Message::DiskChanged {
                what: format!("自動バックアップ「{name}」を削除しました"),
                failed: "自動バックアップの削除に失敗しました".to_string(),
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
