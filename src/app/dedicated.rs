//! AppRun 専有型画面の状態と操作（閲覧のみ）。
//!
//! クラスタを選ぶと、そのクラスタに紐づくアプリ・ASG・証明書をタブで切り替えて見る。
//! ASG を選ぶとワーカーノードがぶら下がる。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::{ListState, TableState};

use super::{App, Loadable, Message, Pane, matches};
use crate::apprun_dedicated::{Application, AutoScalingGroup, Certificate, Cluster, WorkerNode};

/// 専有型画面のタブ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DedicatedTab {
    #[default]
    Overview,
    Applications,
    ScalingGroups,
    Certificates,
}

impl DedicatedTab {
    pub const ALL: [DedicatedTab; 4] = [
        DedicatedTab::Overview,
        DedicatedTab::Applications,
        DedicatedTab::ScalingGroups,
        DedicatedTab::Certificates,
    ];

    pub fn title(self) -> &'static str {
        match self {
            DedicatedTab::Overview => "概要",
            DedicatedTab::Applications => "アプリ",
            DedicatedTab::ScalingGroups => "ASG",
            DedicatedTab::Certificates => "証明書",
        }
    }
}

/// 左右どちらのペインを操作しているか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DedicatedFocus {
    #[default]
    Clusters,
    Detail,
}

#[derive(Debug, Default)]
pub struct DedicatedView {
    pub clusters: Loadable<Vec<Cluster>>,
    pub cluster_state: TableState,

    pub tab: DedicatedTab,
    pub focus: DedicatedFocus,

    /// クラスタ ID をキーにした各種一覧。
    pub details: HashMap<String, Loadable<Cluster>>,
    pub applications: HashMap<String, Loadable<Vec<Application>>>,
    pub application_state: TableState,
    pub scaling_groups: HashMap<String, Loadable<Vec<AutoScalingGroup>>>,
    pub scaling_group_state: TableState,
    /// `(クラスタID, ASG ID)` をキーにしたワーカーノード。
    pub worker_nodes: HashMap<(String, String), Loadable<Vec<WorkerNode>>>,
    pub worker_node_state: ListState,
    pub certificates: HashMap<String, Loadable<Vec<Certificate>>>,
    pub certificate_state: TableState,
}

impl App {
    // --- 表示中の要素 ---

    pub fn visible_clusters(&self) -> Vec<&Cluster> {
        let Some(items) = self.dedicated.clusters.ready() else {
            return Vec::new();
        };
        let filter = self.filters.get(Pane::Clusters);
        items
            .iter()
            .filter(|c| matches(filter, &[&c.name, &c.id]))
            .collect()
    }

    pub fn selected_cluster(&self) -> Option<&Cluster> {
        let index = self.dedicated.cluster_state.selected()?;
        self.visible_clusters().into_iter().nth(index)
    }

