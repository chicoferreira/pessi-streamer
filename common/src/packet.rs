use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Node to Bootstraper packet
#[derive(Serialize, Deserialize, Debug)]
pub enum NBPacket {
    /// Request neighbours
    RequestNeighbours,
}

/// Bootstraper to Node packet
#[derive(Serialize, Deserialize, Debug)]
pub enum BNPacket {
    /// Response to RequestNeighbours
    Neighbours(Vec<IpAddr>),
}

/// Client to Server packet
#[derive(Serialize, Deserialize, Debug)]
pub enum CSPacket {
    /// Heartbeat packet to keep connection alive
    Heartbeat,
    /// Request to start a video
    RequestVideo(String),
    /// Request to stop a video
    StopVideo(String),
}

/// Server to Client packet
#[derive(Serialize, Deserialize, Debug)]
pub enum SCPacket {
    /// A packet containing video data
    VideoPacket(Vec<u8>),
}

/// Server or Node to Node packet
#[derive(Serialize, Deserialize, Debug)]
pub enum SNCPacket {
    FloodPacket {
        hops: u8,
        millis_created_at_server: u128,
        videos_available: Vec<String>,
    },
    SCPacket(SCPacket),
}
