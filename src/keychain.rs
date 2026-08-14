//! レジストリのパスワードを OS のキーチェーンに預ける。
//!
//! macOS ならキーチェーン、Windows なら資格情報マネージャー、
//! Linux なら Secret Service に保存する。設定ファイルにはユーザー名だけを残し、
//! パスワードは書かない。
//!
//! キーチェーンが使えない環境（Secret Service の無いサーバなど）では
//! **平文にフォールバックせず保存自体を諦める**。保存できないことを伝えたうえで、
//! そのセッションのあいだだけログイン情報を保持する。

use anyhow::{Context, Result, bail};
use keyring::v1::Entry;

/// キーチェーン上のサービス名。
const SERVICE: &str = "sakura-tui";

/// レジストリのホスト名に対応するエントリ。
fn entry(host: &str) -> Result<Entry> {
    Entry::new(SERVICE, host).with_context(|| format!("キーチェーンを開けませんでした: {host}"))
}

/// キーチェーンが使えるか。使えない理由が分かれば返す。
pub fn availability() -> Result<()> {
    // 実在しないホスト名で作ってみて、ストアが初期化できるかだけを確かめる。
    Entry::new(SERVICE, "__probe__")
        .map(|_| ())
        .context("この環境ではOSのキーチェーンを利用できません")
}

/// パスワードを保存する。
pub fn set_password(host: &str, password: &str) -> Result<()> {
    match entry(host)?.set_password(password) {
        Ok(()) => Ok(()),
        Err(err) => Err(anyhow::anyhow!(
            "キーチェーンに保存できませんでした（{host}）: {err}{ACCESS_HINT}"
        )),
    }
}

/// 読み出しに失敗したときに添える案内。
///
/// macOS のキーチェーンは「どのアプリが作った項目か」を記録していて、別のバイナリが
/// 読もうとすると確認ダイアログが出る。ビルドし直すとバイナリが変わるため、
/// 開発中はこれに当たりやすい。拒否したりダイアログを出せなかったりすると
/// 「Platform failure」として失敗する。
#[cfg(target_os = "macos")]
const ACCESS_HINT: &str = "\n\n     macOS のキーチェーンは項目を作ったアプリを記録しています。\n\
     sakura-tui をビルドし直すとバイナリが変わるため、保存済みの項目には\n\
     アクセス確認のダイアログが出ます。\n\n\
     対処:\n\
     ・ダイアログが出たら「常に許可」を選ぶ\n\
     ・ダイアログが出ない/拒否した場合は、キーチェーンアクセス.app で\n\
       サービス名 sakura-tui の項目を削除してから作り直す\n\
     ・この TUI からは p → d で削除し、p → n で作り直せます";

#[cfg(not(target_os = "macos"))]
const ACCESS_HINT: &str = "\n\n     OS の資格情報ストアへのアクセスが拒否されました。\n\
     ロックされている場合は解除してから、もう一度お試しください。";

/// パスワードを取り出す。登録が無ければ `None`。
pub fn get_password(host: &str) -> Result<Option<String>> {
    match entry(host)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::v1::Error::NoEntry) => Ok(None),
        Err(err) => Err(anyhow::anyhow!(
            "キーチェーンから読み出せませんでした（{host}）: {err}{ACCESS_HINT}"
        )),
    }
}

/// パスワードを削除する。登録が無い場合も成功として扱う。
pub fn delete_password(host: &str) -> Result<()> {
    match entry(host)?.delete_credential() {
        Ok(()) | Err(keyring::v1::Error::NoEntry) => Ok(()),
        Err(err) => {
            Err(err).with_context(|| format!("キーチェーンから削除できませんでした: {host}"))
        }
    }
}

/// この TUI 専用の資格情報のエントリ名。
///
/// レジストリのホスト名と衝突しないよう接頭辞を付ける。
fn credential_key(name: &str) -> String {
    format!("@credential:{name}")
}

/// 旧形式（トークンとシークレットを別項目に分けていた頃）のエントリ名。
fn legacy_key(name: &str, part: &str) -> String {
    format!("@credential:{name}:{part}")
}

/// トークンとシークレットを 1 つの項目にまとめる。
///
/// 項目を分けると読み出しのたびに確認ダイアログが 2 回出るため、
/// 1 項目にまとめて 1 回で済むようにしている。
fn encode_pair(token: &str, secret: &str) -> String {
    // トークンとシークレットに改行は含まれないので、改行で区切れば十分。
    format!("{token}\n{secret}")
}

