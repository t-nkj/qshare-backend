import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { beforeEach, test } from "node:test"
import type { Hono } from "hono"
import { createApp } from "../src/app.js"
import type {
    AuthenticatedDevice,
    CreateDeviceInput,
    CreateUrlInput,
    DeviceRecord,
    ListUrlsInput,
    Repository,
    SharedUrlRecord
} from "../src/types.js"

interface MemoryDevice {
    id: string
    userId: string
    name: string
    tokenHash: Buffer
    createdAt: Date
    updatedAt: Date
    lastUsedAt: Date | null
}

interface MemoryUrl extends SharedUrlRecord {
    userId: string
}

class MemoryRepository implements Repository {
    readonly devices: MemoryDevice[] = []
    readonly urls: MemoryUrl[] = []

    async close(): Promise<void> {}

    async createDevice({ id, userId, name, tokenHash, now }: CreateDeviceInput): Promise<DeviceRecord> {
        this.devices.push({
            id,
            userId,
            name,
            tokenHash: Buffer.from(tokenHash),
            createdAt: now,
            updatedAt: now,
            lastUsedAt: null
        })
        return { id, name, createdAt: now.toISOString(), updatedAt: now.toISOString(), lastUsedAt: null }
    }

    async findDeviceByTokenHash(tokenHash: Buffer, now: Date): Promise<AuthenticatedDevice | null> {
        const device = this.devices.find((item) => item.tokenHash.equals(tokenHash))
        if (!device) return null
        device.lastUsedAt = now
        return { id: device.id, userId: device.userId, name: device.name }
    }

    async listDevices(userId: string): Promise<DeviceRecord[]> {
        return this.devices
            .filter((device) => device.userId === userId)
            .map((device) => ({
                id: device.id,
                name: device.name,
                createdAt: device.createdAt.toISOString(),
                updatedAt: device.updatedAt.toISOString(),
                lastUsedAt: device.lastUsedAt?.toISOString() ?? null
            }))
    }

    async renameDevice(userId: string, id: string, name: string): Promise<DeviceRecord | null> {
        const device = this.devices.find((item) => item.userId === userId && item.id === id)
        if (!device) return null
        device.name = name
        device.updatedAt = new Date(device.updatedAt.getTime() + 1)
        return {
            id: device.id,
            name,
            createdAt: device.createdAt.toISOString(),
            updatedAt: device.updatedAt.toISOString(),
            lastUsedAt: device.lastUsedAt?.toISOString() ?? null
        }
    }

    async deleteDevice(userId: string, id: string): Promise<boolean> {
        const index = this.devices.findIndex((item) => item.userId === userId && item.id === id)
        if (index < 0) return false
        this.devices.splice(index, 1)
        for (const item of this.urls) if (item.sourceDeviceId === id) item.sourceDeviceId = null
        return true
    }

    async createUrl(input: CreateUrlInput): Promise<SharedUrlRecord> {
        const result: MemoryUrl = {
            id: input.id,
            userId: input.userId,
            url: input.url,
            sourceDeviceId: input.sourceDeviceId,
            sourceDeviceName: input.sourceDeviceName,
            createdAt: input.now.toISOString(),
            expiresAt: input.expiresAt.toISOString()
        }
        this.urls.push(result)
        return withoutUser(result)
    }

    async getLatestUrl(userId: string, now: Date): Promise<SharedUrlRecord | null> {
        const item = this.urls
            .filter((url) => url.userId === userId && Date.parse(url.expiresAt) > now.getTime())
            .sort(compareNewest)[0]
        return item ? withoutUser(item) : null
    }

    async listUrls({ userId, now, limit, cursor }: ListUrlsInput): Promise<SharedUrlRecord[]> {
        return this.urls
            .filter((item) => item.userId === userId && Date.parse(item.expiresAt) > now.getTime())
            .filter(
                (item) =>
                    !cursor ||
                    Date.parse(item.createdAt) < cursor.createdAt.getTime() ||
                    (Date.parse(item.createdAt) === cursor.createdAt.getTime() && item.id < cursor.id)
            )
            .sort(compareNewest)
            .slice(0, limit + 1)
            .map(withoutUser)
    }

