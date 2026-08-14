use anyhow::{Context, Result, bail};
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::config::ApiCredentials;
use crate::sacloud::{flexible_number, null_as_default};

fn api_root(creds: &ApiCredentials) -> String {
    let base = creds.api_root().trim_end_matches("/zone");
    format!("{base}/api/apigw/1.0")
}

#[derive(Debug, Clone)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub plan_id: String,
    pub resource_id: i64,
    pub monthly_request: i64,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ApiGatewayService {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub path: String,
    pub port: Option<u16>,
    pub authentication: String,
    pub subscription_name: String,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub id: String,
    pub service_id: String,
    pub name: String,
    pub protocols: Vec<String>,
    pub path: String,
    pub host: String,
    pub hosts: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ApiGatewayUser {
    pub id: String,
    pub name: String,
    pub custom_id: String,
    pub groups: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UserAuthentication {
    pub basic_username: Option<String>,
    pub jwt_algorithm: Option<String>,
    pub hmac_username: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiGatewayGroup {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Domain {
    pub id: String,
    pub name: String,
    pub certificate_id: String,
    pub certificate_name: String,
}

#[derive(Debug, Clone)]
pub struct Certificate {
    pub id: String,
    pub name: String,
    pub rsa_expires_at: Option<String>,
    pub ecdsa_expires_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Oidc {
    pub id: String,
    pub name: String,
    pub issuer: String,
    pub authentication_methods: Vec<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiGwEnvelope<T> {
    apigw: T,
}

#[derive(Debug, Deserialize)]
struct SubscriptionsResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    subscriptions: Vec<RawSubscription>,
    #[serde(default, rename = "maxSubscription")]
    _max_subscription: i64,
}

#[derive(Debug, Deserialize)]
struct ServicesResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    services: Vec<RawService>,
}

#[derive(Debug, Deserialize)]
struct RoutesResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    routes: Vec<RawRoute>,
}

#[derive(Debug, Deserialize)]
struct UsersResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    users: Vec<RawUser>,
}

#[derive(Debug, Deserialize)]
struct UserAuthenticationResponse {
    #[serde(default, rename = "userAuthentication")]
    user_authentication: RawUserAuthentication,
}

#[derive(Debug, Deserialize)]
struct GroupsResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    groups: Vec<RawGroup>,
}

#[derive(Debug, Deserialize)]
struct DomainsResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    domains: Vec<RawDomain>,
}

#[derive(Debug, Deserialize)]
struct CertificatesResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    certificates: Vec<RawCertificate>,
}

#[derive(Debug, Deserialize)]
struct OidcsResponse {
    #[serde(default, deserialize_with = "null_as_default")]
    oidcs: Vec<RawOidc>,
}

#[derive(Debug, Deserialize)]
struct RawSubscription {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, rename = "planId", deserialize_with = "null_as_default")]
    plan_id: String,
    #[serde(default, rename = "resourceId", deserialize_with = "flexible_number")]
    resource_id: i64,
    #[serde(
        default,
        rename = "monthlyRequest",
        deserialize_with = "flexible_number"
    )]
    monthly_request: i64,
    service: Option<RawSubscriptionRef>,
}

#[derive(Debug, Deserialize)]
struct RawService {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    protocol: String,
    #[serde(default, deserialize_with = "null_as_default")]
    host: String,
    #[serde(default, deserialize_with = "null_as_default")]
    path: String,
    port: Option<u16>,
    #[serde(default, deserialize_with = "null_as_default")]
    authentication: String,
    subscription: Option<RawSubscriptionRef>,
}