fn decode_pair(stored: &str) -> Option<(String, String)> {
    let (token, secret) = stored.split_once('\n')?;
    Some((token.to_string(), secret.to_string()))
}

/// API のトークンとシークレットを預ける。
pub fn set_api_credentials(name: &str, token: &str, secret: &str) -> Result<()> {
    set_password(&credential_key(name), &encode_pair(token, secret))
}

/// API のトークンとシークレットを取り出す。無ければ `None`。
pub fn get_api_credentials(name: &str) -> Result<Option<(String, String)>> {
    if let Some(stored) = get_password(&credential_key(name))? {
        return Ok(decode_pair(&stored));
    }
    // 旧形式で保存されたものは読めるようにしておく。
    let Some(token) = get_password(&legacy_key(name, "token"))? else {
        return Ok(None);
    };
    let Some(secret) = get_password(&legacy_key(name, "secret"))? else {
        return Ok(None);
    };
    Ok(Some((token, secret)))
}

/// API の資格情報を削除する。旧形式の項目も掃除する。
pub fn delete_api_credentials(name: &str) -> Result<()> {
    delete_password(&credential_key(name))?;
    delete_password(&legacy_key(name, "token"))?;
    delete_password(&legacy_key(name, "secret"))
}

/// レジストリのユーザーごとにパスワードを分離する（1ホストに複数ユーザーを保存するため）。
fn registry_user_key(host: &str, username: &str) -> String {
    format!("@registry-user:{host}:{username}")
}

pub fn set_registry_user_password(host: &str, username: &str, password: &str) -> Result<()> {
    set_password(&registry_user_key(host, username), password)
}

pub fn get_registry_user_password(host: &str, username: &str) -> Result<Option<String>> {
    get_password(&registry_user_key(host, username))
}

pub fn delete_registry_user_password(host: &str, username: &str) -> Result<()> {
    delete_password(&registry_user_key(host, username))
}

/// クラウドAPIの認証元ごとにIAMサービスプリンシパルの秘密鍵を分離する。
fn iam_private_key_key(profile_key: &str) -> String {
    format!("@iam-service-principal:{profile_key}")
}

pub fn set_iam_private_key(profile_key: &str, private_key: &str) -> Result<()> {
    set_password(&iam_private_key_key(profile_key), private_key)
}

pub fn get_iam_private_key(profile_key: &str) -> Result<Option<String>> {
    get_password(&iam_private_key_key(profile_key))
}

pub fn delete_iam_private_key(profile_key: &str) -> Result<()> {
    delete_password(&iam_private_key_key(profile_key))
}

/// クラウドAPIの認証元ごとにAI Engine専用トークンを分離する。
fn ai_engine_key(profile_key: &str) -> String {
    format!("@ai-engine:{profile_key}")
}

fn named_ai_engine_key(profile_key: &str, name: &str) -> String {
    format!("@ai-engine:{profile_key}:{name}")
}

pub fn get_ai_engine_token(profile_key: &str) -> Result<Option<String>> {
    get_password(&ai_engine_key(profile_key))
}

pub fn delete_ai_engine_token(profile_key: &str) -> Result<()> {
    delete_password(&ai_engine_key(profile_key))
}

pub fn set_named_ai_engine_token(profile_key: &str, name: &str, token: &str) -> Result<()> {
    set_password(&named_ai_engine_key(profile_key, name), token)
}

pub fn get_named_ai_engine_token(profile_key: &str, name: &str) -> Result<Option<String>> {
    get_password(&named_ai_engine_key(profile_key, name))
}

pub fn delete_named_ai_engine_token(profile_key: &str, name: &str) -> Result<()> {
    delete_password(&named_ai_engine_key(profile_key, name))
}

