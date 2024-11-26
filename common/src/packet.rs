use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Debug)]
pub enum Packet {
    /// A video packet that contains video data
    VideoPacket {
        stream_id: u8,
        sequence_number: u64,
        stream_data: Vec<u8>,
    },
    /// Packets that only the node should receive
    NodePacket(NodePacket),
    /// Packets that only the server should receive
    ServerPacket(ServerPacket),
    /// Packets that only the client should receive
    ClientPacket(ClientPacket),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum NodePacket {
    /// Ping packets sent by clients
    /// The node will answer with `Packet::ClientPacket::VideoList`
    ClientPing {
        /// The sequence number of the ping packet
        /// Used to match the response with the request
        sequence_number: u64,
    },
    /// Packets originated by the server
    FloodPacket {
        hops: u8,
        created_at_server_time: SystemTime,
        videos_available: Vec<(u8, String)>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerPacket {
    /// Request to start a video
    RequestVideo(u8),
    /// Request to stop a video
    StopVideo(u8),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientPacket {
    /// Answer to a `Packet::NodePacket::ClientPing`
    VideoList {
        sequence_number: u64,
        videos: Vec<(u8, String)>,
        answer_creation_date: SystemTime,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum BootstraperPacket {
    /// Request to get neighbours
    RequestNeighbours,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BootstraperNeighboursResponse(pub Vec<IpAddr>);
