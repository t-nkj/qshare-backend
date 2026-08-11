import "dotenv/config"
import { defineConfig } from "prisma/config"

export default defineConfig({
    schema: "prisma/schema.prisma",
    migrations: {
        path: "prisma/migrations"
    },
    datasource: {
        // Client generation does not connect to this fallback. Migrations require DATABASE_URL.
        url: process.env.DATABASE_URL ?? "mysql://placeholder:placeholder@localhost:3306/qshare"
    }
})
