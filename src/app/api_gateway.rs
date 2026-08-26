//! API Gateway 画面の状態と操作。

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::TableState;

use super::{App, Loadable, Message, Pane, child_id_to_load, fmt_error, matches};
use crate::api_gateway::{
    ApiGatewayGroup, ApiGatewayService, ApiGatewayUser, Certificate, Domain, Oidc, Route,
    Subscription, UserAuthentication,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApiGatewayTab {
    #[default]
    Subscriptions,
    Services,
    Routes,
    Users,
    Groups,
    Domains,
    Certificates,
    Oidc,
}

impl ApiGatewayTab {
    pub const ALL: [ApiGatewayTab; 8] = [
        ApiGatewayTab::Subscriptions,
        ApiGatewayTab::Services,
        ApiGatewayTab::Routes,
        ApiGatewayTab::Users,
        ApiGatewayTab::Groups,
        ApiGatewayTab::Domains,
        ApiGatewayTab::Certificates,
        ApiGatewayTab::Oidc,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ApiGatewayTab::Subscriptions => "Subscriptions",
            ApiGatewayTab::Services => "Services",
            ApiGatewayTab::Routes => "Routes",
            ApiGatewayTab::Users => "Users",
            ApiGatewayTab::Groups => "Groups",
            ApiGatewayTab::Domains => "Domains",
            ApiGatewayTab::Certificates => "Certificates",
            ApiGatewayTab::Oidc => "OIDC",
        }
    }

    pub fn cycled(self, delta: i32) -> Self {
        let current = Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0) as i32;
        let len = Self::ALL.len() as i32;
        Self::ALL[(current + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Default)]
pub struct ApiGatewayView {
    pub tab: ApiGatewayTab,
    pub subscriptions: Loadable<Vec<Subscription>>,
    pub subscription_state: TableState,
    pub services: Loadable<Vec<ApiGatewayService>>,
    pub service_state: TableState,
    pub routes: HashMap<String, Loadable<Vec<Route>>>,
    pub route_state: TableState,
    pub users: Loadable<Vec<ApiGatewayUser>>,
    pub user_state: TableState,
    pub authentications: HashMap<String, Loadable<UserAuthentication>>,
    pub groups: Loadable<Vec<ApiGatewayGroup>>,
    pub group_state: TableState,
    pub domains: Loadable<Vec<Domain>>,
    pub domain_state: TableState,
    pub certificates: Loadable<Vec<Certificate>>,
    pub certificate_state: TableState,
    pub oidcs: Loadable<Vec<Oidc>>,
    pub oidc_state: TableState,
}

impl App {
    pub fn visible_api_gateway_subscriptions(&self) -> Loadable<Vec<Subscription>> {
        let Loadable::Ready(items) = &self.api_gateway.subscriptions else {
            return self.api_gateway.subscriptions.clone();
        };
        let filter = self.filters.get(Pane::ApiGatewaySubscriptions);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.plan_id,
                            &item.service_name,
                            &item.resource_id.to_string(),
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_api_gateway_subscription(&self) -> Option<Subscription> {
        let index = self.api_gateway.subscription_state.selected()?;
        self.visible_api_gateway_subscriptions()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_api_gateway_services(&self) -> Loadable<Vec<ApiGatewayService>> {
        let Loadable::Ready(items) = &self.api_gateway.services else {
            return self.api_gateway.services.clone();
        };
        let filter = self.filters.get(Pane::ApiGatewayServices);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.protocol,
                            &item.host,
                            &item.path,
                            &item.authentication,
                            &item.subscription_name,
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_api_gateway_service(&self) -> Option<ApiGatewayService> {
        let index = self.api_gateway.service_state.selected()?;
        self.visible_api_gateway_services()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_api_gateway_routes(&self) -> Loadable<Vec<Route>> {
        let Some(service) = self.selected_api_gateway_service() else {
            return Loadable::Idle;
        };
        let loadable = self
            .api_gateway
            .routes
            .get(&service.id)
            .cloned()
            .unwrap_or(Loadable::Idle);
        let Loadable::Ready(items) = loadable else {
            return loadable;
        };
        let filter = self.filters.get(Pane::ApiGatewayRoutes);
        Loadable::Ready(
            items
                .into_iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.service_id,
                            &item.name,
                            &item.path,
                            &item.host,
                            &item.protocols.join(","),
                            &item.hosts.join(","),
                            &item.methods.join(","),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub fn selected_api_gateway_route(&self) -> Option<Route> {
        let index = self.api_gateway.route_state.selected()?;
        self.visible_api_gateway_routes()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_api_gateway_users(&self) -> Loadable<Vec<ApiGatewayUser>> {
        let Loadable::Ready(items) = &self.api_gateway.users else {
            return self.api_gateway.users.clone();
        };
        let filter = self.filters.get(Pane::ApiGatewayUsers);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.custom_id,
                            &item.groups.join(","),
                            &item.tags.join(","),
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_api_gateway_user(&self) -> Option<ApiGatewayUser> {
        let index = self.api_gateway.user_state.selected()?;
        self.visible_api_gateway_users()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn selected_api_gateway_user_authentication(&self) -> Loadable<UserAuthentication> {
        let Some(user) = self.selected_api_gateway_user() else {
            return Loadable::Idle;
        };
        self.api_gateway
            .authentications
            .get(&user.id)
            .cloned()
            .unwrap_or(Loadable::Idle)
    }

    pub fn visible_api_gateway_groups(&self) -> Loadable<Vec<ApiGatewayGroup>> {
        let Loadable::Ready(items) = &self.api_gateway.groups else {
            return self.api_gateway.groups.clone();
        };
        let filter = self.filters.get(Pane::ApiGatewayGroups);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| matches(filter, &[&item.id, &item.name, &item.tags.join(",")]))
                .cloned()
                .collect(),
        )
    }

    pub fn selected_api_gateway_group(&self) -> Option<ApiGatewayGroup> {
        let index = self.api_gateway.group_state.selected()?;
        self.visible_api_gateway_groups()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_api_gateway_domains(&self) -> Loadable<Vec<Domain>> {
        let Loadable::Ready(items) = &self.api_gateway.domains else {
            return self.api_gateway.domains.clone();
        };
        let filter = self.filters.get(Pane::ApiGatewayDomains);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.certificate_id,
                            &item.certificate_name,
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_api_gateway_domain(&self) -> Option<Domain> {
        let index = self.api_gateway.domain_state.selected()?;
        self.visible_api_gateway_domains()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_api_gateway_certificates(&self) -> Loadable<Vec<Certificate>> {
        let Loadable::Ready(items) = &self.api_gateway.certificates else {
            return self.api_gateway.certificates.clone();
        };
        let filter = self.filters.get(Pane::ApiGatewayCertificates);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.rsa_expires_at.clone().unwrap_or_default(),
                            &item.ecdsa_expires_at.clone().unwrap_or_default(),
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_api_gateway_certificate(&self) -> Option<Certificate> {
        let index = self.api_gateway.certificate_state.selected()?;
        self.visible_api_gateway_certificates()
            .ready()?
            .get(index)
            .cloned()
    }

    pub fn visible_api_gateway_oidcs(&self) -> Loadable<Vec<Oidc>> {
        let Loadable::Ready(items) = &self.api_gateway.oidcs else {
            return self.api_gateway.oidcs.clone();
        };
        let filter = self.filters.get(Pane::ApiGatewayOidcs);
        Loadable::Ready(
            items
                .iter()
                .filter(|item| {
                    matches(
                        filter,
                        &[
                            &item.id,
                            &item.name,
                            &item.issuer,
                            &item.authentication_methods.join(","),
                            &item.scopes.join(","),
                        ],
                    )
                })
                .cloned()
                .collect(),
        )
    }

    pub fn selected_api_gateway_oidc(&self) -> Option<Oidc> {
        let index = self.api_gateway.oidc_state.selected()?;
        self.visible_api_gateway_oidcs()
            .ready()?
            .get(index)
            .cloned()
    }

    pub(super) fn api_gateway_ensure_loaded(&mut self) {
        if self.api_gateway.subscriptions.is_idle() {
            self.load_api_gateway_subscriptions();
        } else {
            self.fill_selection(Pane::ApiGatewaySubscriptions);
        }
        if self.api_gateway.services.is_idle() {
            self.load_api_gateway_services();
        } else {
            self.fill_selection(Pane::ApiGatewayServices);
        }
        if self.api_gateway.users.is_idle() {
            self.load_api_gateway_users();
        } else {
            self.fill_selection(Pane::ApiGatewayUsers);
        }
        if self.api_gateway.groups.is_idle() {
            self.load_api_gateway_groups();
        } else {
            self.fill_selection(Pane::ApiGatewayGroups);
        }
        if self.api_gateway.domains.is_idle() {
            self.load_api_gateway_domains();
        } else {
            self.fill_selection(Pane::ApiGatewayDomains);
        }
        if self.api_gateway.certificates.is_idle() {
            self.load_api_gateway_certificates();
        } else {
            self.fill_selection(Pane::ApiGatewayCertificates);
        }
        if self.api_gateway.oidcs.is_idle() {
            self.load_api_gateway_oidcs();
        } else {
            self.fill_selection(Pane::ApiGatewayOidcs);
        }

        let selected_service_id = self
            .selected_api_gateway_service()
            .map(|service| service.id);
        if let Some(service_id) =
            child_id_to_load(selected_service_id.clone(), &self.api_gateway.routes)
        {
            self.load_api_gateway_routes(service_id);
        } else if selected_service_id.is_some() {
            self.fill_selection(Pane::ApiGatewayRoutes);
        }

        let selected_user_id = self.selected_api_gateway_user().map(|user| user.id);
        if let Some(user_id) = child_id_to_load(selected_user_id, &self.api_gateway.authentications)
        {
            self.load_api_gateway_user_authentication(user_id);
        }
    }

    fn load_api_gateway_subscriptions(&mut self) {
        self.api_gateway.subscriptions = Loadable::Loading;
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_subscriptions().await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewaySubscriptions { result });
        });
    }

    fn load_api_gateway_services(&mut self) {
        self.api_gateway.services = Loadable::Loading;
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_services().await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayServices { result });
        });
    }

    fn load_api_gateway_routes(&mut self, service_id: String) {
        self.api_gateway
            .routes
            .insert(service_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_routes(&service_id).await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayRoutes { service_id, result });
        });
    }

    fn load_api_gateway_users(&mut self) {
        self.api_gateway.users = Loadable::Loading;
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_users().await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayUsers { result });
        });
    }

    fn load_api_gateway_user_authentication(&mut self, user_id: String) {
        self.api_gateway
            .authentications
            .insert(user_id.clone(), Loadable::Loading);
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .user_authentication(&user_id)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayUserAuthentication { user_id, result });
        });
    }

    fn load_api_gateway_groups(&mut self) {
        self.api_gateway.groups = Loadable::Loading;
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_groups().await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayGroups { result });
        });
    }

    fn load_api_gateway_domains(&mut self) {
        self.api_gateway.domains = Loadable::Loading;
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_domains().await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayDomains { result });
        });
    }

    fn load_api_gateway_certificates(&mut self) {
        self.api_gateway.certificates = Loadable::Loading;
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_certificates().await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayCertificates { result });
        });
    }

    fn load_api_gateway_oidcs(&mut self) {
        self.api_gateway.oidcs = Loadable::Loading;
        self.inflight += 1;
        let client = self.api_gateway_client.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_oidcs().await.map_err(fmt_error);
            let _ = tx.send(Message::ApiGatewayOidcs { result });
        });
    }

    pub(super) fn on_key_api_gateway(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.cycle_api_gateway_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_api_gateway_tab(1),
            KeyCode::Char('1') => self.set_api_gateway_tab(ApiGatewayTab::Subscriptions),
            KeyCode::Char('2') => self.set_api_gateway_tab(ApiGatewayTab::Services),
            KeyCode::Char('3') => self.set_api_gateway_tab(ApiGatewayTab::Routes),
            KeyCode::Char('4') => self.set_api_gateway_tab(ApiGatewayTab::Users),
            KeyCode::Char('5') => self.set_api_gateway_tab(ApiGatewayTab::Groups),
            KeyCode::Char('6') => self.set_api_gateway_tab(ApiGatewayTab::Domains),
            KeyCode::Char('7') => self.set_api_gateway_tab(ApiGatewayTab::Certificates),
            KeyCode::Char('8') => self.set_api_gateway_tab(ApiGatewayTab::Oidc),
            _ => {}
        }
    }

    fn set_api_gateway_tab(&mut self, tab: ApiGatewayTab) {
        self.api_gateway.tab = tab;
    }

    fn cycle_api_gateway_tab(&mut self, delta: i32) {
        self.api_gateway.tab = self.api_gateway.tab.cycled(delta);
    }

    pub(super) fn api_gateway_refresh(&mut self) {
        self.api_gateway.subscriptions = Loadable::Idle;
        self.api_gateway.services = Loadable::Idle;
        self.api_gateway.users = Loadable::Idle;
        self.api_gateway.groups = Loadable::Idle;
        self.api_gateway.domains = Loadable::Idle;
        self.api_gateway.certificates = Loadable::Idle;
        self.api_gateway.oidcs = Loadable::Idle;
        self.api_gateway.routes.clear();
        self.api_gateway.authentications.clear();
        self.api_gateway.route_state.select(None);
        self.api_gateway.subscription_state.select(None);
        self.api_gateway.service_state.select(None);
        self.api_gateway.user_state.select(None);
        self.api_gateway.group_state.select(None);
        self.api_gateway.domain_state.select(None);
        self.api_gateway.certificate_state.select(None);
        self.api_gateway.oidc_state.select(None);
        self.api_gateway_ensure_loaded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncached_selected_child_is_scheduled_once() {
        let cache = HashMap::<String, Loadable<Vec<Route>>>::new();
        assert_eq!(
            child_id_to_load(Some("service-1".to_string()), &cache),
            Some("service-1".to_string())
        );

        let cache = HashMap::from([(
            "service-1".to_string(),
            Loadable::Ready(Vec::<Route>::new()),
        )]);
        assert_eq!(
            child_id_to_load(Some("service-1".to_string()), &cache),
            None
        );
    }
}
