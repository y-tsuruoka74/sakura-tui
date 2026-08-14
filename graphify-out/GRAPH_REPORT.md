# Graph Report - sakura-tui  (2026-08-14)

## Corpus Check
- 44 files · ~80,849 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1907 nodes · 5928 edges · 57 communities (51 shown, 6 thin omitted)
- Extraction: 96% EXTRACTED · 4% INFERRED · 0% AMBIGUOUS · INFERRED: 210 edges (avg confidence: 0.79)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Configuration and Credentials
- Managed Resource Operations
- Sacloud API Client
- Container Registry Client
- AppRun Application Flow
- Overlay UI Components
- Billing and Summaries
- Account Authentication Status
- IaaS Server Management
- Common Service Resources
- Global Key Routing
- Dedicated AppRun Resources
- Application Command Coordination
- HTTP and Runtime Bootstrap
- Switch Management
- Resource Form Models
- Monitoring Domain Models
- Observability Resource Selection
- Keychain Credential Storage
- Monitoring Key Handlers
- Monitoring API Operations
- Secret Manager Resources
- Registry and IAM Editing
- Main TUI Rendering
- Observability Application State
- Service Catalog Navigation
- Cloud Resource Parsing
- Dedicated Resource State
- Observability Screen Rendering
- Scoped Resource Filtering
- Selection and Clipboard Helpers
- Pane Navigation State
- Monitoring Storage Forms
- Paginated Resource Lists
- Dedicated Screen Rendering
- Server and Switch Screens
- Profile Management
- Registry Detail Rendering
- Storage Access Keys
- Billing Screen Rendering
- Async Application State Machine
- Secret Vault Interactions
- Cloud Resource Browser
- Account Screen Rendering
- Dashboard Management
- Simple Monitor Management
- Managed Resources Browser
- AppRun Screen Rendering
- JSON Value Flattening
- Registry List Rendering
- Managed Resource Roadmap
- Monitoring Storage Types
- Domain Behavior Tests
- Dimmed Table Styling
- Package and Local Permissions
- Storage Credential Concepts
- Project Overview

## God Nodes (most connected - your core abstractions)
1. `App` - 148 edges
2. `App` - 131 edges
3. `accent()` - 62 edges
4. `Async Message Bus` - 61 edges
5. `MonitoringClient` - 56 edges
6. `Loadable State` - 51 edges
7. `Overlay dialogs and forms` - 38 edges
8. `ResourceId` - 37 edges
9. `CredentialSource` - 35 edges
10. `border_style()` - 35 edges

## Surprising Connections (you probably didn't know these)
- `Service name summary` --conceptually_related_to--> `Service switching`  [INFERRED]
  src/main.rs → README.md
- `Managed services browsing` --rationale_for--> `Managed resources browser`  [EXTRACTED]
  README.md → src/ui/managed_resources.rs
- `AI Engine token handling` --conceptually_related_to--> `Overlay dialogs and forms`  [INFERRED]
  README.md → src/ui/overlay.rs
- `Overlay dialogs and forms` --conceptually_related_to--> `Read-only mode`  [INFERRED]
  src/ui/overlay.rs → README.md
- `Secret manager` --rationale_for--> `Secret vault`  [EXTRACTED]
  README.md → src/secretmanager.rs

## Import Cycles
- 2-file cycle: `src/app/account.rs -> src/app/mod.rs -> src/app/account.rs`
- 2-file cycle: `src/app/mod.rs -> src/app/server.rs -> src/app/mod.rs`
- 2-file cycle: `src/app/mod.rs -> src/app/switch.rs -> src/app/mod.rs`

