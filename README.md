# QShare backend

同じtraQ IDで使う複数端末の間で、URL・メモ・ファイルを共有するRust製APIです。登録画面などのクライアントUIは含みません。

## 環境構築

### 必要なもの

- Rust stable（最小対応: 1.97）
- MariaDB

### ローカル起動

```sh
cp .env.example .env
cargo run
```

起動時にSQLxマイグレーションが自動適用されます。MariaDB接続情報は`.env`へ設定してください。

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

`NS_MARIADB_*`から接続URLを生成するため、`DATABASE_URL`は不要です。パスワードのURLエンコードも不要です。

| 変数 | 必須 | 内容 |
| --- | --- | --- |
| `NS_MARIADB_DATABASE` | はい | MariaDBデータベース名 |
| `NS_MARIADB_HOSTNAME` | はい | MariaDBホスト名 |
| `NS_MARIADB_PASSWORD` | はい | MariaDBパスワード |
| `NS_MARIADB_PORT` | はい | MariaDBポート |
| `NS_MARIADB_USER` | はい | MariaDBユーザー名 |
| `PORT` | いいえ | HTTP待受ポート。既定値は`3000` |
| `HOST` | いいえ | HTTP待受ホスト。既定値は`0.0.0.0` |
| `CORS_ALLOWED_ORIGINS` | いいえ | 許可するOriginをカンマ区切りで指定 |
| `FILE_STORAGE_DIR` | いいえ | ファイル本体の保存先。既定値は`/tmp/qshare-files` |
| `RUST_LOG` | いいえ | ログレベル |

### NeoShowcaseへのデプロイ

- Build設定: `Runtime Command`
- Base Image: `rust:latest`
- Context: `backend`
- Build Command: `cargo build --release --locked`
- Entrypoint: `./target/release/qshare-backend`
- Command: 空欄
- HTTP Port: `PORT`と同じ値
- Use MariaDB: 有効
- 部員認証: `Soft`
- 公開URL: `https://qshare.trap.show/api`
- Strip Path Prefix: 有効

NeoShowcaseではMariaDBの自動設定済み`NS_MARIADB_*`を利用します。ファイルはRuntimeコンテナのローカルファイルシステムへ保存するため、**再起動・再デプロイ時にファイル本体とその履歴はすべて削除されます**。

### 品質確認

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release --locked
```

## API共通仕様

本番のベースURLは`https://qshare.trap.show/api`です。以降の`/v1/...`はこのベースURLからのパスです。

JSONを送るエンドポイントは`Content-Type: application/json`を指定します。端末登録以外は次の認証ヘッダーが必要です。

```http
Authorization: Bearer qsh_...
```

日時はUTCのRFC 3339形式（ミリ秒付き）で返ります。

```json
"2026-08-13T00:00:00.000Z"
```

エラーはすべて次の形式です。

```json
{
  "error": {
    "code": "INVALID_TOKEN",
    "message": "a valid device token is required"
  }
}
```

`GET /v1/urls`、`GET /v1/memos`、`GET /v1/files`は共通してカーソルページングに対応します。

| クエリ | 内容 |
| --- | --- |
| `limit` | 1〜100。省略時は50 |
| `cursor` | 前ページの`nextCursor`。次ページがない場合は`null` |

## データ形式

### Device

```json
{
  "id": "UUID",
  "name": "iPhone",
  "createdAt": "...",
  "updatedAt": "...",
  "lastUsedAt": "..."
}
```

### URL

```json
{
  "id": "UUID",
  "url": "https://example.com/",
  "sourceDeviceId": "UUID",
  "sourceDeviceName": "iPhone",
  "createdAt": "...",
  "expiresAt": "..."
}
```

URLは作成から7日間保持されます。

### Memo

```json
{
  "id": "UUID",
  "content": "メモ本文",
  "sourceDeviceId": "UUID",
  "sourceDeviceName": "iPhone",
  "createdAt": "...",
  "updatedAt": "...",
  "expiresAt": "..."
}
```

メモは作成・編集から7日間保持されます。本文は空白のみを除き、最大10,000文字です。

### File

```json
{
  "id": "UUID",
  "name": "document.pdf",
  "contentType": "application/pdf",
  "size": 12345,
  "hasThumbnail": false,
  "sourceDeviceId": "UUID",
  "sourceDeviceName": "iPhone",
  "createdAt": "...",
  "updatedAt": "...",
  "expiresAt": "..."
}
```

