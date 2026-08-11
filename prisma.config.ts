import "dotenv/config"
import { defineConfig } from "prisma/config"
import { getDatabaseUrl } from "./src/database-url.js"

export default defineConfig({
    schema: "prisma/schema.prisma",
    migrations: {
        path: "prisma/migrations"
    },
    datasource: {
        // Client generation does not connect to this fallback. Migrations require a real database URL.
        url: getDatabaseUrl() ?? "mysql://placeholder:placeholder@localhost:3306/qshare"
    }
})
