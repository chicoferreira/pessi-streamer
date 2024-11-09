use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Packets that a node can receive
#[derive(Serialize, Deserialize, Debug)]
pub enum NodePacket {
    /// Response for `BootstraperPacket::RequestNeighbours`
    FloodPacket {
        hops: u8,
        millis_created_at_server: u128,
        videos_available: Vec<String>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BootstraperPacket {
    /// Request to get neighbours
    RequestNeighbours,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BootstraperNeighboursResponse(pub Vec<IpAddr>);

/// Packets that a client can receive
#[derive(Serialize, Deserialize, Debug)]
pub enum ClientPacket {
    /// A packet containing video data
    VideoPacket(Vec<u8>),
}

/// Packets that a server can receive
#[derive(Serialize, Deserialize, Debug)]
pub enum ServerPacket {
    /// Request to start a video
    RequestVideo(String),
    /// Request to stop a video
    StopVideo(String),
}