ファイルは作成・名前変更から3日間保持されます。ファイル1件は100 MiBまで、未期限切れの合計はユーザーごとに1 GiBまでです。超過時は更新日時が古いファイルから自動削除されます。

## エンドポイント

### `POST /v1/devices`

NeoShowcase経由で端末を登録します。`X-Forwarded-User`が必要で、通常のBearer認証は不要です。

```http
X-Forwarded-User: traq-id
```

```json
{ "name": "iPhone" }
```

端末名は1〜64文字です。成功時は`201 Created`を返します。`token`はこの応答でのみ返るため、クライアント側で安全に保存してください。

```json
{
  "device": { "id": "UUID", "name": "iPhone", "createdAt": "...", "updatedAt": "...", "lastUsedAt": null },
  "token": "qsh_..."
}
```

### `GET /v1/devices`

登録済みの自分の端末を返します。`200 OK`:

```json
{ "devices": [{ "id": "UUID", "name": "iPhone", "createdAt": "...", "updatedAt": "...", "lastUsedAt": "..." }] }
```

### `PATCH /v1/devices/{deviceId}`

端末名を変更します。本文は`POST /v1/devices`と同じです。成功時は`200 OK`:

```json
{ "device": { "id": "UUID", "name": "iPad", "createdAt": "...", "updatedAt": "...", "lastUsedAt": "..." } }
```

対象が存在しない・他人の端末の場合は`404 DEVICE_NOT_FOUND`です。

### `DELETE /v1/devices/{deviceId}`

端末を削除し、そのトークンを無効化します。成功時は`204 No Content`です。対象が存在しない・他人の端末の場合は`404 DEVICE_NOT_FOUND`です。

### `POST /v1/urls`

HTTP(S) URLを共有します。

```json
{ "url": "https://example.com/" }
```

URLは絶対HTTP(S) URL、最大4,096文字で、前後の空白は不可です。成功時は`201 Created`:

```json
{ "url": { "id": "UUID", "url": "https://example.com/", "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "expiresAt": "..." } }
```

### `GET /v1/urls`

7日以内のURL履歴を新しい順で返します。`200 OK`:

```json
{
  "urls": [{ "id": "UUID", "url": "https://example.com/", "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "expiresAt": "..." }],
  "nextCursor": null
}
```

### `DELETE /v1/urls/{urlId}`

自分のURL履歴を削除します。成功時は`204 No Content`、存在しない・他人のURLは`404 URL_NOT_FOUND`です。

### `POST /v1/memos`

メモを追加します。

```json
{
  "content": "確認して https://example.com/",
  "autoDetectUrls": true
}
```

`autoDetectUrls`は省略時`false`です。`true`では裸のHTTP(S) URLとMarkdownリンクのリンク先を出現順にURL履歴へ追加します。同じURLは1リクエストにつき1件だけ追加します。本文が前後の空白を除いて裸URLだけの場合、メモは作られません。URL作成とメモ作成は同一トランザクションです。

成功時は常に`201 Created`:

```json
{
  "created": [
    { "type": "url", "url": { "id": "UUID", "url": "https://example.com/", "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "expiresAt": "..." } },
    { "type": "memo", "memo": { "id": "UUID", "content": "確認して https://example.com/", "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "updatedAt": "...", "expiresAt": "..." } }
  ]
}
```

### `GET /v1/memos`

7日以内のメモ履歴を新しい順で返します。`200 OK`:

```json
{ "memos": [{ "id": "UUID", "content": "メモ本文", "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "updatedAt": "...", "expiresAt": "..." }], "nextCursor": null }
```

### `PATCH /v1/memos/{memoId}`

本文を更新します。

```json
{ "content": "更新した本文" }
```

URL自動判定は実行せず、URL履歴も変更しません。成功時は`200 OK`で`{ "memo": Memo }`を返し、`updatedAt`と`expiresAt`は更新時刻・更新から7日後になります。対象が存在しない・他人のメモは`404 MEMO_NOT_FOUND`です。

### `DELETE /v1/memos/{memoId}`

自分のメモを削除します。成功時は`204 No Content`、存在しない・他人のメモは`404 MEMO_NOT_FOUND`です。

### `POST /v1/files`

1件以上のファイルをアップロードします。Bearer認証と`multipart/form-data`が必要です。各ファイルのフィールド名は必ず複数形の`files`にします。同じフィールドを繰り返してください。単数形の`file`は受け付けません。

```text
files: <binary 1>
files: <binary 2>
```

