use serde::{Deserialize, Serialize};

pub const SERVER_PORT: u16 = 8011;

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


