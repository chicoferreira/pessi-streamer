use crate::bootstraper::{Neighbours, State};
use anyhow::Context;
use clap::{command, Parser};
use env_logger::Env;
use log::info;
use std::path::PathBuf;
use tokio::net::TcpListener;

mod bootstraper;

/// Simple program to start a bootstraper
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

    let bootstraper_addr =
        common::get_bootstraper_address().context("Failed to get bootstraper address")?;

    let server_socket = TcpListener::bind(bootstraper_addr)
        .await
        .context("Failed to bind to server socket")?;

    let neighbours =
        std::fs::read_to_string(&args.config).context("Failed to read neighbours.toml")?;

    let neighbours: Neighbours =
        toml::from_str(&neighbours).context("Failed to parse neighbours.toml")?;

    info!("Loaded {} neighbours information.", neighbours.len());

    info!("Starting bootstraper...");
    let state = State::new(neighbours);

    bootstraper::run_server(state, server_socket).await?;

    Ok(())
}
