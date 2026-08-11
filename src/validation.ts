import { badRequest } from "./errors.js"
import type { SharedUrlRecord, UrlCursor } from "./types.js"

export const DEVICE_NAME_MAX_LENGTH = 64
export const URL_MAX_LENGTH = 4096

export function validateDeviceName(value: unknown): string {
    if (typeof value !== "string") {
        throw badRequest("INVALID_DEVICE_NAME", "name must be a string")
    }

    const name = value.trim()
    if (name.length === 0 || [...name].length > DEVICE_NAME_MAX_LENGTH) {
        throw badRequest("INVALID_DEVICE_NAME", `name must contain between 1 and ${DEVICE_NAME_MAX_LENGTH} characters`)
    }
    return name
}

export function validateHttpUrl(value: unknown): string {
    if (typeof value !== "string" || value.length === 0 || value.length > URL_MAX_LENGTH) {
        throw badRequest("INVALID_URL", `url must be a non-empty string of at most ${URL_MAX_LENGTH} characters`)
    }
    if (value !== value.trim()) {
        throw badRequest("INVALID_URL", "url must not have leading or trailing whitespace")
    }

    let parsed: URL
    try {
        parsed = new URL(value)
    } catch {
        throw badRequest("INVALID_URL", "url must be an absolute HTTP(S) URL")
    }

    if ((parsed.protocol !== "http:" && parsed.protocol !== "https:") || parsed.hostname.length === 0) {
        throw badRequest("INVALID_URL", "url must be an absolute HTTP(S) URL")
    }
    return value
}

export function parseLimit(value: string | undefined): number {
    if (value === undefined) return 50
    if (!/^\d+$/.test(value)) {
        throw badRequest("INVALID_LIMIT", "limit must be an integer between 1 and 100")
    }
    const limit = Number(value)
    if (limit < 1 || limit > 100) {
        throw badRequest("INVALID_LIMIT", "limit must be an integer between 1 and 100")
    }
    return limit
}

export function encodeCursor(item: Pick<SharedUrlRecord, "createdAt" | "id">): string {
    return Buffer.from(JSON.stringify({ createdAt: item.createdAt, id: item.id }), "utf8").toString("base64url")
}

export function decodeCursor(value: string | undefined): UrlCursor | null {
    if (value === undefined) return null
    try {
        const decoded: unknown = JSON.parse(Buffer.from(value, "base64url").toString("utf8"))
        if (
            typeof decoded !== "object" ||
            decoded === null ||
            !("id" in decoded) ||
            !("createdAt" in decoded) ||
            typeof decoded.id !== "string" ||
            typeof decoded.createdAt !== "string" ||
            !Number.isFinite(Date.parse(decoded.createdAt))
        ) {
            throw new Error("invalid cursor shape")
        }
        return { id: decoded.id, createdAt: new Date(decoded.createdAt) }
    } catch {
        throw badRequest("INVALID_CURSOR", "cursor is invalid")
    }
}
