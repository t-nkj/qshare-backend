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
| `GET` | `/v1/urls/latest` | Bearer | 最新URL取得 |
| `GET` | `/v1/urls` | Bearer | 7日以内の履歴取得 |
| `DELETE` | `/v1/urls/{urlId}` | Bearer | URL削除 |
| `GET` | `/healthz` | なし | ヘルスチェック |

`GET /v1/urls`は`limit`（既定50、最大100）とカーソルによるページングに対応します。
エラーは`{"error":{"code":"...","message":"..."}}`形式です。

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
RUST_LOG="qshare_backend=info,tower_http=info"
```

接続URLはアプリ側で安全に組み立てるため、パスワードなどのURLエンコードは不要です。

## NeoShowcase

Rustは標準のパッケージ管理とビルド方法を使っているため、Runtime Buildpackを使います。

- Build設定: `Runtime Buildpack`
- Context: `.`
- Entrypoint: 空欄
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
