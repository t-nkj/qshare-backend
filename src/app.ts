import { createHash, randomBytes, randomUUID } from "node:crypto"
import { type Context, Hono } from "hono"
import { ApiError, badRequest } from "./errors.js"
import type { AuthenticatedDevice, Repository } from "./types.js"
import { decodeCursor, encodeCursor, parseLimit, validateDeviceName, validateHttpUrl } from "./validation.js"

const JSON_BODY_LIMIT = 16 * 1024
const TOKEN_PREFIX = "qsh_"
const RETENTION_MILLISECONDS = 7 * 24 * 60 * 60 * 1000
const TRAQ_ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/
const UUID_V4_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

interface AppOptions {
    repository: Repository
    clock?: () => Date
    corsAllowedOrigins?: string[]
}

function tokenHash(token: string): Buffer {
    return createHash("sha256").update(token, "utf8").digest()
}

async function readJson(c: Context): Promise<Record<string, unknown>> {
    const contentType = c.req.header("Content-Type") ?? ""
    if (!contentType.toLowerCase().startsWith("application/json")) {
        throw new ApiError(415, "UNSUPPORTED_MEDIA_TYPE", "Content-Type must be application/json")
    }

    const text = await c.req.text()
    if (Buffer.byteLength(text, "utf8") > JSON_BODY_LIMIT) {
        throw new ApiError(413, "PAYLOAD_TOO_LARGE", "request body is too large")
    }

    try {
        const body: unknown = JSON.parse(text)
        if (typeof body !== "object" || body === null || Array.isArray(body)) {
            throw new Error("not an object")
        }
        return body as Record<string, unknown>
    } catch {
        throw badRequest("INVALID_JSON", "request body must be a JSON object")
    }
}

function forwardedUser(c: Context): string {
    const value = c.req.header("X-Forwarded-User")
    if (!value || !TRAQ_ID_PATTERN.test(value)) {
        throw new ApiError(401, "TRAQ_AUTH_REQUIRED", "traQ authentication is required")
    }
    return value
}

async function bearerDevice(c: Context, repository: Repository, now: Date): Promise<AuthenticatedDevice> {
    const authorization = c.req.header("Authorization")
    if (!authorization?.startsWith("Bearer ")) {
        throw new ApiError(401, "INVALID_TOKEN", "a valid device token is required", {
            "WWW-Authenticate": "Bearer"
        })
    }

    const token = authorization.slice("Bearer ".length)
    if (!token.startsWith(TOKEN_PREFIX) || token.length < 20) {
        throw new ApiError(401, "INVALID_TOKEN", "a valid device token is required", {
            "WWW-Authenticate": "Bearer"
        })
    }

    const device = await repository.findDeviceByTokenHash(tokenHash(token), now)
    if (!device) {
        throw new ApiError(401, "INVALID_TOKEN", "a valid device token is required", {
            "WWW-Authenticate": "Bearer"
        })
    }
    return device
}

function requireUuid(value: string, code: string, message: string): string {
    if (!UUID_V4_PATTERN.test(value)) throw new ApiError(404, code, message)
    return value
}

