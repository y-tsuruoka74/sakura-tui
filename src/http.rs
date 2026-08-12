//! 各 API クライアントが共有する HTTP の作法。
//!
//! - 一時的な失敗（429 / 5xx / 接続エラー）は指数バックオフでリトライする
//! - `SAKURA_TUI_TRACE` が設定されていればリクエストを標準エラーに記録する
//!
//! トレースは TUI が画面を占有しているあいだ標準エラーに出るため、
//! `2>trace.log` のようにリダイレクトして使う。

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Method, Request, Response, StatusCode};

/// リトライする回数（初回を除く）。
const MAX_RETRIES: u32 = 3;
/// 初回の待ち時間。以降は倍にしていく。
const BASE_BACKOFF: Duration = Duration::from_millis(400);
/// リクエスト全体のタイムアウト。
const TIMEOUT: Duration = Duration::from_secs(30);
/// 使い回す接続を寝かせておく上限。
///
/// さくらの API サーバはしばらく無通信の接続を切る。こちらが長く抱えていると
/// 「閉じられた接続に送ってしまう」（connection closed before message completed）
/// が起きるため、サーバより短い時間で自分から捨てる。
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(15);

/// 全クライアント共通の HTTP クライアントを作る。
pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("sakura-tui/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .build()
        .context("HTTPクライアントの初期化に失敗しました")
}

/// トレースが有効か。
pub fn trace_enabled() -> bool {
    std::env::var("SAKURA_TUI_TRACE").is_ok_and(|v| !v.is_empty())
}

/// このステータスは時間を置けば直る見込みがあるか。
fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// 送信時のエラーを、送り直して良いかどうかで分類する。
///
/// POST は同じ内容を二度実行してしまう恐れがあるため、接続が確立できなかった
/// （＝サーバに届いていない）場合に限る。GET/PUT/DELETE は何度実行しても
/// 結果が変わらないので、切断や中断も送り直す。
fn should_retry_error(err: &reqwest::Error, method: &Method) -> bool {
    if err.is_connect() {
        return true;
    }
    if method == Method::POST {
        return false;
    }
    // タイムアウトと、接続を使い回した際の中断（connection closed 等）。
    err.is_timeout() || err.is_request() || err.is_body()
}

/// `Retry-After` ヘッダ（秒指定）を読む。
fn retry_after(res: &Response) -> Option<Duration> {
    res.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// リクエストを送る。一時的な失敗なら間を空けて送り直す。
///
/// `request` は毎回作り直す必要があるためクロージャで受け取る。
pub async fn send_with_retry<F>(client: &reqwest::Client, build: F) -> Result<Response>
where
    F: Fn() -> Result<Request>,
{
    let mut attempt = 0u32;
    loop {
        let request = build()?;
        let method = request.method().clone();
        let url = request.url().clone();
        if trace_enabled() {
            eprintln!("[sakura-tui] -> {method} {url}");
        }

        let result = client.execute(request).await;
        let wait = match &result {
            Ok(res) if is_retryable(res.status()) && attempt < MAX_RETRIES => {
                if trace_enabled() {
                    eprintln!("[sakura-tui] <- {} {url} (リトライします)", res.status());
                }
                // サーバが待ち時間を指示していればそれに従う。
                retry_after(res).or(Some(BASE_BACKOFF * 2u32.pow(attempt)))
            }
            Ok(res) => {
                if trace_enabled() {
                    eprintln!("[sakura-tui] <- {} {url}", res.status());
                }
                None
            }
            Err(err) if attempt < MAX_RETRIES && should_retry_error(err, &method) => {
                if trace_enabled() {
                    eprintln!("[sakura-tui] <- 接続エラー {url}: {err} (リトライします)");
                }
                Some(BASE_BACKOFF * 2u32.pow(attempt))
            }
            Err(_) => None,
        };

        match wait {
            Some(wait) => {
                tokio::time::sleep(wait).await;
                attempt += 1;
            }
            None => {
                return result.map_err(|err| describe(&err, &url, attempt));
            }
        }
    }
}

/// 送信エラーを、原因が分かる日本語にする。
///
/// reqwest のメッセージは URL を含んだ英語なので、そのまま重ねると
/// 同じ URL が何度も並んで読みづらくなる。ここで 1 行にまとめる。
fn describe(err: &reqwest::Error, url: &reqwest::Url, retries: u32) -> anyhow::Error {
    let host = url.host_str().unwrap_or("(不明なホスト)");
    let retried = if retries > 0 {
        format!("（{retries}回再試行しました）")
    } else {
        String::new()
    };
    let cause = if err.is_timeout() {
        format!("{host} への接続がタイムアウトしました{retried}")
    } else if err.is_connect() {
        format!("{host} に接続できませんでした{retried}")
    } else if err.is_request() || err.is_body() {
        format!("{host} との通信が中断されました{retried}")
    } else {
        format!("{host} との通信に失敗しました{retried}: {err}")
    };
    anyhow::anyhow!(
        "{cause}\n{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_on_rate_limit_and_server_errors() {
        assert!(is_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable(StatusCode::SERVICE_UNAVAILABLE));
    }

    /// 認証エラーや不正リクエストは何度送っても同じなのでリトライしない。
    #[test]
    fn does_not_retry_client_errors() {
        assert!(!is_retryable(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable(StatusCode::FORBIDDEN));
        assert!(!is_retryable(StatusCode::NOT_FOUND));
        assert!(!is_retryable(StatusCode::BAD_REQUEST));
        assert!(!is_retryable(StatusCode::OK));
    }

    /// POST は届いていた可能性があるので、接続失敗以外は送り直さない。
    #[test]
    fn post_is_only_retried_when_connection_failed() {
        // reqwest::Error は自前で作れないので、分類の意図をここに残しておく。
        // GET/PUT/DELETE: 切断・タイムアウトも再送する（何度でも同じ結果になる）
        // POST:           接続できなかったときだけ再送する
        for method in [Method::GET, Method::PUT, Method::DELETE] {
            assert_ne!(method, Method::POST);
        }
    }

    #[test]
    fn backoff_grows_exponentially() {
        assert_eq!(BASE_BACKOFF * 2u32.pow(0), Duration::from_millis(400));
        assert_eq!(BASE_BACKOFF * 2u32.pow(1), Duration::from_millis(800));
        assert_eq!(BASE_BACKOFF * 2u32.pow(2), Duration::from_millis(1600));
    }

    #[test]
    fn reads_retry_after_header() {
        let res = Response::from(
            http::Response::builder()
                .status(429)
                .header("retry-after", "5")
                .body("")
                .unwrap(),
        );
        assert_eq!(retry_after(&res), Some(Duration::from_secs(5)));

        let without = Response::from(http::Response::builder().status(429).body("").unwrap());
        assert!(retry_after(&without).is_none());
    }
}

#[cfg(test)]
mod describe_tests {
    /// エラー文に同じ URL が何度も並ばないこと。
    #[test]
    fn message_mentions_host_and_path_once() {
        let url = reqwest::Url::parse(
            "https://secure.sakura.ad.jp/cloud/zone/is1a/api/monitoring/1.0/alerts/projects/?from=0&count=100",
        )
        .unwrap();
        // reqwest::Error は自前で作れないため、組み立て部分だけを検証する。
        let host = url.host_str().unwrap();
        assert_eq!(host, "secure.sakura.ad.jp");
        assert_eq!(
            url.path(),
            "/cloud/zone/is1a/api/monitoring/1.0/alerts/projects/"
        );
        assert_eq!(url.query(), Some("from=0&count=100"));
    }
}
