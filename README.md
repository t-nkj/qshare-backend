# QShare backend

同じtraQ IDに属する複数端末でHTTP(S) URLを共有する、Rust製のJSON APIです。
端末登録時だけNeoShowcaseの`X-Forwarded-User`を使い、以後は端末ごとのBearerトークンで認証します。
登録画面などのクライアントUIは含みません。

## 技術構成

- Rust stable（現在の最小対応バージョンは1.97）
- Axum / Tokio
- MariaDB / SQLx
- rustfmt / Clippy

## API

| Method | Path | Authentication | Description |
| --- | --- | --- | --- |
| `POST` | `/v1/devices` | `X-Forwarded-User` | 端末登録・トークン発行 |
| `GET` | `/v1/devices` | Bearer | 所有端末一覧 |
| `PATCH` | `/v1/devices/{deviceId}` | Bearer | 端末名変更 |
| `DELETE` | `/v1/devices/{deviceId}` | Bearer | 端末削除・トークン失効 |
| `POST` | `/v1/urls` | Bearer | URL共有 |
| `GET` | `/v1/latest/u` | Bearer | 最新URL取得 |
| `GET` | `/v1/urls` | Bearer | 7日以内の履歴取得 |
| `DELETE` | `/v1/urls/{urlId}` | Bearer | URL削除 |
| `POST` | `/v1/memos` | Bearer | メモ追加（URL自動判定対応） |
| `GET` | `/v1/latest/m` | Bearer | 最新メモ取得 |
| `GET` | `/v1/memos` | Bearer | 7日以内のメモ履歴取得 |
| `PATCH` | `/v1/memos/{memoId}` | Bearer | メモ編集 |
| `DELETE` | `/v1/memos/{memoId}` | Bearer | メモ削除 |
| `POST` | `/v1/files` | Bearer | ファイル共有 |
| `GET` | `/v1/files` | Bearer | 3日以内のファイル履歴取得 |
| `GET` | `/v1/files/{fileId}` | Bearer | ファイル本体をダウンロード |
| `PATCH` | `/v1/files/{fileId}` | Bearer | ファイル名変更 |
| `DELETE` | `/v1/files/{fileId}` | Bearer | ファイル削除 |
| `GET` | `/v1/latest/mu` | Bearer | URL・メモを含む最終更新コンテンツを取得 |
| `GET` | `/healthz` | なし | ヘルスチェック |

`GET /v1/urls`は`limit`（既定50、最大100）とカーソルによるページングに対応します。
`GET /v1/memos`も同じページングに対応します。メモ本文は最大10,000文字です。
エラーは`{"error":{"code":"...","message":"..."}}`形式です。

`GET /v1/latest/{types}`は、`f`（ファイル）、`u`（URL）、`m`（メモ）を重複なし・順不同で指定し、その集合から最終更新が新しい1件を返します。URLは`createdAt`、メモとファイルは`updatedAt`で比較し、同時刻はメモ、URL、ファイルの順で返します。`/v1/latest`、`/v1/urls/latest`、`/v1/memos/latest`は提供しません。

```json
{
  "type": "memo",
  "memo": { "id": "...", "content": "最新のメモ" }
}
```

## メモのURL自動判定

`POST /v1/memos`は次の形式です。

```json
{
  "content": "確認して https://example.com/ と [資料](https://example.org/)",
  "autoDetectUrls": true
}
```

`autoDetectUrls`は省略時`false`です。`true`の場合、HTTP(S)の裸URLとMarkdownリンク先を出現順にURL履歴へ追加してから、元の本文のままメモを追加します。同一URLは1リクエスト中に1件だけ追加されます。

本文が前後の空白を除いて裸のHTTP(S) URLだけなら、メモは作らずURLだけを追加します。応答は常に作成順の`created`配列です。

```json
{
  "created": [
    { "type": "url", "url": { "id": "...", "url": "https://example.com/" } },
    { "type": "memo", "memo": { "id": "...", "content": "確認して https://example.com/" } }
  ]
}
```

メモは作成・編集から7日間保持されます。編集時にURL自動判定は行いません。

## ファイル共有

`POST /v1/files`はBearer認証付きの`multipart/form-data`で、`file`フィールド1つを送信します。任意形式を最大100 MiBまで受け付け、元のファイル名、MIME type、サイズ、送信元端末、作成・更新・期限日時を返します。

ファイルは3日間保持され、名前を`PATCH /v1/files/{fileId}`の`{ "name": "..." }`で変更すると、更新時刻と期限が変更から3日後へ更新されます。未期限切れファイルの合計はユーザーごとに1 GiBで、超過時には最終更新が古いファイルから自動削除されます。

ファイル本体は`GET /v1/files/{fileId}`で取得します。`Content-Disposition: attachment`を常に返すため、ブラウザ上で直接表示せずダウンロードします。

NeoShowcaseのRuntimeファイルシステムを使用するため、アプリの再起動・再デプロイ時にはファイルとその履歴がすべて消えます。

## ローカル開発

MariaDBを用意してから、接続情報を`.env`に保存します。

```sh
cp .env.example .env
cargo run
```

起動時にSQLxのマイグレーションが自動適用されます。既存のPrisma版が作成した
`devices`と`shared_urls`も同じスキーマのまま利用できます。

`.env`の例:

```dotenv
NS_MARIADB_DATABASE="qshare"
NS_MARIADB_HOSTNAME="127.0.0.1"
NS_MARIADB_PASSWORD="secret"
NS_MARIADB_PORT="3306"
NS_MARIADB_USER="qshare"
PORT=3000
CORS_ALLOWED_ORIGINS="chrome-extension://extension-id"
FILE_STORAGE_DIR="/tmp/qshare-files"
RUST_LOG="qshare_backend=info,tower_http=info"
```

接続URLはアプリ側で安全に組み立てるため、パスワードなどのURLエンコードは不要です。

## NeoShowcase

NeoShowcaseの標準Runtime BuildpackはRust/Cargoを含まないため、`Runtime Command`を使います。

- Build設定: `Runtime Command`
- Base Image: `rust:latest`
- Build Command: `cargo build --release --locked`
- Entrypoint: `./target/release/qshare-backend`
- Command: 空欄
- HTTP Port: アプリ環境変数`PORT`と同じ値
- Use MariaDB: 有効
- 部員認証: `Soft`
- 公開URL: `https://qshare.trap.show/api`
- Strip Path Prefix: 有効

ローカル・NeoShowcaseともに、以下の環境変数からMariaDB接続URLを自動生成します。

- `NS_MARIADB_DATABASE`
- `NS_MARIADB_HOSTNAME`
- `NS_MARIADB_PASSWORD`
- `NS_MARIADB_PORT`
- `NS_MARIADB_USER`

## 品質確認

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release --locked
```