#[derive(Debug, Deserialize)]
struct RawSubscriptionRef {
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawRoute {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, rename = "serviceId", deserialize_with = "null_as_default")]
    service_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "string_or_list")]
    protocols: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    path: String,
    #[serde(default, deserialize_with = "null_as_default")]
    host: String,
    #[serde(default, deserialize_with = "null_as_default")]
    hosts: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    methods: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawUser {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, rename = "customID", deserialize_with = "null_as_default")]
    custom_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    groups: Vec<RawGroupRef>,
    #[serde(default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawUserAuthentication {
    #[serde(default, rename = "basicAuth")]
    basic_auth: Option<RawBasicAuth>,
    #[serde(default)]
    jwt: Option<RawJwtAuth>,
    #[serde(default, rename = "hmacAuth")]
    hmac_auth: Option<RawHmacAuth>,
}

#[derive(Debug, Deserialize)]
struct RawBasicAuth {
    #[serde(default, rename = "userName", deserialize_with = "null_as_default")]
    username: String,
}

#[derive(Debug, Deserialize)]
struct RawJwtAuth {
    #[serde(default, deserialize_with = "null_as_default")]
    algorithm: String,
}

#[derive(Debug, Deserialize)]
struct RawHmacAuth {
    #[serde(default, rename = "userName", deserialize_with = "null_as_default")]
    username: String,
}

#[derive(Debug, Deserialize)]
struct RawGroupRef {
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawGroup {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawDomain {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, rename = "domainName", deserialize_with = "null_as_default")]
    name: String,
    #[serde(
        default,
        rename = "certificateId",
        deserialize_with = "null_as_default"
    )]
    certificate_id: String,
    #[serde(
        default,
        rename = "certificateName",
        deserialize_with = "null_as_default"
    )]
    certificate_name: String,
}

#[derive(Debug, Deserialize)]
struct RawCertificate {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    rsa: Option<RawCertificateDetails>,
    ecdsa: Option<RawCertificateDetails>,
}

#[derive(Debug, Deserialize)]
struct RawCertificateDetails {
    #[serde(default, rename = "expiredAt")]
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawOidc {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    issuer: String,
    #[serde(
        default,
        rename = "authenticationMethods",
        deserialize_with = "null_as_default"
    )]
    authentication_methods: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    scopes: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ApiError {
    #[serde(default, deserialize_with = "null_as_default")]
    message: String,
    #[serde(default, deserialize_with = "null_as_default")]
    detail: String,
    #[serde(default, deserialize_with = "null_as_default")]
    error_msg: String,
    #[serde(default, deserialize_with = "null_as_default")]
    error_code: String,
}

impl ApiError {
    fn parts(&self) -> Option<(&str, &str)> {
        if !self.message.is_empty() {
            return Some((&self.message, &self.detail));
        }
        if !self.error_msg.is_empty() {
            return Some((&self.error_msg, &self.error_code));
        }
        None
    }
}

#[derive(Debug)]
pub struct ApiGatewayClient {
    http: reqwest::Client,
    token: String,
    secret: String,
    api_root: String,
}

impl ApiGatewayClient {
    pub fn new(creds: &ApiCredentials) -> Result<Self> {
        Ok(Self {
            http: crate::http::client()?,
            token: creds.token.clone(),
            secret: creds.secret.clone(),
            api_root: api_root(creds),
        })
    }

