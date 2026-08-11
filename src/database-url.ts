export function getDatabaseUrl(env: NodeJS.ProcessEnv = process.env): string | undefined {
    if (env.DATABASE_URL) return env.DATABASE_URL

    const hostname = env.NS_MARIADB_HOSTNAME
    const port = env.NS_MARIADB_PORT
    const user = env.NS_MARIADB_USER
    const password = env.NS_MARIADB_PASSWORD
    const database = env.NS_MARIADB_DATABASE

    if (!hostname || !port || !user || password === undefined || !database) return undefined

    const url = new URL("mysql://localhost")
    url.hostname = hostname
    url.port = port
    url.username = user
    url.password = password
    url.pathname = `/${encodeURIComponent(database)}`
    return url.toString()
}