## Hyperedges (group relationships)
- **Shared API Transport** — src_http_client, src_http_send_with_retry, src_iam_auth_issue_access_token, src_ai_engine_client, src_apprun_client, src_apprun_dedicated_client [INFERRED 0.92]
- **Async App State Pipeline** — src_app_mod_state_machine, src_app_mod_message, src_app_mod_loadable, src_app_mod_tx, src_app_mod_pane, src_app_apprun_view, src_app_dedicated_view, src_app_billing_view, src_app_server_view, src_app_observability_view [INFERRED 0.88]
- **Common Service Item Flow** — src_commonservice_dnszone, src_commonservice_simplemonitor, src_commonservice_update_dns_records, src_commonservice_update_simple_monitor, src_commonservice_set_simple_monitor_enabled [INFERRED 0.84]
- **UI screen suite** — src_ui_mod_ui, src_ui_account_draw, src_ui_apprun_draw, src_ui_billing_draw, src_ui_cloud_resources_draw, src_ui_dedicated_draw, src_ui_detail_draw, src_ui_managed_resources_draw, src_ui_observability_draw, src_ui_overlay_draw, src_ui_registries_draw, src_ui_server_draw, src_ui_switch_draw [EXTRACTED 0.92]

## Communities (57 total, 6 thin omitted)

### Community 0 - "Configuration and Credentials"
Cohesion: 0.05
Nodes (72): API Credentials, Credential Source, Access Token, IAM Credentials, Path, PathBuf, AiEngineTokenEntry, AiEngineTokenProfile (+64 more)

### Community 1 - "Managed Resource Operations"
Cohesion: 0.07
Nodes (60): add_detail(), AiEngineClient, authentication_error_has_a_setup_hint(), capability_text(), format_error(), ignores_entries_without_an_id(), non_empty(), parse_model() (+52 more)

### Community 2 - "Sacloud API Client"
Cohesion: 0.06
Nodes (47): Instant, accepts_null_strings(), ApiError, CachedIamToken, ContainerRegistry, filters_out_other_common_service_items(), FindResponse, flexible_number() (+39 more)

### Community 3 - "Container Registry Client"
Cohesion: 0.07
Nodes (57): HeaderMap, Container registry login, RequestBuilder, Auth challenge, check_status(), digest_header(), entry(), falls_back_to_any_linux() (+49 more)

### Community 4 - "AppRun Application Flow"
Cohesion: 0.06
Nodes (43): App, AppRunPane, AppRunView, Application, HashMap, KeyEvent, Option, String (+35 more)

### Community 5 - "Overlay UI Components"
Cohesion: 0.14
Nodes (66): Browse-first policy, Fn, AI Engine token handling, Read-only mode, accent(), aligned_padding(), category_picker_line(), centered() (+58 more)

### Community 6 - "Billing and Summaries"
Cohesion: 0.06
Nodes (37): Default, App, BillingFocus, BillingTab, BillingView, current_year(), month_list_is_focused_by_default(), pane_for() (+29 more)

### Community 7 - "Account Authentication Status"
Cohesion: 0.07
Nodes (46): Account, Auth Status, collects_member_errors(), id_to_string(), keeps_unknown_access_raw(), Key Permission, limit(), Member (+38 more)

### Community 8 - "IaaS Server Management"
Cohesion: 0.06
Nodes (38): App, HashMap, KeyEvent, Option, String, Vec, ServerView, Server View (+30 more)

### Community 9 - "Common Service Resources"
Cohesion: 0.08
Nodes (43): ConfirmAction, Observability View, accepts_numeric_port(), builds_dns_update_with_settings_hash(), dns_update_body(), DnsRecord, DNS Zone, FindResponse (+35 more)

### Community 10 - "Global Key Routing"
Cohesion: 0.07
Nodes (38): HashSet, AlertRuleFormMode, DnsRecordForm, DnsRecordFormMode, edit_alert_project_form(), edit_alert_rule_form(), edit_dashboard_form(), edit_dns_record_form() (+30 more)

### Community 11 - "Dedicated AppRun Resources"
Cohesion: 0.11
Nodes (40): api_root(), ApiError, Application, AutoScalingGroup, Certificate, Cluster, DedicatedClient, forbidden_includes_hint() (+32 more)