    async fn get_body(&self, path: &str) -> Result<String> {
        let url = format!("{}{path}", self.api_root);
        let response = crate::http::send_with_retry(&self.http, || {
            Ok(self
                .http
                .request(Method::GET, &url)
                .basic_auth(&self.token, Some(&self.secret))
                .build()?)
        })
        .await
        .context("API Gateway APIへのリクエストに失敗しました")?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("API Gateway APIのレスポンス読み取りに失敗しました")?;
        if !status.is_success() {
            bail!("{}", format_api_error(status, &body));
        }
        Ok(body)
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<Subscription>> {
        parse_subscriptions(&self.get_body("/subscriptions").await?)
    }

    pub async fn list_services(&self) -> Result<Vec<ApiGatewayService>> {
        parse_services(&self.get_body("/services").await?)
    }

    pub async fn list_routes(&self, service_id: &str) -> Result<Vec<Route>> {
        let path = format!("/services/{service_id}/routes");
        parse_routes(&self.get_body(&path).await?)
    }

    pub async fn list_users(&self) -> Result<Vec<ApiGatewayUser>> {
        parse_users(&self.get_body("/users").await?)
    }

    pub async fn user_authentication(&self, user_id: &str) -> Result<UserAuthentication> {
        let path = format!("/users/{user_id}/authentication");
        parse_user_authentication(&self.get_body(&path).await?)
    }

    pub async fn list_groups(&self) -> Result<Vec<ApiGatewayGroup>> {
        parse_groups(&self.get_body("/groups").await?)
    }

    pub async fn list_domains(&self) -> Result<Vec<Domain>> {
        parse_domains(&self.get_body("/domains").await?)
    }

    pub async fn list_certificates(&self) -> Result<Vec<Certificate>> {
        parse_certificates(&self.get_body("/certificates").await?)
    }

    pub async fn list_oidcs(&self) -> Result<Vec<Oidc>> {
        parse_oidcs(&self.get_body("/oidc").await?)
    }
}

fn parse_body<T: DeserializeOwned>(body: &str) -> Result<T> {
    let text = if body.trim().is_empty() { "{}" } else { body };
    serde_json::from_str(text).with_context(|| {
        let head: String = text.chars().take(200).collect();
        format!("API Gateway APIのレスポンス解析に失敗しました: {head}")
    })
}

fn string_or_list<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrList {
        String(String),
        List(Vec<String>),
    }

    let value = Option::<StringOrList>::deserialize(deserializer)?;
    Ok(match value {
        None => Vec::new(),
        Some(StringOrList::List(items)) => items,
        Some(StringOrList::String(items)) => items
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
    })
}

fn parse_subscriptions(body: &str) -> Result<Vec<Subscription>> {
    let res: ApiGwEnvelope<SubscriptionsResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .subscriptions
        .into_iter()
        .map(|s| Subscription {
            id: s.id,
            name: s.name,
            plan_id: s.plan_id,
            resource_id: s.resource_id,
            monthly_request: s.monthly_request,
            service_name: s.service.map(|service| service.name).unwrap_or_default(),
        })
        .collect())
}

fn parse_services(body: &str) -> Result<Vec<ApiGatewayService>> {
    let res: ApiGwEnvelope<ServicesResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .services
        .into_iter()
        .map(|s| ApiGatewayService {
            id: s.id,
            name: s.name,
            protocol: s.protocol,
            host: s.host,
            path: s.path,
            port: s.port,
            authentication: s.authentication,
            subscription_name: s.subscription.map(|sub| sub.name).unwrap_or_default(),
        })
        .collect())
}

fn parse_routes(body: &str) -> Result<Vec<Route>> {
    let res: ApiGwEnvelope<RoutesResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .routes
        .into_iter()
        .map(|r| Route {
            id: r.id,
            service_id: r.service_id,
            name: r.name,
            protocols: r.protocols,
            path: r.path,
            host: r.host,
            hosts: r.hosts,
            methods: r.methods,
        })
        .collect())
}

fn parse_users(body: &str) -> Result<Vec<ApiGatewayUser>> {
    let res: ApiGwEnvelope<UsersResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .users
        .into_iter()
        .map(|u| ApiGatewayUser {
            id: u.id,
            name: u.name,
            custom_id: u.custom_id,
            groups: u.groups.into_iter().map(|group| group.name).collect(),
            tags: u.tags,
        })
        .collect())
}

fn parse_user_authentication(body: &str) -> Result<UserAuthentication> {
    let res: ApiGwEnvelope<UserAuthenticationResponse> = parse_body(body)?;
    let raw = res.apigw.user_authentication;
    Ok(UserAuthentication {
        basic_username: raw
            .basic_auth
            .map(|x| x.username)
            .filter(|name| !name.is_empty()),
        jwt_algorithm: raw.jwt.map(|x| x.algorithm).filter(|alg| !alg.is_empty()),
        hmac_username: raw
            .hmac_auth
            .map(|x| x.username)
            .filter(|name| !name.is_empty()),
    })
}

