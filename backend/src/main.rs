use std::sync::Arc;

use findamodel::{LogBuffer, Settings, logging};

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let log_buffer = Arc::new(LogBuffer::new(2000));
    logging::init(log_buffer.clone());
    let settings = Settings::from_env()?;
    findamodel::run(settings, log_buffer, shutdown_signal()).await
}