### Community 12 - "Application Command Coordination"
Cohesion: 0.11
Nodes (8): AiEngineTokenForm, App, fmt_error(), IamCredentialForm, Error, Into, SacloudClient, StatusKind

### Community 13 - "HTTP and Runtime Bootstrap"
Cohesion: 0.06
Nodes (46): DefaultTerminal, Common completion criteria, Duration, Monitoring suite, Service switching, Simple monitoring, AI Engine Client, List AI Engine Models (+38 more)

### Community 14 - "Switch Management"
Cohesion: 0.06
Nodes (31): Switch management, SwitchForm, SwitchFormMode, App, form(), HashMap, KeyEvent, Option (+23 more)

### Community 15 - "Resource Form Models"
Cohesion: 0.08
Nodes (20): AlertRuleForm, copy_to_clipboard(), DnsZoneForm, DnsZoneFormMode, LoginForm, LogMeasureRuleForm, LogRoutingForm, MetricsRoutingForm (+12 more)

### Community 16 - "Monitoring Domain Models"
Cohesion: 0.09
Nodes (38): accepts_string_ids_in_projects(), accepts_string_ids_in_storages(), AlertHistory, ApiError, format_api_error(), formats_error_from_detail(), LogMeasureRule, LogMeasureRuleInput (+30 more)

### Community 17 - "Observability Resource Selection"
Cohesion: 0.08
Nodes (5): App, String, LogRouting, MetricsRouting, NotificationRouting

### Community 18 - "Keychain Credential Storage"
Cohesion: 0.15
Nodes (39): ai_engine_key(), availability(), credential_key(), decode_pair(), delete_ai_engine_token(), delete_api_credentials(), delete_iam_private_key(), Delete Named AI Engine Token (+31 more)

### Community 19 - "Monitoring Key Handlers"
Cohesion: 0.09
Nodes (7): AlertProjectForm, AlertProjectFormMode, NotificationTargetForm, NotificationTargetFormMode, AlertProject, AlertRule, NotificationTarget

### Community 20 - "Monitoring API Operations"
Cohesion: 0.15
Nodes (7): AlertRuleInput, MonitoringClient, parses_log_measure_rule_and_preserves_matcher_json(), parses_log_routing_and_builds_payload(), Client, Method, Result

### Community 21 - "Secret Manager Resources"
Cohesion: 0.12
Nodes (19): Secret manager, Vault lister, PaginatedSecretList, PaginatedVaultList, parses_vault_list(), RawSecret, RawVault, From (+11 more)

### Community 22 - "Registry and IAM Editing"
Cohesion: 0.11
Nodes (7): credential_messages_survive_epoch_change(), iam_user_form_debug_redacts_password(), IamResourceForm, IamResourceFormMode, IamRoleForm, old_results_are_dropped(), secret_form_debug_redacts_value()

### Community 23 - "Main TUI Rendering"
Cohesion: 0.14
Nodes (29): compact_service_area_keeps_all_available_space(), credential_badge(), draw(), draw_body(), draw_full_width_error(), draw_header(), draw_hints(), draw_registry() (+21 more)

### Community 24 - "Observability Application State"
Cohesion: 0.10
Nodes (19): TableState, dns_form(), dns_record_from_form(), DnsView, format_match_labels(), ListFocus, monitor_form(), MonitoringTab (+11 more)

### Community 25 - "Service Catalog Navigation"
Cohesion: 0.10
Nodes (15): Cloud Resource Kind, Item, Iterator, arg_names_are_unique(), Availability, availability_reason(), Category, category_order_matches_service_order() (+7 more)

### Community 26 - "Cloud Resource Parsing"
Cohesion: 0.17
Nodes (21): add_detail(), CloudResource, CloudResourceKind, detail_fields(), find_items(), first_non_empty(), packet_filter_rule_summary(), parse_resource() (+13 more)