fn parse_groups(body: &str) -> Result<Vec<ApiGatewayGroup>> {
    let res: ApiGwEnvelope<GroupsResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .groups
        .into_iter()
        .map(|g| ApiGatewayGroup {
            id: g.id,
            name: g.name,
            tags: g.tags,
        })
        .collect())
}

fn parse_domains(body: &str) -> Result<Vec<Domain>> {
    let res: ApiGwEnvelope<DomainsResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .domains
        .into_iter()
        .map(|d| Domain {
            id: d.id,
            name: d.name,
            certificate_id: d.certificate_id,
            certificate_name: d.certificate_name,
        })
        .collect())
}

fn parse_certificates(body: &str) -> Result<Vec<Certificate>> {
    let res: ApiGwEnvelope<CertificatesResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .certificates
        .into_iter()
        .map(|c| Certificate {
            id: c.id,
            name: c.name,
            rsa_expires_at: c.rsa.and_then(|details| details.expires_at),
            ecdsa_expires_at: c.ecdsa.and_then(|details| details.expires_at),
        })
        .collect())
}

fn parse_oidcs(body: &str) -> Result<Vec<Oidc>> {
    let res: ApiGwEnvelope<OidcsResponse> = parse_body(body)?;
    Ok(res
        .apigw
        .oidcs
        .into_iter()
        .map(|o| Oidc {
            id: o.id,
            name: o.name,
            issuer: o.issuer,
            authentication_methods: o.authentication_methods,
            scopes: o.scopes,
        })
        .collect())
}

