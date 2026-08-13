use std::{env, net::SocketAddr, path::PathBuf};

use url::Url;

use crate::error::StartupError;

pub struct Config {
    pub address: SocketAddr,
    pub database_url: String,
    pub cors_allowed_origins: Vec<String>,
    pub file_storage_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self, StartupError> {
        let port = env::var("PORT")
            .unwrap_or_else(|_| "3000".to_owned())
            .parse::<u16>()
            .map_err(|error| StartupError::Config(format!("PORT is invalid: {error}")))?;
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_owned());
        let address = format!("{host}:{port}")
            .parse()
            .map_err(|error| StartupError::Config(format!("HOST is invalid: {error}")))?;
        let database_url = database_url_from_env()?;
        let cors_allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(str::to_owned)
            .collect();
        let file_storage_dir = env::var("FILE_STORAGE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp/qshare-files"));

        Ok(Self {
            address,
            database_url,
            cors_allowed_origins,
            file_storage_dir,
        })
    }
}

fn database_url_from_env() -> Result<String, StartupError> {
    let required = |name: &'static str| env::var(name).map_err(|_| StartupError::Config(format!("{name} is required")));
    let hostname = required("NS_MARIADB_HOSTNAME")?;
    let port = required("NS_MARIADB_PORT")?;
    let user = required("NS_MARIADB_USER")?;
    let password = required("NS_MARIADB_PASSWORD")?;
    let database = required("NS_MARIADB_DATABASE")?;

    let mut url = Url::parse("mysql://localhost").expect("static database URL must be valid");
    url.set_host(Some(&hostname))
        .map_err(|_| StartupError::Config("NS_MARIADB_HOSTNAME is invalid".to_owned()))?;
    url.set_port(Some(port.parse().map_err(|error| {
        StartupError::Config(format!("NS_MARIADB_PORT is invalid: {error}"))
    })?))
    .map_err(|_| StartupError::Config("NS_MARIADB_PORT is invalid".to_owned()))?;
    url.set_username(&user)
        .map_err(|_| StartupError::Config("NS_MARIADB_USER is invalid".to_owned()))?;
    url.set_password(Some(&password))
        .map_err(|_| StartupError::Config("NS_MARIADB_PASSWORD is invalid".to_owned()))?;
    url.set_path(&database);
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_encoded_database_url() {
        let mut url = Url::parse("mysql://localhost").unwrap();
        url.set_host(Some("mariadb.internal")).unwrap();
        url.set_port(Some(3306)).unwrap();
        url.set_username("qshare@example").unwrap();
        url.set_password(Some("p@ss/word")).unwrap();
        url.set_path("qshare");

        assert_eq!(
            url.as_str(),
            "mysql://qshare%40example:p%40ss%2Fword@mariadb.internal:3306/qshare"
        );
    }
}
