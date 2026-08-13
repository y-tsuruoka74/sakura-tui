# Graph Report - .  (2026-08-13)

## Corpus Check
- Corpus is ~43,048 words - fits in a single context window. You may not need a graph.

## Summary
- 1258 nodes · 3465 edges · 32 communities (29 shown, 3 thin omitted)
- Extraction: 96% EXTRACTED · 4% INFERRED · 0% AMBIGUOUS · INFERRED: 135 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Dedicated AppRun Resources
- Reusable UI Components
- Container Registry API
- Shared AppRun Services
- Sacloud API Models
- Credential Configuration
- IaaS Server Control
- Billing Data Views
- Account Permissions
- Common Service Resources
- Application Navigation
- Architecture and Clients
- Overlay Dialog Rendering
- HTTP Retry Runtime
- Secret Manager
- Monitoring Data Models
- Root TUI Layout
- Credential State Transitions
- Server UI Workflow
- Keychain Storage
- Registry Async Operations
- Pane Navigation
- Registry Selection State
- Loadable View State
- Profile Form State
- Registry User Actions
- Monitoring Client API
- Monitoring Interactions
- Registry Form Editing
- Registry Detail Navigation
- Rust TUI Dependencies
- Project Overview

## God Nodes (most connected - your core abstractions)
1. `App` - 119 edges
2. `Message` - 44 edges
3. `accent()` - 37 edges
4. `Loadable` - 36 edges
5. `border_style()` - 29 edges
6. `App` - 26 edges
7. `ResourceId` - 25 edges
8. `matches()` - 24 edges
9. `ProfileForm` - 24 edges
10. `CredentialSource` - 24 edges

## Surprising Connections (you probably didn't know these)
- `Loadable Service Cache` --conceptually_related_to--> `Asynchronous Responsive UI`  [INFERRED]
  src/app/mod.rs → README.md
- `Dedicated AppRun Read-Only Boundary` --rationale_for--> `Dedicated AppRun Client`  [EXTRACTED]
  README.md → src/apprun_dedicated.rs
- `Transient Failure Retry Policy` --rationale_for--> `Retrying HTTP Transport`  [EXTRACTED]
  README.md → src/http.rs
- `sakura-tui` --references--> `Service Catalog and Navigation`  [EXTRACTED]
  README.md → src/app/mod.rs
- `Read-Only-by-Default Safety` --rationale_for--> `Read-Only Write Guard`  [EXTRACTED]
  README.md → src/app/mod.rs

## Import Cycles
- 2-file cycle: `src/app/account.rs -> src/app/mod.rs -> src/app/account.rs`
- 2-file cycle: `src/app/mod.rs -> src/app/server.rs -> src/app/mod.rs`

## Hyperedges (group relationships)
- **Shared Authenticated HTTP Client Pattern** — src_sacloud_client, src_apprun_client, dedicated_client, src_monitoring_client, src_http_retry_transport, src_config_api_credentials [EXTRACTED 1.00]
- **Asynchronous TUI Data Flow** — src_main_runtime, app_state_machine, app_message_channel, app_loadable_cache [EXTRACTED 1.00]
- **Service-Specific View Models** — src_app_account_view, src_app_apprun_view, src_app_dedicated_view, src_app_billing_view, src_app_observability_views, app_state_machine [EXTRACTED 1.00]
- **Root TUI Composition** — src_ui_mod_root_renderer, src_ui_mod_service_dispatch, src_ui_mod_status_and_hints, src_ui_overlay_overlay_router [EXTRACTED 1.00]
- **Registry Navigation Flow** — src_ui_registries_registry_list, src_ui_detail_registry_detail, src_ui_detail_repository_tag_drilldown [EXTRACTED 1.00]
- **Server Control Flow** — src_ui_server_server_console, src_app_server_server_filtering, src_app_server_power_confirmation, src_app_server_power_execution [INFERRED 0.88]

## Communities (32 total, 3 thin omitted)

### Community 0 - "Dedicated AppRun Resources"
Cohesion: 0.06
Nodes (53): App, ChildKind, DedicatedFocus, DedicatedTab, DedicatedView, Application, HashMap, KeyEvent (+45 more)