    async deleteUrl(userId: string, id: string): Promise<boolean> {
        const index = this.urls.findIndex((item) => item.userId === userId && item.id === id)
        if (index < 0) return false
        this.urls.splice(index, 1)
        return true
    }

    async deleteExpiredUrls(now: Date): Promise<number> {
        const retained = this.urls.filter((item) => Date.parse(item.expiresAt) > now.getTime())
        const deleted = this.urls.length - retained.length
        this.urls.splice(0, this.urls.length, ...retained)
        return deleted
    }
}

function withoutUser({ userId: _, ...item }: MemoryUrl): SharedUrlRecord {
    return item
}

function compareNewest(left: SharedUrlRecord, right: SharedUrlRecord): number {
    return Date.parse(right.createdAt) - Date.parse(left.createdAt) || right.id.localeCompare(left.id)
}

let repository: MemoryRepository
let app: Hono
let now: Date

beforeEach(() => {
    repository = new MemoryRepository()
    now = new Date("2026-08-12T00:00:00.000Z")
    app = createApp({ repository, clock: () => new Date(now) })
})

async function register(name: string, userId = "alice"): Promise<{ device: DeviceRecord; token: string }> {
    const response = await app.request("/v1/devices", {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-Forwarded-User": userId },
        body: JSON.stringify({ name })
    })
    assert.equal(response.status, 201)
    return (await response.json()) as { device: DeviceRecord; token: string }
}

function bearer(token: string, extra: Record<string, string> = {}): Record<string, string> {
    return { Authorization: `Bearer ${token}`, ...extra }
}

test("registers a device only with traQ authentication and stores only the token hash", async () => {
    const denied = await app.request("/v1/devices", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: "iPhone" })
    })
    assert.equal(denied.status, 401)
    assert.equal(((await denied.json()) as { error: { code: string } }).error.code, "TRAQ_AUTH_REQUIRED")

    const created = await register(" iPhone ")
    assert.equal(created.device.name, "iPhone")
    assert.match(created.token, /^qsh_[A-Za-z0-9_-]{43}$/)
    assert.equal(repository.devices[0]?.tokenHash.length, 32)
    assert.equal(
        repository.devices[0]?.tokenHash.toString("hex"),
        createHash("sha256").update(created.token).digest("hex")
    )
    assert.equal(created.token in created.device, false)
    assert.equal(denied.headers.get("cache-control"), "no-store")
})

test("lists, renames, isolates, and revokes devices", async () => {
    const first = await register("iPhone")
    const second = await register("Windows")
    const other = await register("iPad", "bob")

    const list = await app.request("/v1/devices", { headers: bearer(first.token) })
    const listBody = (await list.json()) as { devices: DeviceRecord[] }
    assert.deepEqual(
        listBody.devices.map((device) => device.name),
        ["iPhone", "Windows"]
    )

    const isolated = await app.request(`/v1/devices/${other.device.id}`, {
        method: "PATCH",
        headers: bearer(first.token, { "Content-Type": "application/json" }),
        body: JSON.stringify({ name: "stolen" })
    })
    assert.equal(isolated.status, 404)

    const renamed = await app.request(`/v1/devices/${second.device.id}`, {
        method: "PATCH",
        headers: bearer(first.token, { "Content-Type": "application/json" }),
        body: JSON.stringify({ name: "Desktop" })
    })
    assert.equal(renamed.status, 200)

    const removed = await app.request(`/v1/devices/${second.device.id}`, {
        method: "DELETE",
        headers: bearer(first.token)
    })
    assert.equal(removed.status, 204)
    assert.equal((await app.request("/v1/devices", { headers: bearer(second.token) })).status, 401)
})

