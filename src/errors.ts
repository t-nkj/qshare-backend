import type { ContentfulStatusCode } from "hono/utils/http-status"

export class ApiError extends Error {
    constructor(
        readonly status: ContentfulStatusCode,
        readonly code: string,
        message: string,
        readonly headers: Record<string, string> = {}
    ) {
        super(message)
        this.name = "ApiError"
    }
}

export function badRequest(code: string, message: string): ApiError {
    return new ApiError(400, code, message)
}