    /// 一覧には含まれない項目（ポートなど）を含む詳細。
    pub fn selected_cluster_detail(&self) -> Loadable<Cluster> {
        self.selected_cluster()
            .and_then(|cluster| self.dedicated.details.get(&cluster.id))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    fn cluster_scoped<T: Clone>(
        &self,
        map: &HashMap<String, Loadable<Vec<T>>>,
    ) -> Loadable<Vec<T>> {
        self.selected_cluster()
            .and_then(|cluster| map.get(&cluster.id))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_dedicated_applications(&self) -> Loadable<Vec<Application>> {
        let loadable = self.cluster_scoped(&self.dedicated.applications);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::DedicatedApplications);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|a| matches(filter, &[&a.name, &a.id]))
                .collect(),
        )
    }

    pub fn visible_scaling_groups(&self) -> Loadable<Vec<AutoScalingGroup>> {
        let loadable = self.cluster_scoped(&self.dedicated.scaling_groups);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::ScalingGroups);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|g| matches(filter, &[&g.name, &g.zone, &g.service_class]))
                .collect(),
        )
    }

    pub fn selected_scaling_group(&self) -> Option<AutoScalingGroup> {
        let index = self.dedicated.scaling_group_state.selected()?;
        self.visible_scaling_groups().ready()?.get(index).cloned()
    }

    pub fn current_worker_nodes(&self) -> Loadable<Vec<WorkerNode>> {
        let Some(cluster) = self.selected_cluster() else {
            return Loadable::Idle;
        };
        let Some(group) = self.selected_scaling_group() else {
            return Loadable::Idle;
        };
        self.dedicated
            .worker_nodes
            .get(&(cluster.id.clone(), group.id))
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_certificates(&self) -> Loadable<Vec<Certificate>> {
        let loadable = self.cluster_scoped(&self.dedicated.certificates);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::Certificates);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|c| {
                    let sans = c.alternative_names.join(" ");
                    matches(filter, &[&c.name, &c.common_name, &sans])
                })
                .collect(),
        )
    }

    pub(super) fn dedicated_active_pane(&self) -> Pane {
        if self.dedicated.focus == DedicatedFocus::Clusters {
            return Pane::Clusters;
        }
        match self.dedicated.tab {
            DedicatedTab::Overview => Pane::None,
            DedicatedTab::Applications => Pane::DedicatedApplications,
            DedicatedTab::ScalingGroups => Pane::ScalingGroups,
            DedicatedTab::Certificates => Pane::Certificates,
        }
    }

    // --- 読み込み ---

    pub(super) fn load_clusters(&mut self) {
        self.dedicated.clusters = Loadable::Loading;
        self.inflight += 1;
        let client = self.dedicated_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_clusters().await.map_err(super::fmt_error);
            let _ = tx.send(Message::Clusters(result));
        });
    }

    /// クラスタに紐づく一覧を種類ごとに読む。
    fn load_cluster_child(&mut self, cluster_id: String, kind: ChildKind) {
        let client = self.dedicated_client.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        let id = cluster_id.clone();
        match kind {
            ChildKind::Detail => {
                self.dedicated.details.insert(id.clone(), Loadable::Loading);
                tokio::spawn(async move {
                    let result = client.cluster_detail(&id).await.map_err(super::fmt_error);
                    let _ = tx.send(Message::ClusterDetail {
                        id: cluster_id,
                        result,
                    });
                });
            }
            ChildKind::Applications => {
                self.dedicated
                    .applications
                    .insert(id.clone(), Loadable::Loading);
                tokio::spawn(async move {
                    let result = client
                        .list_applications(&id)
                        .await
                        .map_err(super::fmt_error);
                    let _ = tx.send(Message::DedicatedApplications {
                        cluster: cluster_id,
                        result,
                    });
                });
            }
            ChildKind::ScalingGroups => {
                self.dedicated
                    .scaling_groups
                    .insert(id.clone(), Loadable::Loading);
                tokio::spawn(async move {
                    let result = client.list_asg(&id).await.map_err(super::fmt_error);
                    let _ = tx.send(Message::ScalingGroups {
                        cluster: cluster_id,
                        result,
                    });
                });
            }
            ChildKind::Certificates => {
                self.dedicated
                    .certificates
                    .insert(id.clone(), Loadable::Loading);
                tokio::spawn(async move {
                    let result = client
                        .list_certificates(&id)
                        .await
                        .map_err(super::fmt_error);
                    let _ = tx.send(Message::Certificates {
                        cluster: cluster_id,
                        result,
                    });
                });
            }
        }
    }

    fn load_worker_nodes(&mut self, cluster_id: String, asg_id: String) {
        self.dedicated
            .worker_nodes
            .insert((cluster_id.clone(), asg_id.clone()), Loadable::Loading);
        self.inflight += 1;
        let client = self.dedicated_client.clone();
        let tx = self.tx.clone();
        let (cluster, asg) = (cluster_id.clone(), asg_id.clone());
        tokio::spawn(async move {
            let result = client
                .list_worker_nodes(&cluster_id, &asg_id)
                .await
                .map_err(super::fmt_error);
            let _ = tx.send(Message::WorkerNodes {
                cluster,
                asg,
                result,
            });
        });
    }

    pub(super) fn dedicated_ensure_loaded(&mut self) {
        if self.dedicated.clusters.is_idle() {
            self.load_clusters();
            return;
        }
        if !self.visible_clusters().is_empty() && self.dedicated.cluster_state.selected().is_none()
        {
            self.dedicated.cluster_state.select(Some(0));
        }
        let Some(id) = self.selected_cluster().map(|c| c.id.clone()) else {
            return;
        };

        match self.dedicated.tab {
            DedicatedTab::Overview => {
                if self.dedicated.details.get(&id).is_none_or(Loadable::is_idle) {
                    self.load_cluster_child(id, ChildKind::Detail);
                }
            }
            DedicatedTab::Applications => {
                if self
                    .dedicated
                    .applications
                    .get(&id)
                    .is_none_or(Loadable::is_idle)
                {
                    self.load_cluster_child(id, ChildKind::Applications);
                } else {
                    self.fill_selection(Pane::DedicatedApplications);
                }
            }
            DedicatedTab::ScalingGroups => {
                if self
                    .dedicated
                    .scaling_groups
                    .get(&id)
                    .is_none_or(Loadable::is_idle)
                {
                    self.load_cluster_child(id, ChildKind::ScalingGroups);
                    return;
                }
                self.fill_selection(Pane::ScalingGroups);
                // 選択中の ASG のワーカーノードも見に行く。
                if let Some(group) = self.selected_scaling_group() {
                    let key = (id.clone(), group.id.clone());
                    if self
                        .dedicated
                        .worker_nodes
                        .get(&key)
                        .is_none_or(Loadable::is_idle)
                    {
                        self.load_worker_nodes(id, group.id);
                    }
                }
            }
            DedicatedTab::Certificates => {
                if self
                    .dedicated
                    .certificates
                    .get(&id)
                    .is_none_or(Loadable::is_idle)
                {
                    self.load_cluster_child(id, ChildKind::Certificates);
                } else {
                    self.fill_selection(Pane::Certificates);
                }
            }
        }
    }

    pub(super) fn dedicated_refresh(&mut self) {
        let Some(id) = self.selected_cluster().map(|c| c.id.clone()) else {
            self.load_clusters();
            return;
        };
        match self.dedicated.tab {
            DedicatedTab::Overview => self.load_clusters(),
            DedicatedTab::Applications => self.load_cluster_child(id, ChildKind::Applications),
            DedicatedTab::ScalingGroups => self.load_cluster_child(id, ChildKind::ScalingGroups),
            DedicatedTab::Certificates => self.load_cluster_child(id, ChildKind::Certificates),
        }
    }

    // --- キー入力 ---

    pub(super) fn on_key_dedicated(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab => self.cycle_dedicated_tab(1),
            KeyCode::BackTab => self.cycle_dedicated_tab(-1),
            KeyCode::Char('1') => self.set_dedicated_tab(DedicatedTab::Overview),
            KeyCode::Char('2') => self.set_dedicated_tab(DedicatedTab::Applications),
            KeyCode::Char('3') => self.set_dedicated_tab(DedicatedTab::ScalingGroups),
            KeyCode::Char('4') => self.set_dedicated_tab(DedicatedTab::Certificates),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Esc => {
                self.dedicated.focus = DedicatedFocus::Clusters
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                self.dedicated.focus = DedicatedFocus::Detail
            }
            _ => {}
        }
    }

    fn set_dedicated_tab(&mut self, tab: DedicatedTab) {
        self.dedicated.tab = tab;
        self.dedicated.focus = DedicatedFocus::Detail;
    }

    fn cycle_dedicated_tab(&mut self, delta: i32) {
        let current = DedicatedTab::ALL
            .iter()
            .position(|t| *t == self.dedicated.tab)
            .unwrap_or(0) as i32;
        let len = DedicatedTab::ALL.len() as i32;
        self.set_dedicated_tab(DedicatedTab::ALL[(current + delta).rem_euclid(len) as usize]);
    }

    /// クラスタを選び直したら、ぶら下がる選択をリセットする。
    pub(super) fn dedicated_after_cluster_change(&mut self) {
        self.dedicated.application_state.select(None);
        self.dedicated.scaling_group_state.select(None);
        self.dedicated.worker_node_state.select(None);
        self.dedicated.certificate_state.select(None);
    }

    pub(super) fn dedicated_invalidate(&mut self) {
        self.dedicated = DedicatedView::default();
    }
}

/// クラスタにぶら下がる一覧の種類。
#[derive(Debug, Clone, Copy)]
enum ChildKind {
    Detail,
    Applications,
    ScalingGroups,
    Certificates,
}