### Community 27 - "Dedicated Resource State"
Cohesion: 0.13
Nodes (10): App, ChildKind, DedicatedFocus, DedicatedTab, DedicatedView, Application, HashMap, KeyEvent (+2 more)

### Community 28 - "Observability Screen Rendering"
Cohesion: 0.32
Nodes (24): Constraint, Row, draw_dashboards(), draw_dns(), draw_histories(), draw_log_measure_rules(), draw_log_routings(), draw_metrics_routings() (+16 more)

### Community 29 - "Scoped Resource Filtering"
Cohesion: 0.20
Nodes (6): T, Vec, Loadable State, matches(), Vec, SimpleMonitorView

### Community 30 - "Selection and Clipboard Helpers"
Cohesion: 0.22
Nodes (4): Loadable<T>, T, Vec, TagKey

### Community 31 - "Pane Navigation State"
Cohesion: 0.17
Nodes (7): CloudResourcesView, Filters, ListState, Filter Pane Registry, HashMap, SelectableList, Switch View

### Community 32 - "Monitoring Storage Forms"
Cohesion: 0.11
Nodes (6): StorageAccessKeyForm, StorageAccessKeyFormMode, StorageForm, StorageFormMode, StorageRetentionForm, StorageAccessKey

### Community 33 - "Paginated Resource Lists"
Cohesion: 0.32
Nodes (6): Paginated, Paginated<T>, F, T, Vec, storage_ref_id()

### Community 34 - "Dedicated Screen Rendering"
Cohesion: 0.35
Nodes (19): Paragraph, AppRun dedicated endpoint, AppRun dedicated screen, draw_applications(), draw_certificates(), draw_clusters(), draw_overview(), draw_scaling_groups() (+11 more)

### Community 35 - "Server and Switch Screens"
Cohesion: 0.28
Nodes (15): Server and IaaS browsing, field(), Line, Server screen, draw_detail(), draw_list(), App, Frame (+7 more)

### Community 36 - "Profile Management"
Cohesion: 0.18
Nodes (4): ApiRootChoice, edit_profile_form(), ProfileForm, ProfileStorage

### Community 37 - "Registry Detail Rendering"
Cohesion: 0.40
Nodes (16): Registry detail screen, draw_images(), draw_overview(), draw_repositories(), draw_tag_detail(), draw_tags(), draw_users(), format_bytes() (+8 more)

### Community 38 - "Storage Access Keys"
Cohesion: 0.24
Nodes (7): access_key_secret(), parses_wrapped_access_key_secret(), Debug, Formatter, Monitoring storage, Storage kind, StorageAccessKeySecret

### Community 39 - "Billing Screen Rendering"
Cohesion: 0.38
Nodes (10): Billing screen, draw_bills(), draw_details(), draw_summary(), draw_tabs(), App, Frame, Rect (+2 more)

### Community 40 - "Async Application State Machine"
Cohesion: 0.21
Nodes (8): Box, Async Message Bus, Mode, Application, Self, App State Machine, Generation-Aware Sender, UnboundedSender

### Community 42 - "Cloud Resource Browser"
Cohesion: 0.42
Nodes (9): Managed services browsing, Cloud resources browser, draw_detail(), draw_list(), App, Color, Frame, Rect (+1 more)

### Community 43 - "Account Screen Rendering"
Cohesion: 0.44
Nodes (9): Account screen, draw_selected(), draw_table(), App, Frame, Rect, Style, section_style() (+1 more)

### Community 44 - "Dashboard Management"
Cohesion: 0.25
Nodes (3): DashboardForm, DashboardFormMode, DashboardProject

### Community 46 - "Managed Resources Browser"
Cohesion: 0.50
Nodes (8): Managed resources browser, draw_detail(), draw_list(), App, Color, Frame, Rect, status_color()

