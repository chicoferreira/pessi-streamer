use serde::{Deserialize, Serialize};

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

#[derive(Serialize, Deserialize, Debug)]
pub enum SCPacket {
    /// A packet containing video data
    VideoPacket(Vec<u8>),
}
