use crate::bootstraper::State;
use tokio::net::TcpListener;
use env_logger::Env;
use log::info;

mod bootstraper;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    info!("Starting bootstraper...");

    let server_socket = TcpListener::bind(("localhost", common::BOOTSTRAPER_PORT))
        .await
        .expect("Failed to bind to server socket");

    let state = State::new(server_socket);

    bootstraper::run_server(state).await?;

    Ok(())
}
