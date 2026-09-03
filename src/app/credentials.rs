//! 認証情報とログインの操作。
//!
//! クラウドAPIの資格情報（環境変数・usacloud・キーチェーン）の切り替えと作成、
//! AI Engine のアカウントトークン、IAM サービスプリンシパル、
//! コンテナレジストリへのログインをここにまとめる。
//! サービスごとの閲覧・操作は各サービスのモジュールが持つ。

use super::*;

impl App {
    /// 保存済みのログイン情報があれば自動でクライアントを作る。
    ///
    /// パスワードの取り出しはキーチェーンに触るため別スレッドで行う。
    /// UI スレッドで呼ぶと、OS の確認ダイアログが出ている間 TUI が固まる。
    pub(super) fn try_auto_login(&mut self, host: &str) {
        // 一度試したホストは再試行しない。
        //
        // `ensure_loaded` はキー入力とメッセージのたびに走るため、ここで印を
        // 付けないと失敗するたびに読み直してしまう。キーチェーンは読むたびに
        // OS の確認ダイアログを出しうるので、それが延々と繰り返される。
        if !self.registry.auto_login_tried.insert(host.to_string()) {
            return;
        }
        if !self.config.registries.contains_key(host) {
            return;
        }
        self.registry
            .repositories
            .insert(host.to_string(), Loadable::Loading);
        self.inflight += 1;
        let config = self.config.clone();
        let tx = self.tx.clone();
        let host = host.to_string();
        tokio::task::spawn_blocking(move || {
            let login = config.registry_login(&host);
            let _ = tx.send(Message::SavedLogin { host, login });
        });
    }