/// 平文で保存されていたパスワードをキーチェーンへ移す。
///
/// 以前のバージョンは設定ファイルにパスワードを書いていたため、
/// 起動時に見つけたら移し替えて設定ファイルからは消す。
pub fn migrate(host: &str, plaintext: &str) -> Result<()> {
    if plaintext.is_empty() {
        bail!("移行するパスワードが空です");
    }
    set_password(host, plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用のホスト名。実際のキーチェーンを汚さないよう接頭辞を付ける。
    fn test_host(name: &str) -> String {
        format!("sakura-tui-test.invalid.{name}")
    }

    /// キーチェーンが使えない環境では、テストを飛ばして落とさない。
    fn skip_if_unavailable() -> bool {
        if availability().is_err() {
            eprintln!("キーチェーンが使えないためスキップします");
            return true;
        }
        false
    }

    #[test]
    fn round_trips_a_password() {
        if skip_if_unavailable() {
            return;
        }
        let host = test_host("round-trip");
        let _ = delete_password(&host);

        assert_eq!(get_password(&host).unwrap(), None, "最初は未登録");
        set_password(&host, "s3cret").unwrap();
        assert_eq!(get_password(&host).unwrap().as_deref(), Some("s3cret"));

        // 上書きできること。
        set_password(&host, "rotated").unwrap();
        assert_eq!(get_password(&host).unwrap().as_deref(), Some("rotated"));

        delete_password(&host).unwrap();
        assert_eq!(get_password(&host).unwrap(), None, "削除後は未登録");
    }

    /// 未登録のものを消しても成功扱いにすること。
    #[test]
    fn deleting_missing_entry_is_ok() {
        if skip_if_unavailable() {
            return;
        }
        let host = test_host("missing");
        let _ = delete_password(&host);
        assert!(delete_password(&host).is_ok());
    }

    /// トークンとシークレットを組で出し入れできること。
    #[test]
    fn round_trips_api_credentials() {
        if skip_if_unavailable() {
            return;
        }
        let name = "sakura-tui-test-invalid-cred";
        let _ = delete_api_credentials(name);

        assert_eq!(get_api_credentials(name).unwrap(), None);
        set_api_credentials(name, "tok", "sec").unwrap();
        assert_eq!(
            get_api_credentials(name).unwrap(),
            Some(("tok".to_string(), "sec".to_string()))
        );

        delete_api_credentials(name).unwrap();
        assert_eq!(get_api_credentials(name).unwrap(), None);
    }

    /// レジストリのパスワードと名前空間が衝突しないこと。
    #[test]
    fn credential_keys_are_namespaced() {
        assert_eq!(credential_key("prod"), "@credential:prod");
        assert_ne!(credential_key("prod"), "prod");
        assert_eq!(ai_engine_key("prod"), "@ai-engine:prod");
        assert_eq!(
            named_ai_engine_key("prod", "batch"),
            "@ai-engine:prod:batch"
        );
        assert_ne!(ai_engine_key("prod"), credential_key("prod"));
        assert_eq!(
            registry_user_key("example.sakuracr.jp", "alice"),
            "@registry-user:example.sakuracr.jp:alice"
        );
    }

    /// レジストリの複数ユーザーを別々に出し入れできること。
    #[test]
    fn round_trips_registry_user_passwords() {
        if skip_if_unavailable() {
            return;
        }
        let host = test_host("registry-users");
        let _ = delete_registry_user_password(&host, "alice");
        let _ = delete_registry_user_password(&host, "bob");

        set_registry_user_password(&host, "alice", "a-pass").unwrap();
        set_registry_user_password(&host, "bob", "b-pass").unwrap();
        assert_eq!(
            get_registry_user_password(&host, "alice").unwrap().as_deref(),
            Some("a-pass")
        );
        assert_eq!(
            get_registry_user_password(&host, "bob").unwrap().as_deref(),
            Some("b-pass")
        );

        delete_registry_user_password(&host, "alice").unwrap();
        assert_eq!(get_registry_user_password(&host, "alice").unwrap(), None);
        assert_eq!(
            get_registry_user_password(&host, "bob").unwrap().as_deref(),
            Some("b-pass")
        );
        delete_registry_user_password(&host, "bob").unwrap();
    }

    /// 1 項目にまとめて往復できること（確認ダイアログを 1 回で済ませるため）。
    #[test]
    fn encodes_pair_in_one_entry() {
        let stored = encode_pair("tok", "sec");
        assert_eq!(decode_pair(&stored), Some(("tok".into(), "sec".into())));
    }

    /// 区切りが無い値は壊れているとみなす。
    #[test]
    fn rejects_malformed_pair() {
        assert_eq!(decode_pair("tokenonly"), None);
    }

    /// シークレット側が空でも読めること。
    #[test]
    fn allows_empty_half() {
        assert_eq!(decode_pair("tok\n"), Some(("tok".into(), String::new())));
    }

    #[test]
    fn migrate_rejects_empty_password() {
        assert!(migrate("example.sakuracr.jp", "").is_err());
    }
}
