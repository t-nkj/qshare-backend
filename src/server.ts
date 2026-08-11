import "dotenv/config"
import { serve } from "@hono/node-server"
import { createApp } from "./app.js"
import { createPrismaRepository } from "./repository.js"

const port = Number(process.env.PORT ?? 3000)
const hostname = process.env.HOST ?? "0.0.0.0"
const repository = await createPrismaRepository()
const corsAllowedOrigins = (process.env.CORS_ALLOWED_ORIGINS ?? "")
    .split(",")
    .map((origin) => origin.trim())
    .filter(Boolean)

const app = createApp({ repository, corsAllowedOrigins })
const server = serve({ fetch: app.fetch, port, hostname }, (info) => {
    console.log(`QShare API listening on http://${info.address}:${info.port}`)
})

const cleanupInterval = setInterval(
    async () => {
        try {
            const deleted = await repository.deleteExpiredUrls(new Date())
            if (deleted > 0) console.log(`Deleted ${deleted} expired URL records`)
        } catch (error) {
            console.error("Failed to delete expired URL records", error)
        }
    },
    60 * 60 * 1000
)
cleanupInterval.unref()

try {
    await repository.deleteExpiredUrls(new Date())
} catch (error) {
    console.error("Initial expired URL cleanup failed", error)
}

async function shutdown(signal: string): Promise<void> {
    console.log(`Received ${signal}; shutting down`)
    clearInterval(cleanupInterval)
    server.close(async () => {
        await repository.close()
        process.exit(0)
    })
    setTimeout(() => process.exit(1), 10_000).unref()
}

process.on("SIGTERM", () => void shutdown("SIGTERM"))
process.on("SIGINT", () => void shutdown("SIGINT"))
