import { PrismaMariaDb } from "@prisma/adapter-mariadb"
import { type Device, PrismaClient, type SharedUrl } from "@prisma/client"
import { getDatabaseUrl } from "./database-url.js"
import type {
    AuthenticatedDevice,
    CreateDeviceInput,
    CreateUrlInput,
    DeviceRecord,
    ListUrlsInput,
    Repository,
    SharedUrlRecord
} from "./types.js"

function mapDevice(device: Device): DeviceRecord {
    return {
        id: device.id,
        name: device.name,
        createdAt: device.createdAt.toISOString(),
        updatedAt: device.updatedAt.toISOString(),
        lastUsedAt: device.lastUsedAt?.toISOString() ?? null
    }
}

function mapUrl(sharedUrl: SharedUrl): SharedUrlRecord {
    return {
        id: sharedUrl.id,
        url: sharedUrl.url,
        sourceDeviceId: sharedUrl.sourceDeviceId,
        sourceDeviceName: sharedUrl.sourceDeviceName,
        createdAt: sharedUrl.createdAt.toISOString(),
        expiresAt: sharedUrl.expiresAt.toISOString()
    }
}

export async function createPrismaRepository(env: NodeJS.ProcessEnv = process.env): Promise<PrismaRepository> {
    const databaseUrl = getDatabaseUrl(env)
    if (!databaseUrl) throw new Error("DATABASE_URL or NeoShowcase MariaDB environment variables are required")
    const adapter = new PrismaMariaDb(databaseUrl)
    const prisma = new PrismaClient({ adapter })
    await prisma.$connect()
    return new PrismaRepository(prisma)
}

export class PrismaRepository implements Repository {
    constructor(private readonly prisma: PrismaClient) {}

    async close(): Promise<void> {
        await this.prisma.$disconnect()
    }

    async createDevice({ id, userId, name, tokenHash, now }: CreateDeviceInput): Promise<DeviceRecord> {
        const device = await this.prisma.device.create({
            data: {
                id,
                userId,
                name,
                tokenHash: Uint8Array.from(tokenHash),
                createdAt: now,
                updatedAt: now
            }
        })
        return mapDevice(device)
    }

    async findDeviceByTokenHash(tokenHash: Buffer, now: Date): Promise<AuthenticatedDevice | null> {
        const hash = Uint8Array.from(tokenHash)
        const result = await this.prisma.device.updateMany({
            where: { tokenHash: hash },
            data: { lastUsedAt: now }
        })
        if (result.count === 0) return null

        const device = await this.prisma.device.findUnique({ where: { tokenHash: hash } })
        return device ? { id: device.id, userId: device.userId, name: device.name } : null
    }

    async listDevices(userId: string): Promise<DeviceRecord[]> {
        const devices = await this.prisma.device.findMany({
            where: { userId },
            orderBy: [{ createdAt: "asc" }, { id: "asc" }]
        })
        return devices.map(mapDevice)
    }

    async renameDevice(userId: string, id: string, name: string): Promise<DeviceRecord | null> {
        const result = await this.prisma.device.updateMany({
            where: { id, userId },
            data: { name }
        })
        if (result.count === 0) return null

        const device = await this.prisma.device.findUnique({ where: { id } })
        return device ? mapDevice(device) : null
    }

    async deleteDevice(userId: string, id: string): Promise<boolean> {
        const result = await this.prisma.device.deleteMany({ where: { id, userId } })
        return result.count > 0
    }

    async createUrl(input: CreateUrlInput): Promise<SharedUrlRecord> {
        const sharedUrl = await this.prisma.sharedUrl.create({
            data: {
                id: input.id,
                userId: input.userId,
                sourceDeviceId: input.sourceDeviceId,
                sourceDeviceName: input.sourceDeviceName,
                url: input.url,
                createdAt: input.now,
                expiresAt: input.expiresAt
            }
        })
        return mapUrl(sharedUrl)
    }

    async getLatestUrl(userId: string, now: Date): Promise<SharedUrlRecord | null> {
        const sharedUrl = await this.prisma.sharedUrl.findFirst({
            where: { userId, expiresAt: { gt: now } },
            orderBy: [{ createdAt: "desc" }, { id: "desc" }]
        })
        return sharedUrl ? mapUrl(sharedUrl) : null
    }

    async listUrls({ userId, now, limit, cursor }: ListUrlsInput): Promise<SharedUrlRecord[]> {
        const cursorFilter = cursor
            ? {
                  OR: [{ createdAt: { lt: cursor.createdAt } }, { createdAt: cursor.createdAt, id: { lt: cursor.id } }]
              }
            : {}
        const sharedUrls = await this.prisma.sharedUrl.findMany({
            where: {
                userId,
                expiresAt: { gt: now },
                ...cursorFilter
            },
            orderBy: [{ createdAt: "desc" }, { id: "desc" }],
            take: limit + 1
        })
        return sharedUrls.map(mapUrl)
    }

    async deleteUrl(userId: string, id: string): Promise<boolean> {
        const result = await this.prisma.sharedUrl.deleteMany({ where: { id, userId } })
        return result.count > 0
    }

    async deleteExpiredUrls(now: Date): Promise<number> {
        const result = await this.prisma.sharedUrl.deleteMany({ where: { expiresAt: { lte: now } } })
        return result.count
    }
}
