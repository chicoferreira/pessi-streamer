use crate::bootstraper::{Neighbours, State};
use anyhow::Context;
use env_logger::Env;
use log::info;
use tokio::net::TcpListener;

mod bootstraper;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    info!("Starting bootstraper...");

    let bootstraper_addr = common::get_bootstraper_address()
        .context("Failed to get bootstraper address")?;

    let server_socket = TcpListener::bind(bootstraper_addr)
        .await
        .context("Failed to bind to server socket")?;

    let neighbours = std::fs::read_to_string("topologies/neighbours.toml")
        .context("Failed to read neighbours.toml")?;

    let neighbours: Neighbours = toml::from_str(&*neighbours)
        .context("Failed to parse neighbours.toml")?;

    info!("Loaded {} neighbours information.", neighbours.len());

    let state = State::new(neighbours);

    bootstraper::run_server(state, server_socket).await?;

    Ok(())
}
