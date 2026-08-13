use std::{sync::Arc, time::Duration};

use chrono::Utc;
use qshare_backend::{
    app::{AppState, create_app},
    config::Config,
    error::StartupError,
    repository::{MySqlRepository, Repository},
};
use tokio::{net::TcpListener, signal, time};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = Config::from_env()?;
    let repository = Arc::new(MySqlRepository::connect(&config.database_url).await?);
    repository.migrate().await?;
    reset_file_storage(&config.file_storage_dir, &*repository).await?;
    cleanup_expired(&*repository, &config.file_storage_dir).await;
    spawn_cleanup(repository.clone(), config.file_storage_dir.clone());

    let app = create_app(
        AppState::new(repository, config.cors_allowed_origins).with_file_storage_dir(config.file_storage_dir),
    );
    let listener = TcpListener::bind(config.address).await?;
    tracing::info!(address = %config.address, "QShare API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn spawn_cleanup(repository: Arc<MySqlRepository>, file_storage_dir: std::path::PathBuf) {
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            cleanup_expired(&*repository, &file_storage_dir).await;
        }
    });
}

async fn cleanup_expired(repository: &dyn Repository, file_storage_dir: &std::path::Path) {
    let now = Utc::now().naive_utc();
    let files = repository.delete_expired_files(now).await;
    match (
        repository.delete_expired_urls(now).await,
        repository.delete_expired_memos(now).await,
        files,
    ) {
        (Ok(urls), Ok(memos), Ok(files)) if urls > 0 || memos > 0 || !files.is_empty() => {
            let file_count = files.len();
            for file in files {
                let _ = tokio::fs::remove_file(file_storage_dir.join(&file.storage_key)).await;
            }
            tracing::info!(urls, memos, files = file_count, "deleted expired shared records")
        }
        (Ok(_), Ok(_), Ok(_)) => {}
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
            tracing::error!(%error, "failed to delete expired shared records")
        }
    }
}

async fn reset_file_storage(directory: &std::path::Path, repository: &dyn Repository) -> Result<(), StartupError> {
    match tokio::fs::remove_dir_all(directory).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    tokio::fs::create_dir_all(directory).await?;
    repository.clear_files().await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