    pub(super) fn open_iam_credential_form(&mut self) {
        let credentials = match crate::config::load_iam_credentials(&self.credential_source) {
            Ok(credentials) => credentials,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                None
            }
        };
        self.overlay = Some(Overlay::IamCredentialForm(match credentials {
            Some(credentials) => IamCredentialForm {
                service_principal_id: credentials.service_principal_id,
                key_id: credentials.key_id,
                private_key: credentials.private_key,
                ..IamCredentialForm::default()
            },
            None => IamCredentialForm::default(),
        }));
    }

    pub(super) fn submit_iam_credentials(&mut self, mut form: IamCredentialForm) {
        let credentials = form.credentials();
        if credentials.service_principal_id.is_empty()
            || credentials.key_id.is_empty()
            || credentials.private_key.is_empty()
        {
            self.set_status(
                "リソースID、キーID、RSA秘密鍵をすべて入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::IamCredentialForm(form));
            return;
        }
        if !credentials.private_key.contains("-----BEGIN")
            || !credentials.private_key.contains("PRIVATE KEY-----")
        {
            self.set_status("PEM形式のRSA秘密鍵を貼り付けてください", StatusKind::Error);
            self.overlay = Some(Overlay::IamCredentialForm(form));
            return;
        }
        form.verifying = true;
        self.overlay = Some(Overlay::IamCredentialForm(form.clone()));
        self.inflight += 1;
        self.set_status("IAMサービスプリンシパルを検証しています…", StatusKind::Info);
        let client = self.sacloud.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client
                .verify_iam_credentials(&credentials)
                .await
                .map_err(fmt_error);
            let _ = tx.send(Message::IamCredentialsVerified {
                form: Box::new(form),
                result,
            });
        });
    }

    pub(super) fn open_profile_picker(&mut self) {
        let sources = crate::config::available_credential_sources();
        let index = sources
            .iter()
            .position(|s| *s == self.credential_source)
            .unwrap_or(0);
        let sources = sources
            .into_iter()
            .map(|source| {
                let zone = source.zone();
                (source, zone)
            })
            .collect();
        self.overlay = Some(Overlay::ProfilePicker { sources, index });
    }

    /// 認証情報が無い初回起動を、既存のプロファイル作成フォームへつなぐ。
    pub fn start_credential_setup(&mut self) {
        self.set_status(
            "認証情報が見つかりません。アプリ内で新しいプロファイルを作成してください",
            StatusKind::Info,
        );
        self.open_profile_form();
    }

    /// キーチェーンに預けた資格情報の削除を確認する。
    pub(super) fn confirm_delete_credential(&mut self, source: &CredentialSource) {
        let CredentialSource::Keychain(name) = source else {
            self.set_status(
                "削除できるのはキーチェーンに保存したものだけです（usacloud のプロファイルは他のツールも使うため消しません）",
                StatusKind::Info,
            );
            return;
        };
        if *source == self.credential_source {
            self.set_status("使用中の資格情報は削除できません", StatusKind::Info);
            return;
        }
        self.overlay = Some(Overlay::Confirm {
            title: "資格情報の削除".to_string(),
            body: format!(
                "「{name}」をキーチェーンと設定ファイルから削除します。\n\
                 アクセストークン自体はさくらのコントロールパネルに残ります。"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteCredential { name: name.clone() },
        });
    }

    /// 資格情報の作成フォームを開く。
    pub(super) fn open_profile_form(&mut self) {
        // 接続先は本番と社内テストから選ぶ。他の環境は --api-root で指定する。
        let current = self.api_root.clone();
        let mut api_roots = vec![
            ApiRootChoice {
                label: "本番 (cloud)",
                url: crate::config::DEFAULT_API_ROOT.to_string(),
            },
            ApiRootChoice {
                label: "テスト (cloud-test)",
                url: crate::config::TEST_API_ROOT.to_string(),
            },
        ];
        // 起動時に別の接続先を指定していれば、それも選べるようにする。
        if !api_roots.iter().any(|r| r.url == current) {
            api_roots.push(ApiRootChoice {
                label: "起動時の指定",
                url: current.clone(),
            });
        }
        let api_root_index = api_roots.iter().position(|r| r.url == current).unwrap_or(0);

        // ゾーンは接続先に対応するものを出す。
        // 既に API から取れていればそちらを優先する（環境の実態に一番近い）。
        let zones = match self.zones.ready() {
            Some(zones) if !zones.is_empty() => zones.clone(),
            _ => crate::iaas::known_zones_for(&current),
        };
        let zone_index = zones.iter().position(|z| z.name == self.zone).unwrap_or(0);

        self.overlay = Some(Overlay::ProfileForm(ProfileForm {
            zones,
            zone_index,
            api_roots,
            api_root_index,
            ..ProfileForm::default()
        }));
    }

    pub(super) fn open_ai_engine_token_form(&mut self) {
        let entries = match crate::config::list_ai_engine_tokens(&self.credential_source) {
            Ok(entries) => entries,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                Vec::new()
            }
        };
        let index = entries.iter().position(|entry| entry.active).unwrap_or(0);
        self.overlay = Some(Overlay::AiEngineTokenForm(AiEngineTokenForm {
            entries,
            index,
            ..AiEngineTokenForm::default()
        }));
    }

    pub(super) fn submit_ai_engine_token(&mut self, mut form: AiEngineTokenForm) {
        if let Err(err) = crate::config::validate_ai_engine_token_name(&form.name) {
            self.set_status(fmt_error(err), StatusKind::Error);
            self.overlay = Some(Overlay::AiEngineTokenForm(form));
            return;
        }
        let name = form.name.trim().to_string();
        let valid_shape = form
            .token
            .split_once(':')
            .is_some_and(|(id, secret)| !id.trim().is_empty() && !secret.trim().is_empty());
        if !valid_shape {
            self.set_status(
                "アカウントトークンを UUID:シークレット の形式で入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::AiEngineTokenForm(form));
            return;
        }
        let token = form.token.trim().to_string();
        let client = match AiEngineClient::new(token.clone()) {
            Ok(client) => Arc::new(client),
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                self.overlay = Some(Overlay::AiEngineTokenForm(form));
                return;
            }
        };
        form.verifying = true;
        self.overlay = Some(Overlay::AiEngineTokenForm(form));
        self.inflight += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = client.list_models().await.map_err(fmt_error);
            let _ = tx.send(Message::AiEngineTokenVerified {
                name,
                token,
                result,
            });
        });
    }

    pub(super) fn select_ai_engine_token(&mut self, name: &str) {
        let token = match crate::config::select_ai_engine_token(&self.credential_source, name) {
            Ok(Some(token)) => token,
            Ok(None) => {
                self.set_status("選択したトークンを読み出せませんでした", StatusKind::Error);
                return;
            }
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                return;
            }
        };
        match AiEngineClient::new(token) {
            Ok(client) => {
                self.ai_engine_client = Some(Arc::new(client));
                self.managed_resources
                    .items
                    .remove(&ManagedResourceKind::AiEngine);
                self.ai_engine_reset_rag();
                self.overlay = None;
                self.set_status(
                    format!("AI Engineトークン「{name}」へ切り替えました"),
                    StatusKind::Success,
                );
                self.managed_resources_ensure_loaded();
            }
            Err(err) => self.set_status(fmt_error(err), StatusKind::Error),
        }
    }

    pub(super) fn confirm_delete_ai_engine_token(&mut self, name: String) {
        self.overlay = Some(Overlay::Confirm {
            title: "AI Engineトークンの削除".to_string(),
            body: format!(
                "このPCのキーチェーンからAI Engineトークン「{name}」を削除します。\n\
                 AI Engine側のトークンは失効しません。失効はコントロールパネルで行ってください。"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::DeleteAiEngineToken { name },
        });
    }

    pub(super) fn copy_ai_engine_token(&mut self, name: &str, form: AiEngineTokenForm) {
        match crate::config::load_named_ai_engine_token(&self.credential_source, name) {
            Ok(Some(token)) => match copy_to_clipboard(&token) {
                Ok(()) => self.set_status(
                    format!("AI Engineトークン「{name}」をクリップボードへコピーしました"),
                    StatusKind::Success,
                ),
                Err(err) => self.set_status(
                    format!("クリップボードへコピーできませんでした: {err}"),
                    StatusKind::Error,
                ),
            },
            Ok(None) => {
                self.set_status("コピーできる保存済みトークンがありません", StatusKind::Info)
            }
            Err(err) => self.set_status(fmt_error(err), StatusKind::Error),
        }
        self.overlay = Some(Overlay::AiEngineTokenForm(form));
    }

    /// 入力内容を検証してから保存する。
    ///
    /// 打ち間違えたトークンを保存してしまわないよう、実際に API を 1 回叩いて
    /// 通ることを確かめてから書き出す。
    pub(super) fn submit_profile_form(&mut self, mut form: ProfileForm) {
        if let Err(err) = crate::config::validate_profile_name(&form.name) {
            self.set_status(fmt_error(err), StatusKind::Error);
            self.overlay = Some(Overlay::ProfileForm(form));
            return;
        }
        // 見えない文字が混ざっていると 401 になるので、ここで落としてから使う。
        form.token = crate::config::clean_secret(&form.token);
        form.secret = crate::config::clean_secret(&form.secret);
        if form.token.is_empty() || form.secret.is_empty() {
            self.set_status(
                "アクセストークンとシークレットを入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::ProfileForm(form));
            return;
        }

        let credentials = crate::config::ApiCredentials {
            token: form.token.clone(),
            secret: form.secret.clone(),
            source: CredentialSource::Env,
            zone: Some(form.zone().name.clone()),
            api_root: Some(form.api_root().url.clone()),
        };
        let client = match SacloudClient::new(&credentials) {
            Ok(client) => client,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                self.overlay = Some(Overlay::ProfileForm(form));
                return;
            }
        };

        form.verifying = true;
        self.overlay = Some(Overlay::ProfileForm(form.clone()));
        self.inflight += 1;
        self.set_status("トークンを検証しています…", StatusKind::Info);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            // 「自分が誰か」を返すだけの auth-status で確かめる。
            // ゾーン一覧やリソース一覧は権限設定によっては読めないため、
            // 有効なキーでも失敗してしまう。
            let result = match client.billing_identity().await {
                // 認証が通ったら、その環境に実在するゾーンも拾っておく。
                // 環境ごとにゾーン名が違うため、以降はこれを使う。
                Ok(_) => Ok(client.list_zones().await.unwrap_or_default()),
                Err(err) => Err(fmt_error(err)),
            };
            let _ = tx.send(Message::ProfileVerified {
                form: Box::new(form),
                result,
            });
        });
    }

    /// 検証が通った資格情報を保存する。
    pub(super) fn save_verified_profile(&mut self, form: ProfileForm) {
        let saved = match form.storage {
            ProfileStorage::Usacloud => crate::config::create_usacloud_profile(
                &form.name,
                &form.token,
                &form.secret,
                &form.zone().name,
                &form.api_root().url,
            ),
            ProfileStorage::Keychain => crate::config::create_keychain_credential(
                &form.name,
                &form.token,
                &form.secret,
                &form.zone().name,
                &form.api_root().url,
            ),
        };
        match saved {
            Ok(path) => {
                // 保存先の設定を読み直してから、一覧に反映した状態でピッカーへ戻る。
                if form.storage == ProfileStorage::Keychain
                    && let Ok(config) = Config::load()
                {
                    self.config = config;
                }
                let created_message = format!(
                    "{} を作成しました（{}）: {}",
                    form.name,
                    form.storage.title(),
                    path.display()
                );
                if !self.has_credentials {
                    let source = match form.storage {
                        ProfileStorage::Usacloud => CredentialSource::Profile(form.name.clone()),
                        ProfileStorage::Keychain => CredentialSource::Keychain(form.name.clone()),
                    };
                    let zone = form.zone().name.clone();
                    let api_root = form.api_root().url.clone();
                    let credentials = ApiCredentials {
                        token: form.token,
                        secret: form.secret,
                        source: source.clone(),
                        zone: Some(zone),
                        api_root: Some(api_root),
                    };
                    if self.apply_credentials(source, credentials) {
                        self.set_status(created_message, StatusKind::Success);
                        self.open_initial_service_picker();
                    }
                } else {
                    self.set_status(created_message, StatusKind::Success);
                    self.open_profile_picker();
                }
            }
            Err(err) => {
                self.pending_form = Some(Box::new(form));
                self.overlay = Some(Overlay::Message {
                    title: "作成に失敗しました".to_string(),
                    body: format!(
                        "{}\n\n閉じると入力内容を残したままフォームに戻ります。",
                        fmt_error(err)
                    ),
                    kind: StatusKind::Error,
                    scroll: 0,
                });
            }
        }
    }

    /// 認証情報に割り当てる色を順に切り替えて保存する。
    ///
    /// dev と prod のように名前が似ている契約を、自分で決めた色で
    /// 見分けられるようにするためのもの。既定色に戻すところまで一巡する。
    pub(super) fn cycle_profile_color(&mut self, source: &CredentialSource) {
        let palette = crate::ui::PROFILE_COLORS;
        let next = match self.config.profile_color(source) {
            None => Some(palette[0].to_string()),
            Some(current) => palette
                .iter()
                .position(|c| *c == current)
                .map(|i| i + 1)
                .filter(|i| *i < palette.len())
                .map(|i| palette[i].to_string()),
        };
        self.config.set_profile_color(source, next.clone());

        match self.config.save() {
            Ok(_) => {
                let label = next.as_deref().unwrap_or("既定");
                self.set_status(
                    format!("{} の色を {label} にしました", source.label()),
                    StatusKind::Success,
                );
            }
            Err(err) => self.set_status(
                format!("設定の保存に失敗しました: {}", fmt_error(err)),
                StatusKind::Error,
            ),
        }
    }

    /// 認証情報の読み込みを別スレッドで始める。
    ///
    /// キーチェーンの読み出しは OS が確認ダイアログを出すことがあり、
    /// その間ブロックする。UI スレッドで呼ぶと TUI ごと固まるため切り離す。
    pub(super) fn switch_credentials(&mut self, source: CredentialSource) {
        if source == self.credential_source {
            return;
        }
        self.inflight += 1;
        self.set_status(
            format!("{} の認証情報を読み込んでいます…", source.label()),
            StatusKind::Info,
        );
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = crate::config::load_credentials_from(&source).map_err(fmt_error);
            let _ = tx.send(Message::CredentialsLoaded {
                source: Box::new(source),
                result: Box::new(result),
            });
        });
    }

    /// 読み込めた認証情報に切り替え、クラウド API 側のキャッシュを捨てて読み直す。
    ///
    /// レジストリへのログインはホスト単位でクラウドの契約とは独立なので保持する。
    pub(super) fn apply_credentials(
        &mut self,
        source: CredentialSource,
        credentials: ApiCredentials,
    ) -> bool {
        let was_configured = self.has_credentials;
        // 次回の起動でここから再開できるようにする。
        // 保存に失敗しても切り替え自体は続ける（見た目の設定と同じ扱い）。
        let _ = crate::config::remember_last_credential(&source);
        // 世代を進める。前の資格情報で投げた通信の結果は、
        // これ以降に届いても画面に入らない。
        self.epoch += 1;
        self.tx.epoch = self.epoch;
        // 各サービスのクライアントを作り直す。
        let clients = (
            SacloudClient::new(&credentials),
            AppRunClient::new(&credentials),
            DedicatedClient::new(&credentials),
            MonitoringClient::new(&credentials),
            ApiGatewayClient::new(&credentials),
            AiEngineCloudClient::new(&credentials),
        );
        let (sacloud, apprun, dedicated, monitoring, api_gateway, ai_engine_cloud) = match clients {
            (Ok(a), Ok(b), Ok(c), Ok(d), Ok(e), Ok(f)) => (a, b, c, d, e, f),
            _ => {
                self.show_error(
                    "クライアントを初期化できませんでした",
                    format!("{} への切り替えを中止しました", source.label()),
                );
                return false;
            }
        };

        self.api_root = credentials.api_root().to_string();
        // ゾーン名は環境ごとに違う（本番の is1a は cloud-test には無い）。
        // 切り替え先の既定ゾーンに合わせないと、ゾーン依存のサービスが全て 404 になる。
        self.zone = sacloud.default_zone().to_string();

        self.sacloud = Arc::new(sacloud);
        self.apprun_client = Arc::new(apprun);
        self.dedicated_client = Arc::new(dedicated);
        self.monitoring_client = Arc::new(monitoring);
        self.api_gateway_client = Arc::new(api_gateway);
        self.ai_engine_client = None;
        self.ai_engine_cloud_client = Arc::new(ai_engine_cloud);
        self.credential_source = source;
        self.has_credentials = true;

        // 契約が変われば、取得済みのものは全て別アカウントのもの。
        // どれか一つでも残すと、切り替えたのに前の内容が見える。
        self.zones = Loadable::Idle;
        self.zone_counts.clear();
        self.service_counts.clear();
        self.invalidate_all();
        self.registry.registries = Loadable::Idle;
        self.registry_clients = RegistryClients::default();
        self.filters = Filters::default();

        self.set_status(
            format!(
                "{} に切り替えました（ゾーン {}）",
                self.credential_source.label(),
                self.zone
            ),
            StatusKind::Info,
        );
        // 表示中のサービスを読み直す。レジストリだけ読むと、他のサービスに
        // 移ったときに前のアカウントの内容が残って見える。
        if was_configured {
            self.ensure_loaded();
        }
        true
    }

    pub(super) fn open_login(&mut self) {
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let host = registry.host().to_string();
        if host.is_empty() {
            self.set_status(
                "このレジストリにはホスト名が割り当てられていません",
                StatusKind::Error,
            );
            return;
        }
        self.registry.tab = Tab::Images;
        self.registry.focus = Focus::Detail;
        let accounts = self.config.registry_account_names(&host);
        if accounts.is_empty() {
            self.overlay = Some(Overlay::Login(LoginForm {
                username: String::new(),
                password: String::new(),
                save: false,
                host,
                field: 0,
            }));
        } else {
            self.overlay = Some(Overlay::LoginPicker {
                host,
                accounts,
                index: 0,
            });
        }
    }

    /// 保存済みのユーザー名を選んでログインする。パスワードの取り出しは
    /// キーチェーンに触るため別スレッドで行う。
    pub(super) fn login_with_saved_account(&mut self, host: String, username: String) {
        // これから試すので、前に「試した」印がついていても関係ない。
        self.registry.auto_login_tried.insert(host.clone());
        self.set_status(format!("{host} に接続中…"), StatusKind::Info);
        let config = self.config.clone();
        let tx = self.tx.clone();
        self.inflight += 1;
        tokio::task::spawn_blocking(move || {
            let login = config.registry_user_login(&host, &username);
            let _ = tx.send(Message::SavedLogin { host, login });
        });
    }

    pub(super) fn confirm_forget_login(&mut self) {
        let Some(registry) = self.selected_registry() else {
            return;
        };
        let host = registry.host().to_string();
        if self.registry_clients.get(&host).is_none() && !self.config.registries.contains_key(&host)
        {
            return;
        }
        self.overlay = Some(Overlay::Confirm {
            title: "ログイン情報の削除".to_string(),
            body: format!(
                "{host} のログイン情報を破棄します。\n設定ファイルに保存済みの場合はそこからも削除されます。"
            ),
            verify: None,
            typed: String::new(),
            action: ConfirmAction::ForgetLogin { host },
        });
    }

    pub(super) fn submit_login(&mut self, form: LoginForm) {
        if form.username.is_empty() || form.password.is_empty() {
            self.set_status(
                "ユーザー名とパスワードを入力してください",
                StatusKind::Error,
            );
            self.overlay = Some(Overlay::Login(form));
            return;
        }
        let login = RegistryLogin {
            username: form.username,
            password: form.password,
        };
        let client = match self.registry_clients.insert(&form.host, login.clone()) {
            Ok(client) => client,
            Err(err) => {
                self.set_status(fmt_error(err), StatusKind::Error);
                return;
            }
        };
        let host = form.host;
        let save = form.save;
        let tx = self.tx.clone();
        self.inflight += 1;
        self.set_status(format!("{host} に接続中…"), StatusKind::Info);
        tokio::spawn(async move {
            let result = client.verify().await.map_err(fmt_error);
            let _ = tx.send(Message::LoginVerified {
                host,
                login,
                save,
                result,
            });
        });
    }
}