### Community 1 - "Reusable UI Components"
Cohesion: 0.07
Nodes (82): Cell, Constraint, Paragraph, Row, draw(), draw_selected(), draw_table(), App (+74 more)

### Community 2 - "Container Registry API"
Cohesion: 0.07
Nodes (52): HeaderMap, Mutex, RequestBuilder, RegistryLogin, Challenge, check_status(), digest_header(), entry() (+44 more)

### Community 3 - "Shared AppRun Services"
Cohesion: 0.06
Nodes (43): App, AppRunPane, AppRunView, Application, HashMap, KeyEvent, Option, String (+35 more)

### Community 4 - "Sacloud API Models"
Cohesion: 0.07
Nodes (40): accepts_null_strings(), ApiError, ContainerRegistry, filters_out_other_common_service_items(), FindResponse, flexible_number(), format_api_error(), formats_api_error_from_json() (+32 more)

### Community 5 - "Credential Configuration"
Cohesion: 0.08
Nodes (37): Path, PathBuf, ApiCredentials, available_credential_sources(), clean_secret(), Config, config_path(), create_keychain_credential() (+29 more)

### Community 6 - "IaaS Server Control"
Cohesion: 0.06
Nodes (38): App, KeyEvent, Option, String, environments_have_distinct_zones(), every_zone_has_a_description(), known_zones_for(), NakedDisk (+30 more)

### Community 7 - "Billing Data Views"
Cohesion: 0.06
Nodes (35): Default, App, BillingFocus, BillingTab, BillingView, current_year(), month_list_is_focused_by_default(), pane_for() (+27 more)

### Community 8 - "Account Permissions"
Cohesion: 0.07
Nodes (45): Account, AuthStatus, collects_member_errors(), id_to_string(), keeps_unknown_access_raw(), KeyPermission, limit(), Member (+37 more)

### Community 9 - "Common Service Resources"
Cohesion: 0.10
Nodes (33): accepts_numeric_port(), DnsRecord, DnsZone, FindResponse, has_class(), NakedDns, NakedDnsRecord, NakedDnsSetting (+25 more)

### Community 10 - "Application Navigation"
Cohesion: 0.06
Nodes (27): HashSet, Item, Iterator, arg_names_are_unique(), Availability, availability_reason(), Category, category_order_matches_service_order() (+19 more)

### Community 11 - "Architecture and Clients"
Cohesion: 0.07
Nodes (42): Loadable Service Cache, Asynchronous Message Channel, Read-Only Write Guard, Service Catalog and Navigation, Application State Machine, Dedicated AppRun Client, Dedicated AppRun Resource Hierarchy, Asynchronous Responsive UI (+34 more)

### Community 12 - "Overlay Dialog Rendering"
Cohesion: 0.17
Nodes (38): Fn, accent(), centered(), choice_line(), dialog(), dialog_height(), draw(), draw_confirm() (+30 more)

### Community 13 - "HTTP Retry Runtime"
Cohesion: 0.09
Nodes (29): DefaultTerminal, Duration, client(), describe(), is_retryable(), retry_after(), Error, F (+21 more)

### Community 14 - "Secret Manager"
Cohesion: 0.12
Nodes (17): Option, PaginatedSecretList, PaginatedVaultList, parses_vault_list(), RawSecret, RawVault, From, Option (+9 more)

### Community 15 - "Monitoring Data Models"
Cohesion: 0.09
Nodes (23): accepts_string_ids_in_projects(), accepts_string_ids_in_storages(), ApiError, FlattenValue, flexible_float(), format_api_error(), formats_error_from_detail(), Option<serde_json::Value> (+15 more)

### Community 16 - "Root TUI Layout"
Cohesion: 0.15
Nodes (26): credential_badge(), draw(), draw_body(), draw_full_width_error(), draw_header(), draw_hints(), draw_registry(), draw_status() (+18 more)

### Community 17 - "Credential State Transitions"
Cohesion: 0.16
Nodes (4): fmt_error(), Error, Into, CredentialSource

### Community 18 - "Server UI Workflow"
Cohesion: 0.08
Nodes (29): Asynchronous Server Loading, Server Power Action Confirmation, Asynchronous Power Action Execution, Power State Mismatch Guard, Risky Action Name Verification, Server Search and Selection, Zone-scoped Server View State, API Key Permission Dashboard (+21 more)

