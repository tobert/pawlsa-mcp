mod alsa;
mod pw;
mod server;

use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Tracing to stderr — stdout is the MCP transport
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("pawlsa-mcp starting");

    let pw_handle = pw::spawn_pw_thread();

    // Brief pause to let PW state populate
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let server = server::PawlsaServer::new(pw_handle.state);
    let service = server.serve(rmcp::transport::stdio()).await?;

    tracing::info!("pawlsa-mcp serving on stdio");
    service.waiting().await?;

    Ok(())
}
