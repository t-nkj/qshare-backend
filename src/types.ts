export interface DeviceRecord {
    id: string
    name: string
    createdAt: string
    updatedAt: string
    lastUsedAt: string | null
}

export interface AuthenticatedDevice {
    id: string
    userId: string
    name: string
}

export interface SharedUrlRecord {
    id: string
    url: string
    sourceDeviceId: string | null
    sourceDeviceName: string
    createdAt: string
    expiresAt: string
}

export interface UrlCursor {
    id: string
    createdAt: Date
}

export interface CreateDeviceInput {
    id: string
    userId: string
    name: string
    tokenHash: Buffer
    now: Date
}

export interface CreateUrlInput {
    id: string
    userId: string
    sourceDeviceId: string
    sourceDeviceName: string
    url: string
    now: Date
    expiresAt: Date
}

export interface ListUrlsInput {
    userId: string
    now: Date
    limit: number
    cursor: UrlCursor | null
}

export interface Repository {
    createDevice(input: CreateDeviceInput): Promise<DeviceRecord>
    findDeviceByTokenHash(tokenHash: Buffer, now: Date): Promise<AuthenticatedDevice | null>
    listDevices(userId: string): Promise<DeviceRecord[]>
    renameDevice(userId: string, id: string, name: string): Promise<DeviceRecord | null>
    deleteDevice(userId: string, id: string): Promise<boolean>
    createUrl(input: CreateUrlInput): Promise<SharedUrlRecord>
    getLatestUrl(userId: string, now: Date): Promise<SharedUrlRecord | null>
    listUrls(input: ListUrlsInput): Promise<SharedUrlRecord[]>
    deleteUrl(userId: string, id: string): Promise<boolean>
    deleteExpiredUrls(now: Date): Promise<number>
    close(): Promise<void>
}
