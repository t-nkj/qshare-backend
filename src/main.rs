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
    cleanup_expired(&*repository).await;
    spawn_cleanup(repository.clone());

    let app = create_app(AppState::new(repository, config.cors_allowed_origins));
    let listener = TcpListener::bind(config.address).await?;
    tracing::info!(address = %config.address, "QShare API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn spawn_cleanup(repository: Arc<MySqlRepository>) {
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60 * 60));
        interval.tick().await;
        loop {
            interval.tick().await;
            cleanup_expired(&*repository).await;
        }
    });
}

async fn cleanup_expired(repository: &dyn Repository) {
    match repository.delete_expired_urls(Utc::now().naive_utc()).await {
        Ok(deleted) if deleted > 0 => tracing::info!(deleted, "deleted expired URL records"),
        Ok(_) => {}
        Err(error) => tracing::error!(%error, "failed to delete expired URL records"),
    }
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