test("shares HTTP(S) URLs, preserves the source name, and expires history after seven days", async () => {
    const device = await register("iPhone")
    const invalid = await app.request("/v1/urls", {
        method: "POST",
        headers: bearer(device.token, { "Content-Type": "application/json" }),
        body: JSON.stringify({ url: "javascript:alert(1)" })
    })
    assert.equal(invalid.status, 400)

    const created = await app.request("/v1/urls", {
        method: "POST",
        headers: bearer(device.token, { "Content-Type": "application/json" }),
        body: JSON.stringify({ url: "https://example.com/a?b=1#c" })
    })
    const shared = ((await created.json()) as { url: SharedUrlRecord }).url
    assert.equal(shared.sourceDeviceName, "iPhone")
    assert.equal(shared.expiresAt, "2026-08-19T00:00:00.000Z")

    await app.request(`/v1/devices/${device.device.id}`, {
        method: "PATCH",
        headers: bearer(device.token, { "Content-Type": "application/json" }),
        body: JSON.stringify({ name: "Renamed" })
    })
    const latest = await app.request("/v1/urls/latest", { headers: bearer(device.token) })
    assert.equal(((await latest.json()) as { url: SharedUrlRecord }).url.sourceDeviceName, "iPhone")

    now = new Date("2026-08-19T00:00:00.001Z")
    assert.equal((await app.request("/v1/urls/latest", { headers: bearer(device.token) })).status, 404)
})

test("paginates and deletes owner-scoped URL history", async () => {
    const alice = await register("Alice phone")
    const bob = await register("Bob phone", "bob")
    const ids: string[] = []

    for (let index = 0; index < 3; index += 1) {
        now = new Date(now.getTime() + 1000)
        const response = await app.request("/v1/urls", {
            method: "POST",
            headers: bearer(alice.token, { "Content-Type": "application/json" }),
            body: JSON.stringify({ url: `https://example.com/${index}` })
        })
        ids.push(((await response.json()) as { url: SharedUrlRecord }).url.id)
    }

    const first = (await (await app.request("/v1/urls?limit=2", { headers: bearer(alice.token) })).json()) as {
        urls: SharedUrlRecord[]
        nextCursor: string | null
    }
    assert.deepEqual(
        first.urls.map((url) => url.url),
        ["https://example.com/2", "https://example.com/1"]
    )
    assert.equal(typeof first.nextCursor, "string")

    const second = (await (
        await app.request(`/v1/urls?limit=2&cursor=${encodeURIComponent(first.nextCursor ?? "")}`, {
            headers: bearer(alice.token)
        })
    ).json()) as { urls: SharedUrlRecord[]; nextCursor: string | null }
    assert.deepEqual(
        second.urls.map((url) => url.url),
        ["https://example.com/0"]
    )
    assert.equal(second.nextCursor, null)

    assert.equal(
        (
            await app.request(`/v1/urls/${ids[0]}`, {
                method: "DELETE",
                headers: bearer(bob.token)
            })
        ).status,
        404
    )
    assert.equal(
        (
            await app.request(`/v1/urls/${ids[0]}`, {
                method: "DELETE",
                headers: bearer(alice.token)
            })
        ).status,
        204
    )
})

test("allows CORS only for configured token API origins", async () => {
    app = createApp({ repository, clock: () => new Date(now), corsAllowedOrigins: ["chrome-extension://known"] })
    const allowed = await app.request("/v1/urls", {
        method: "OPTIONS",
        headers: { Origin: "chrome-extension://known", "Access-Control-Request-Method": "GET" }
    })
    assert.equal(allowed.status, 204)
    assert.equal(allowed.headers.get("access-control-allow-origin"), "chrome-extension://known")

    const registration = await app.request("/v1/devices", {
        method: "OPTIONS",
        headers: { Origin: "chrome-extension://known", "Access-Control-Request-Method": "POST" }
    })
    assert.equal(registration.status, 403)
})