export function createApp({ repository, clock = () => new Date(), corsAllowedOrigins = [] }: AppOptions): Hono {
    const app = new Hono()
    const allowedOrigins = new Set(corsAllowedOrigins)

    app.use("*", async (c, next) => {
        await next()
        c.header("X-Content-Type-Options", "nosniff")
    })

    app.use("/v1/*", async (c, next) => {
        c.header("Cache-Control", "no-store")
        const origin = c.req.header("Origin")
        const originAllowed = origin !== undefined && allowedOrigins.has(origin)

        if (c.req.method === "OPTIONS") {
            const requestedMethod = c.req.header("Access-Control-Request-Method")
            if (requestedMethod === "POST" && c.req.path === "/v1/devices") {
                throw new ApiError(403, "CORS_NOT_ALLOWED", "device registration must be same-origin")
            }
            if (!originAllowed || !origin) {
                throw new ApiError(403, "CORS_NOT_ALLOWED", "origin is not allowed")
            }
            c.header("Access-Control-Allow-Origin", origin)
            c.header("Access-Control-Allow-Methods", "GET, PATCH, POST, DELETE, OPTIONS")
            c.header("Access-Control-Allow-Headers", "Authorization, Content-Type")
            c.header("Access-Control-Max-Age", "600")
            c.header("Vary", "Origin")
            return c.body(null, 204)
        }

        await next()
        if (originAllowed && origin && !(c.req.method === "POST" && c.req.path === "/v1/devices")) {
            c.header("Access-Control-Allow-Origin", origin)
            c.header("Vary", "Origin")
        }
    })

    app.get("/healthz", (c) => c.json({ status: "ok" }))

    app.post("/v1/devices", async (c) => {
        const userId = forwardedUser(c)
        const body = await readJson(c)
        const name = validateDeviceName(body.name)
        const now = clock()
        const token = `${TOKEN_PREFIX}${randomBytes(32).toString("base64url")}`
        const device = await repository.createDevice({
            id: randomUUID(),
            userId,
            name,
            tokenHash: tokenHash(token),
            now
        })
        return c.json({ device, token }, 201)
    })

    app.get("/v1/devices", async (c) => {
        const actor = await bearerDevice(c, repository, clock())
        return c.json({ devices: await repository.listDevices(actor.userId) })
    })

    app.patch("/v1/devices/:deviceId", async (c) => {
        const actor = await bearerDevice(c, repository, clock())
        const deviceId = requireUuid(c.req.param("deviceId"), "DEVICE_NOT_FOUND", "device was not found")
        const body = await readJson(c)
        const device = await repository.renameDevice(actor.userId, deviceId, validateDeviceName(body.name))
        if (!device) throw new ApiError(404, "DEVICE_NOT_FOUND", "device was not found")
        return c.json({ device })
    })

    app.delete("/v1/devices/:deviceId", async (c) => {
        const actor = await bearerDevice(c, repository, clock())
        const deviceId = requireUuid(c.req.param("deviceId"), "DEVICE_NOT_FOUND", "device was not found")
        if (!(await repository.deleteDevice(actor.userId, deviceId))) {
            throw new ApiError(404, "DEVICE_NOT_FOUND", "device was not found")
        }
        return c.body(null, 204)
    })

    app.post("/v1/urls", async (c) => {
        const now = clock()
        const actor = await bearerDevice(c, repository, now)
        const body = await readJson(c)
        const sharedUrl = await repository.createUrl({
            id: randomUUID(),
            userId: actor.userId,
            sourceDeviceId: actor.id,
            sourceDeviceName: actor.name,
            url: validateHttpUrl(body.url),
            now,
            expiresAt: new Date(now.getTime() + RETENTION_MILLISECONDS)
        })
        return c.json({ url: sharedUrl }, 201)
    })

    app.get("/v1/urls/latest", async (c) => {
        const now = clock()
        const actor = await bearerDevice(c, repository, now)
        const sharedUrl = await repository.getLatestUrl(actor.userId, now)
        if (!sharedUrl) throw new ApiError(404, "URL_NOT_FOUND", "no unexpired URL was found")
        return c.json({ url: sharedUrl })
    })

    app.get("/v1/urls", async (c) => {
        const now = clock()
        const actor = await bearerDevice(c, repository, now)
        const limit = parseLimit(c.req.query("limit"))
        const cursor = decodeCursor(c.req.query("cursor"))
        const rows = await repository.listUrls({ userId: actor.userId, now, limit, cursor })
        const hasMore = rows.length > limit
        const urls = hasMore ? rows.slice(0, limit) : rows
        const lastUrl = urls.at(-1)
        return c.json({ urls, nextCursor: hasMore && lastUrl ? encodeCursor(lastUrl) : null })
    })

    app.delete("/v1/urls/:urlId", async (c) => {
        const actor = await bearerDevice(c, repository, clock())
        const urlId = requireUuid(c.req.param("urlId"), "URL_NOT_FOUND", "URL was not found")
        if (!(await repository.deleteUrl(actor.userId, urlId))) {
            throw new ApiError(404, "URL_NOT_FOUND", "URL was not found")
        }
        return c.body(null, 204)
    })

    app.notFound((c) =>
        c.json({ error: { code: "NOT_FOUND", message: "endpoint was not found" } }, 404, {
            "Cache-Control": "no-store"
        })
    )

    app.onError((error, c) => {
        if (error instanceof ApiError) {
            for (const [name, value] of Object.entries(error.headers)) c.header(name, value)
            c.header("Cache-Control", "no-store")
            return c.json({ error: { code: error.code, message: error.message } }, error.status)
        }
        console.error(error)
        c.header("Cache-Control", "no-store")
        return c.json({ error: { code: "INTERNAL_ERROR", message: "internal server error" } }, 500)
    })

    return app
}
