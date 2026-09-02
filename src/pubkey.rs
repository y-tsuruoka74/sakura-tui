//! サーバー作成フォームに入れる SSH 公開鍵の取得元。
//!
//! さくらのクラウドに登録済みの鍵は [`crate::iaas`] から引く。ここでは
//! それ以外の、手元と GitHub の鍵を扱う。

use anyhow::{Context, Result, anyhow, bail};

/// 一覧から選べる公開鍵。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    /// 一覧に出す名前。
    pub label: String,
    /// 鍵そのもの。1行。
    pub key: String,
}

impl PublicKey {
    /// 鍵の種類と末尾だけの短い表記。
    ///
    /// 公開鍵は本体が長く、一覧に並べると画面から溢れる。同じ人が複数の鍵を
    /// 持っていても見分けられる程度に、末尾とコメントだけ残す。
    pub fn summary(&self) -> String {
        let mut parts = self.key.split_whitespace();
        let Some(algo) = parts.next() else {
            return String::new();
        };
        let body = parts.next().unwrap_or("");
        let comment = parts.next().unwrap_or("");
        let tail: String = body
            .chars()
            .skip(body.chars().count().saturating_sub(12))
            .collect();
        if comment.is_empty() {
            format!("{algo} …{tail}")
        } else {
            format!("{algo} …{tail}  {comment}")
        }
    }
}

/// 公開鍵として体裁が整っているか。登録前の確認にも使う。
pub fn looks_like_public_key(line: &str) -> bool {
    is_public_key(line.trim())
}

/// 公開鍵として体裁が整っている行か。
///
/// `~/.ssh` にも GitHub にも余計な行が混ざりうるので、種類と本体が
/// 揃っているものだけ拾う。
fn is_public_key(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    let Some(algo) = parts.next() else {
        return false;
    };
    let has_body = parts.next().is_some_and(|b| b.len() >= 16);
    has_body && (algo.starts_with("ssh-") || algo.starts_with("ecdsa-") || algo.starts_with("sk-"))
}

/// 手元の `~/.ssh/*.pub` を読む。
pub fn from_local_ssh_dir() -> Result<Vec<PublicKey>> {
    let dir = dirs::home_dir()
        .context("ホームディレクトリを特定できませんでした")?
        .join(".ssh");
    let entries =
        std::fs::read_dir(&dir).with_context(|| format!("{} を読めませんでした", dir.display()))?;

    let mut keys = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "pub") {
            continue;
        }
        // 読めないファイルが1つあっても他は出したいので、失敗は飛ばす。
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        keys.extend(collect_keys(&text, |i, total| {
            if total > 1 {
                format!("{name} #{}", i + 1)
            } else {
                name.clone()
            }
        }));
    }
    keys.sort_by(|a, b| a.label.cmp(&b.label));
    if keys.is_empty() {
        bail!("{} に公開鍵(*.pub)がありませんでした", dir.display());
    }
    Ok(keys)
}

/// GitHub のユーザー名から公開鍵を取る。
///
/// `https://github.com/<名前>.keys` は認証なしで誰でも読める平文で、
/// 1行に1つ鍵が並ぶ。コメントは落とされているのでこちらで名前を付ける。
pub async fn from_github(user: &str) -> Result<Vec<PublicKey>> {
    let user = user.trim();
    validate_github_user(user)?;
    let url = format!("https://github.com/{user}.keys");
    let client = crate::http::client()?;
    let res = crate::http::send_with_retry(&client, || Ok(client.get(&url).build()?)).await?;

    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::NOT_FOUND {
        bail!("GitHub にユーザー「{user}」が見つかりませんでした");
    }
    if !status.is_success() {
        bail!("GitHub から公開鍵を取得できませんでした（HTTP {status}）");
    }

    let keys: Vec<PublicKey> = collect_keys(&text, |i, _| format!("{user} #{}", i + 1));
    if keys.is_empty() {
        bail!("GitHub のユーザー「{user}」は公開鍵を公開していません");
    }
    Ok(keys)
}

/// GitHub のユーザー名として使える文字だけか確かめる。
///
/// そのまま URL に埋めるので、`../` のような文字を弾いておく。
fn validate_github_user(user: &str) -> Result<()> {
    if user.is_empty() {
        return Err(anyhow!("GitHub のユーザー名を入力してください"));
    }
    if user.len() > 39 || !user.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(anyhow!("「{user}」は GitHub のユーザー名として使えません"));
    }
    Ok(())
}

/// 複数行のテキストから公開鍵を拾い、通し番号付きで名前を付ける。
fn collect_keys(text: &str, label: impl Fn(usize, usize) -> String) -> Vec<PublicKey> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| is_public_key(l))
        .collect();
    let total = lines.len();
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| PublicKey {
            label: label(i, total),
            key: line.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_only_lines_that_look_like_keys() {
        let text = "\
# コメント行

ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample1 me@example
ssh-rsa short
ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQExample2
";
        let keys = collect_keys(text, |i, _| format!("#{i}"));
        assert_eq!(keys.len(), 2);
        assert!(keys[0].key.starts_with("ssh-ed25519"));
        assert!(keys[1].key.starts_with("ssh-rsa AAAAB3"));
    }

    /// 1つしかないファイルには通し番号を付けない。
    #[test]
    fn numbers_keys_only_when_there_are_several() {
        let one = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample1";
        let keys = collect_keys(one, |i, total| {
            if total > 1 {
                format!("鍵 #{}", i + 1)
            } else {
                "鍵".to_string()
            }
        });
        assert_eq!(keys[0].label, "鍵");
    }

    #[test]
    fn summary_keeps_the_type_and_the_tail() {
        let key = PublicKey {
            label: "id_ed25519.pub".to_string(),
            key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample1 me@example".to_string(),
        };
        assert_eq!(key.summary(), "ssh-ed25519 …AAAIExample1  me@example");
    }

    /// URL に埋める前にユーザー名を確かめる。
    #[test]
    fn rejects_user_names_that_would_change_the_url() {
        assert!(validate_github_user("octocat").is_ok());
        assert!(validate_github_user("oct-cat-1").is_ok());
        assert!(validate_github_user("").is_err());
        assert!(validate_github_user("../../etc").is_err());
        assert!(validate_github_user("a/b").is_err());
        assert!(validate_github_user("名前").is_err());
    }
}