### Community 19 - "Keychain Storage"
Cohesion: 0.19
Nodes (22): availability(), credential_key(), decode_pair(), delete_api_credentials(), delete_password(), deleting_missing_entry_is_ok(), encode_pair(), encodes_pair_in_one_entry() (+14 more)

### Community 20 - "Registry Async Operations"
Cohesion: 0.13
Nodes (11): Box, copy_to_clipboard(), Loadable<T>, Message, Application, Result, String, T (+3 more)

### Community 21 - "Pane Navigation"
Cohesion: 0.17
Nodes (5): Filters, ListState, Pane, KeyEvent, SelectableList

### Community 22 - "Registry Selection State"
Cohesion: 0.21
Nodes (3): Option, Vec, RegistryUser

### Community 23 - "Loadable View State"
Cohesion: 0.20
Nodes (16): AccountView, Loadable, TableState, DnsView, ListFocus, MonitoringView, HashMap, String (+8 more)

### Community 24 - "Profile Form State"
Cohesion: 0.13
Nodes (6): ApiRootChoice, edit_profile_form(), Mode, ProfileForm, ProfileStorage, Self

### Community 26 - "Monitoring Client API"
Cohesion: 0.30
Nodes (9): MonitoringClient, Paginated, Paginated<T>, Client, F, Result, Self, T (+1 more)

### Community 27 - "Monitoring Interactions"
Cohesion: 0.16
Nodes (4): App, MonitoringTab, KeyEvent, AlertProject

### Community 28 - "Registry Form Editing"
Cohesion: 0.47
Nodes (3): edit_registry_form(), RegistryForm, RegistryFormMode

### Community 29 - "Registry Detail Navigation"
Cohesion: 0.50
Nodes (4): Container Registry Detail Pane, Registry Users and Login-aware Images, Repository Tag Drilldown, Container Registry List Pane

## Knowledge Gaps
- **32 isolated node(s):** `sakura-tui`, `SacloudClient`, `Paginated<T>`, `Sample`, `sakura-tui` (+27 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **3 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Message` connect `Registry Async Operations` to `Dedicated AppRun Resources`, `Container Registry API`, `Shared AppRun Services`, `Sacloud API Models`, `Credential Configuration`, `IaaS Server Control`, `Billing Data Views`, `Account Permissions`, `Common Service Resources`, `Application Navigation`, `Secret Manager`, `Monitoring Data Models`, `Credential State Transitions`, `Registry Selection State`, `Loadable View State`, `Profile Form State`, `Monitoring Interactions`?**
  _High betweenness centrality (0.173) - this node is a cross-community bridge._
- **Why does `App` connect `Registry User Actions` to `Dedicated AppRun Resources`, `Container Registry API`, `Shared AppRun Services`, `Credential Configuration`, `IaaS Server Control`, `Billing Data Views`, `Application Navigation`, `Credential State Transitions`, `Registry Async Operations`, `Pane Navigation`, `Registry Selection State`, `Loadable View State`, `Profile Form State`, `Monitoring Client API`?**
  _High betweenness centrality (0.168) - this node is a cross-community bridge._
- **Why does `Loadable` connect `Loadable View State` to `Dedicated AppRun Resources`, `Reusable UI Components`, `Shared AppRun Services`, `IaaS Server Control`, `Billing Data Views`, `Application Navigation`, `Secret Manager`, `Registry Async Operations`, `Registry Selection State`, `Registry User Actions`?**
  _High betweenness centrality (0.085) - this node is a cross-community bridge._
- **Are the 31 inferred relationships involving `accent()` (e.g. with `draw_table()` and `section_style()`) actually correct?**
  _`accent()` has 31 INFERRED edges - model-reasoned connections that need verification._
- **Are the 25 inferred relationships involving `border_style()` (e.g. with `draw_selected()` and `draw_table()`) actually correct?**
  _`border_style()` has 25 INFERRED edges - model-reasoned connections that need verification._
- **What connects `sakura-tui`, `SacloudClient`, `Paginated<T>` to the rest of the system?**
  _32 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Dedicated AppRun Resources` be split into smaller, more focused modules?**
  _Cohesion score 0.06241234221598878 - nodes in this community are weakly interconnected._