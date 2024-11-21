use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Packets that a node can receive
#[derive(Serialize, Deserialize, Debug)]
pub enum NodePacket {
    /// Response for `BootstraperPacket::RequestNeighbours`
    FloodPacket {
        hops: u8,
        millis_created_at_server: u128,
        videos_available: Vec<(u8, String)>,
    },
    /// Ping packets sent by clients
    /// The node will answer with `ClientPacket::VideoList`
    ClientPing {
        /// The sequence number of the ping packet
        /// Used to match the response with the request
        sequence_number: u64,
    },
    /// A packet to be redirected to the server
    RedirectToServer(ServerPacket),
    /// A video packet that contains video data
    VideoPacket {
        stream_id: u8,
        stream_data: Vec<u8>,
    },
}

/// Packets that a client can receive
#[derive(Serialize, Deserialize, Debug)]
pub enum ClientPacket {
    /// A packet containing video data
    VideoPacket {
        stream_id: u8,
        stream_data: Vec<u8>,
    },
    VideoList {
        sequence_number: u64,
        videos: Vec<(u8, String)>,
    },
}

/// Packets that a server can receive
#[derive(Serialize, Deserialize, Debug)]
pub enum ServerPacket {
    /// Request to start a video
    RequestVideo(u8),
    /// Request to stop a video
    StopVideo(u8),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BootstraperPacket {
    /// Request to get neighbours
    RequestNeighbours,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BootstraperNeighboursResponse(pub Vec<IpAddr>);
