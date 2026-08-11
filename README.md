# QShare backend

同じtraQ IDに属する複数端末でHTTP(S) URLを共有する、Hono製のJSON APIです。
端末登録時だけNeoShowcaseの`X-Forwarded-User`を使い、以後は端末ごとのBearerトークンで認証します。
登録画面などのクライアントUIは含みません。

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

Node.js 24とpnpm 11を使用します。DB接続情報は`.env`に保存します。

```sh
cp .env.example .env
pnpm install
pnpm migrate
pnpm dev
```

`.env`の例:

```dotenv
DATABASE_URL="mysql://qshare:secret@127.0.0.1:3306/qshare"
PORT=3000
CORS_ALLOWED_ORIGINS="chrome-extension://extension-id"
```

`.env`はGit管理対象外です。Prisma Migrateと実行時のPrisma Clientは同じ`DATABASE_URL`を使います。

## NeoShowcase

添付のNeoShowcase資料に従い、アプリは次の設定でデプロイします。標準のPaketo Node Buildpackは
pnpm用install buildpackを含まないため、pnpm必須のこのプロジェクトではRuntime Commandを使います。

- Build設定: `Runtime Command`
- Base Image: `node:24-alpine`
- Build Command: `corepack enable && corepack prepare pnpm@11.20.0 --activate && pnpm install --frozen-lockfile && pnpm build`
- Entrypoint: `pnpm`
- Command: `start`
- HTTP Port: アプリ環境変数`PORT`と同じ値
- Use MariaDB: 有効
- 部員認証: クライアントの登録導線に合わせて`Soft`

NeoShowcaseでは`.env`を手作業でコンテナへ置いても再起動で消えるため、`.env`と同じキーをアプリの
環境変数として設定します。少なくともMariaDB接続情報をまとめた`DATABASE_URL`が必要です。

## 品質確認

```sh
pnpm format
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Biomeはダブルクオート、セミコロンなし、4スペースインデント、末尾カンマなしで設定されています。