### Community 47 - "AppRun Screen Rendering"
Cohesion: 0.75
Nodes (7): AppRun shared screen, draw_applications(), draw_detail(), draw_versions(), App, Frame, Rect

### Community 48 - "JSON Value Flattening"
Cohesion: 0.33
Nodes (5): FlattenValue, flexible_float(), Option<serde_json::Value>, D, Error

### Community 49 - "Registry List Rendering"
Cohesion: 0.47
Nodes (5): UI module root, Registry list screen, App, Frame, Rect

### Community 50 - "Managed Resource Roadmap"
Cohesion: 0.50
Nodes (4): Future service roadmap, Managed resource lister, Managed resource, Managed resource kind

### Community 52 - "Domain Behavior Tests"
Cohesion: 0.50
Nodes (3): Self, rule_payload_omits_disabled_threshold_with_null(), system_storage_does_not_support_access_keys()

### Community 53 - "Dimmed Table Styling"
Cohesion: 0.67
Nodes (3): Cell, dim(), String

## Knowledge Gaps
- **31 isolated node(s):** `sakura-tui`, `SacloudClient`, `Claims`, `Paginated<T>`, `Sample` (+26 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **6 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Async Message Bus` connect `Async Application State Machine` to `Configuration and Credentials`, `Managed Resource Operations`, `Sacloud API Client`, `Container Registry Client`, `AppRun Application Flow`, `Billing and Summaries`, `Account Authentication Status`, `IaaS Server Management`, `Common Service Resources`, `Global Key Routing`, `Dedicated AppRun Resources`, `Application Command Coordination`, `Switch Management`, `Resource Form Models`, `Monitoring Domain Models`, `Observability Resource Selection`, `Monitoring Key Handlers`, `Secret Manager Resources`, `Service Catalog Navigation`, `Cloud Resource Parsing`, `Scoped Resource Filtering`, `Selection and Clipboard Helpers`, `Monitoring Storage Forms`, `Profile Management`, `Storage Access Keys`, `Dashboard Management`?**
  _High betweenness centrality (0.222) - this node is a cross-community bridge._
- **Why does `App` connect `Application Command Coordination` to `Configuration and Credentials`, `Managed Resource Operations`, `Container Registry Client`, `AppRun Application Flow`, `Billing and Summaries`, `Account Authentication Status`, `IaaS Server Management`, `Global Key Routing`, `Dedicated AppRun Resources`, `Switch Management`, `Resource Form Models`, `Monitoring API Operations`, `Registry and IAM Editing`, `Observability Application State`, `Service Catalog Navigation`, `Dedicated Resource State`, `Scoped Resource Filtering`, `Selection and Clipboard Helpers`, `Pane Navigation State`, `Profile Management`, `Async Application State Machine`?**
  _High betweenness centrality (0.183) - this node is a cross-community bridge._
- **Why does `CredentialSource` connect `Configuration and Credentials` to `Sacloud API Client`, `Overlay UI Components`, `Async Application State Machine`, `Application Command Coordination`, `HTTP and Runtime Bootstrap`, `Resource Form Models`, `Registry and IAM Editing`, `Main TUI Rendering`?**
  _High betweenness centrality (0.123) - this node is a cross-community bridge._
- **Are the 56 inferred relationships involving `accent()` (e.g. with `draw_table()` and `section_style()`) actually correct?**
  _`accent()` has 56 INFERRED edges - model-reasoned connections that need verification._
- **What connects `sakura-tui`, `SacloudClient`, `Claims` to the rest of the system?**
  _31 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Configuration and Credentials` be split into smaller, more focused modules?**
  _Cohesion score 0.051833122629582805 - nodes in this community are weakly interconnected._
- **Should `Managed Resource Operations` be split into smaller, more focused modules?**
  _Cohesion score 0.06843718079673136 - nodes in this community are weakly interconnected._