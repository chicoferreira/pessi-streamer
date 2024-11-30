use crate::bootstrapper::{Neighbours, State};
use anyhow::Context;
use clap::{command, Parser};
use env_logger::Env;
use log::info;
use std::path::PathBuf;
use tokio::net::TcpListener;

mod bootstrapper;

/// Simple program to start a bootstrapper
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Neighbours configuration file
    #[arg(short, long, default_value = "topologies/neighbours.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug")).init();

    let args = Args::parse();

    let bootstrapper_addr =
        common::get_bootstrapper_address().context("Failed to get bootstrapper address")?;

    let server_socket = TcpListener::bind(bootstrapper_addr)
        .await
        .context("Failed to bind to server socket")?;

    let neighbours =
        std::fs::read_to_string(&args.config).context("Failed to read neighbours.toml")?;

    let neighbours: Neighbours =
        toml::from_str(&neighbours).context("Failed to parse neighbours.toml")?;

    info!("Loaded {} neighbours information.", neighbours.len());

    info!("Starting bootstrapper...");
    let state = State::new(neighbours);

    bootstrapper::run_server(state, server_socket).await?;

    Ok(())
}