任意形式を受け付けます。各ファイルは100 MiB以下、リクエスト中のファイル合計は1 GiB以下です。ファイル名は1〜255文字で、制御文字・`/`・`\`は使用できません。ファイル名・MIME typeはmultipartの情報から取得します。MIME typeがなければ`application/octet-stream`です。

ファイルごとに保存するため、失敗したファイルがあっても他のファイルは保存されます。1件以上成功した場合は`201 Created`で、入力順の`created`と失敗した項目の`failed`を返します。`index`は0始まりです。

```json
{
  "created": [
    { "id": "UUID", "name": "document.pdf", "contentType": "application/pdf", "size": 12345, "hasThumbnail": false, "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "updatedAt": "...", "expiresAt": "..." }
  ],
  "failed": [
    { "index": 1, "name": "too-large.mov", "error": { "code": "FILE_TOO_LARGE", "message": "file must not exceed 100 MiB" } }
  ]
}
```

すべて失敗した場合は通常のエラー形式を返します。不正なフィールド・ファイル名は`400`、すべてが個別サイズ超過または合計サイズ超過の場合は`413`です。合計1 GiBに達した後のファイルは`TOTAL_SIZE_EXCEEDED`として`failed`に入り、それまでに保存されたファイルは残ります。

### `GET /v1/files`

3日以内のファイルメタデータを新しい順で返します。本体は含みません。画像アップロード時に生成されたサムネイルの有無は`hasThumbnail`で確認できます。`200 OK`:

```json
{ "files": [{ "id": "UUID", "name": "document.pdf", "contentType": "application/pdf", "size": 12345, "hasThumbnail": false, "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "updatedAt": "...", "expiresAt": "..." }], "nextCursor": null }
```

### `GET /v1/files/{fileId}`

ファイル本体をダウンロードします。`200 OK`で元のMIME type、サイズ、`Content-Disposition: attachment`を返します。存在しない・他人のファイルは`404 FILE_NOT_FOUND`、Runtime再起動などで本体だけ失われた場合は`404 FILE_CONTENT_NOT_FOUND`です。

### `GET /v1/files/{fileId}/thumbnail`

画像ファイルのサムネイルを`image/webp`で返します。`hasThumbnail`が`true`のときだけ利用できます。サムネイルは長辺512px・最大512 KiBで、生成に失敗した場合でも元ファイルは保存されます。対象が存在しない・他人のファイル・サムネイルがない場合は`404 THUMBNAIL_NOT_AVAILABLE`です。

### `PATCH /v1/files/{fileId}`

表示名を変更します。

```json
{ "name": "renamed.pdf" }
```

成功時は`200 OK`で`{ "file": File }`を返します。`updatedAt`と`expiresAt`は更新時刻・更新から3日後になります。対象が存在しない・他人のファイルは`404 FILE_NOT_FOUND`です。

### `DELETE /v1/files/{fileId}`

自分のファイルメタデータと本体を削除します。成功時は`204 No Content`、存在しない・他人のファイルは`404 FILE_NOT_FOUND`です。

### `GET /v1/latest/{types}`

指定種別から最終更新が新しい1件を返します。`types`には`f`（ファイル）、`u`（URL）、`m`（メモ）を、重複なし・順不同で1文字以上指定します。

| 例 | 比較対象 |
| --- | --- |
| `/v1/latest/u` | URLのみ |
| `/v1/latest/mu` | メモとURL |
| `/v1/latest/fum` | ファイル、URL、メモ |

URLは`createdAt`、メモとファイルは`updatedAt`を比較します。同時刻は`memo > url > file`の順です。最新がURLまたはメモなら対応する単一オブジェクト、最新がファイルなら同じ`POST /v1/files`で作成された未期限切れのファイルを`files`配列で返します。成功時は`200 OK`:

```json
{ "type": "memo", "memo": { "id": "UUID", "content": "最新のメモ", "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "updatedAt": "...", "expiresAt": "..." } }
```

ファイルの場合:

```json
{ "type": "file", "files": [{ "id": "UUID", "name": "photo.jpg", "contentType": "image/jpeg", "size": 12345, "hasThumbnail": true, "sourceDeviceId": "UUID", "sourceDeviceName": "iPhone", "createdAt": "...", "updatedAt": "...", "expiresAt": "..." }] }
```

該当コンテンツがない場合は`404 LATEST_NOT_FOUND`です。無効な`types`は`400 INVALID_LATEST_TYPES`です。

### `GET /healthz`

認証不要のヘルスチェックです。`200 OK`:

```json
{ "status": "ok" }
```
