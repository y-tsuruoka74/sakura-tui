# sakura-tui

さくらインターネットのサービスをターミナルから操作する TUI（[ratatui](https://ratatui.rs) 製）。

現在は **さくらのクラウド コンテナレジストリ** に対応しています。

- レジストリの一覧と詳細（FQDN、独自ドメイン、公開設定、タグ、作成日時）
- レジストリユーザーの一覧・追加・編集・削除
- レジストリ内のイメージ（リポジトリとタグ、マニフェストダイジェスト）の閲覧

## インストール

```sh
cargo build --release
./target/release/sakura-tui
```

## 認証

### さくらのクラウド API

以下の順で探します。

1. 環境変数
   ```sh
   export SAKURA_ACCESS_TOKEN=xxxxxxxx          # SAKURACLOUD_ACCESS_TOKEN も可
   export SAKURA_ACCESS_TOKEN_SECRET=xxxxxxxx   # SAKURACLOUD_ACCESS_TOKEN_SECRET も可
   ```
2. usacloud のプロファイル（`~/.usacloud/<プロファイル名>/config.json`）

   プロファイル名は `SAKURA_PROFILE` / `SAKURACLOUD_PROFILE` / `USACLOUD_PROFILE`、
   指定がなければ `~/.usacloud/current` の内容、それも無ければ `default` を使います。
   既に `usacloud config` を済ませていれば設定不要です。

### コンテナレジストリ（イメージ一覧）

イメージ一覧はクラウド API では取得できないため、レジストリの FQDN に対して
Docker Registry HTTP API V2 を直接呼びます。そのため **クラウド API のトークンとは別に、
レジストリユーザーの ID / パスワード** が必要です。

イメージタブで `L` を押すとログインダイアログが開きます。「設定に保存」を有効にすると
`~/.config/sakura-tui/config.toml` に保存され、次回以降は自動でログインします。

> [!WARNING]
> 保存されるパスワードは平文です（ファイルのパーミッションは `0600`）。
> `O` キーで保存済みのログイン情報を破棄できます。

設定ファイルの場所は `SAKURA_TUI_CONFIG` で変更できます。書式は以下のとおりです。

```toml
[registries."example.sakuracr.jp"]
username = "your-registry-user"
password = "your-registry-password"
```

## キーバインド

`?` でいつでも一覧を表示できます。

| キー | 動作 |
| --- | --- |
| `↑` `↓` / `k` `j` | リスト内を移動 |
| `g` / `G` | 先頭 / 末尾へ |
| `PgUp` / `PgDn` | 10 件ずつ移動 |
| `←` `→` / `h` `l` | ペインの移動 |
| `Enter` | 右のペインへ入る |
| `Tab` / `Shift+Tab` | タブを切り替え |
| `1` `2` `3` | 概要 / ユーザー / イメージ |
| `r` | 表示中のデータを再取得 |
| `R` | 全キャッシュを破棄して再取得 |
| `a` `e` `d` | ユーザーの追加 / 編集 / 削除 |
| `L` | レジストリにログイン |
| `O` | レジストリのログイン情報を破棄 |
| `?` | ヘルプ |
| `q` / `Ctrl+C` | 終了 |

ユーザーの編集ではパスワードを空欄のままにすると、権限だけを変更します。

## 構成

| ファイル | 役割 |
| --- | --- |
| `src/config.rs` | 認証情報の読み込み（環境変数 / usacloud プロファイル）と設定ファイルの保存 |
| `src/sacloud.rs` | さくらのクラウド API v1.1 クライアント（`commonserviceitem` / コンテナレジストリ） |
| `src/registry.rs` | Docker Registry HTTP API V2 クライアント（Bearer / Basic 認証に対応） |
| `src/app.rs` | 画面状態、キー入力、非同期処理の起動と結果の反映 |
| `src/ui/` | 描画（一覧・詳細・ダイアログ） |

API 呼び出しは全て `tokio::spawn` して結果をチャネルで受け取るため、通信中も UI は止まりません。

## 制限

- コンテナレジストリ自体の作成・更新・削除には未対応です（ユーザー管理のみ対応）。
- コンテナレジストリはゾーンに依存しないため、常に既定ゾーン `is1a` のエンドポイントを使います。
- イメージの削除には未対応です。