fn format_api_error(status: StatusCode, body: &str) -> String {
    let parsed = serde_json::from_str::<ApiError>(body).unwrap_or_default();
    if let Some((summary, detail)) = parsed.parts() {
        return if detail.is_empty() {
            format!("API Gateway APIエラー ({status}): {summary}")
        } else {
            format!("API Gateway APIエラー ({status}): {summary} [{detail}]")
        };
    }
    let head: String = body.trim().chars().take(200).collect();
    if head.is_empty() {
        format!("API Gateway APIエラー ({status})")
    } else {
        format!("API Gateway APIエラー ({status}): {head}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subscriptions_wrapper_and_numeric_resource_id() {
        let body = r#"{
            "apigw": {
                "maxSubscription": 2,
                "subscriptions": [
                    {
                        "id": "9f7c908b-c530-4206-b89e-8690888af90e",
                        "name": "gold-plan",
                        "planId": "plan-1",
                        "resourceId": 113700000001,
                        "monthlyRequest": 1000000,
                        "service": {"id": "service-1", "name": "orders-api"}
                    }
                ]
            }
        }"#;
        let items = parse_subscriptions(body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].resource_id, 113700000001);
        assert_eq!(items[0].service_name, "orders-api");
    }

    #[test]
    fn parses_services_wrapper_and_tolerates_null_display_fields() {
        let body = r#"{
            "apigw": {
                "services": [
                    {
                        "id": "e8efb9f8-b7b9-4702-88bb-f55672c9b63f",
                        "name": null,
                        "protocol": "https",
                        "host": null,
                        "path": "/v1",
                        "port": null,
                        "authentication": "jwt",
                        "subscription": {"name": "sub-a"}
                    }
                ]
            }
        }"#;
        let items = parse_services(body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "");
        assert_eq!(items[0].host, "");
        assert_eq!(items[0].port, None);
        assert_eq!(items[0].subscription_name, "sub-a");
    }

    #[test]
    fn parses_routes_wrapper_with_protocol_and_method_arrays() {
        let body = r#"{
            "apigw": {
                "routes": [
                    {
                        "id": "3fe04f56-f85f-4ed7-9cc2-3ebd76a2fc2d",
                        "serviceId": "e8efb9f8-b7b9-4702-88bb-f55672c9b63f",
                        "name": "orders-route",
                        "protocols": "http,https",
                        "path": "/orders",
                        "host": "example.com",
                        "hosts": ["example.com", "alt.example.com"],
                        "methods": ["GET", "POST"]
                    }
                ]
            }
        }"#;
        let items = parse_routes(body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].protocols, vec!["http", "https"]);
        assert_eq!(items[0].methods, vec!["GET", "POST"]);
        assert_eq!(items[0].hosts.len(), 2);
    }

    #[test]
    fn parses_users_wrapper() {
        let body = r#"{
            "apigw": {
                "users": [
                    {
                        "id": "4b801855-6cb4-4f13-b40b-4d6d7a9c2f89",
                        "name": "api-user",
                        "customID": null,
                        "groups": [{"id": "group-1", "name": "admins"}],
                        "tags": ["team-a"]
                    }
                ]
            }
        }"#;
        let users = parse_users(body).unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].name, "api-user");
        assert_eq!(users[0].custom_id, "");
        assert_eq!(users[0].groups, vec!["admins"]);
    }

    #[test]
    fn parses_user_authentication_and_omits_secret_values() {
        let body = r#"{
            "apigw": {
                "userAuthentication": {
                    "basicAuth": {"userName": "basic-user", "password": "do-not-expose"},
                    "jwt": {"algorithm": "HS256", "secret": "do-not-expose"},
                    "hmacAuth": {"userName": "hmac-user", "secret": "do-not-expose"}
                }
            }
        }"#;
        let auth = parse_user_authentication(body).unwrap();
        assert_eq!(auth.basic_username, Some("basic-user".to_string()));
        assert_eq!(auth.jwt_algorithm, Some("HS256".to_string()));
        assert_eq!(auth.hmac_username, Some("hmac-user".to_string()));
        let debug = format!("{auth:?}");
        assert!(!debug.contains("do-not-expose"), "{debug}");
    }

    #[test]
    fn parses_groups_domains_certificates_and_oidc_wrappers() {
        let groups = parse_groups(
            r#"{"apigw":{"groups":[{"id":"1c4ec3c0-a148-4ee8-b13f-11e2146a9e53","name":"admins","tags":["ops"]}]}}"#,
        )
        .unwrap();
        assert_eq!(groups[0].name, "admins");

        let domains = parse_domains(
            r#"{"apigw":{"domains":[{"id":"573fd6e7-5b2f-4675-b5be-b66b0cbefb9e","domainName":"api.example.com","certificateId":"cert-1","certificateName":null}]}}"#,
        )
        .unwrap();
        assert_eq!(domains[0].certificate_name, "");

        let certs = parse_certificates(
            r#"{"apigw":{"certificates":[{"id":"5fd7f632-4d8d-43c7-abf8-2aef48cf95cc","name":"wildcard","rsa":{"expiredAt":"2030-01-01T00:00:00Z"},"ecdsa":null}]}}"#,
        )
        .unwrap();
        assert_eq!(
            certs[0].rsa_expires_at,
            Some("2030-01-01T00:00:00Z".to_string())
        );
        assert_eq!(certs[0].ecdsa_expires_at, None);

        let oidcs = parse_oidcs(
            r#"{"apigw":{"oidcs":[{"id":"07338790-a3a1-4f8f-ad8e-fdc194be627e","name":"corp","issuer":"https://issuer.example.com","authenticationMethods":["client_secret_basic"],"scopes":["openid","profile"]}]}}"#,
        )
        .unwrap();
        assert_eq!(oidcs[0].authentication_methods, vec!["client_secret_basic"]);
        assert_eq!(oidcs[0].scopes, vec!["openid", "profile"]);
    }

    #[test]
    fn derives_api_root_from_credentials_root() {
        let creds = ApiCredentials {
            token: "t".to_string(),
            secret: "s".to_string(),
            source: crate::config::CredentialSource::Env,
            zone: None,
            api_root: Some("https://secure.sakura.ad.jp/cloud/zone".to_string()),
        };
        assert_eq!(
            api_root(&creds),
            "https://secure.sakura.ad.jp/cloud/api/apigw/1.0"
        );
    }
}
