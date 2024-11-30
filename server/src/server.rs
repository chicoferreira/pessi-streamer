use crate::video;
use common::packet::{Packet, ServerPacket};
use common::reliable::ReliableUdpSocket;
use dashmap::DashMap;
use log::{debug, error, info, trace};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU8;
use std::sync::Arc;
use tokio::select;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt,
};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

struct Video {
    /// The id of the video stream
    name: String,
    /// The path to the video file
    interested: Vec<SocketAddr>,
    sequence_number: u64,
}

#[derive(Clone)]
pub struct State {
    /// Map of video paths to interested subscribers
    videos: Arc<DashMap<u8, Video>>,
    last_video_id: Arc<AtomicU8>,
    clients_socket: ReliableUdpSocket,
    neighbours: Arc<Vec<SocketAddr>>,
}

impl State {
    pub fn new(clients_socket: ReliableUdpSocket, neighbours: Vec<SocketAddr>) -> Self {
        Self {
            clients_socket,
            videos: Arc::new(DashMap::new()),
            last_video_id: Arc::new(AtomicU8::new(0)),
            neighbours: Arc::new(neighbours),
        }
    }

    fn get_next_video_id(&self) -> u8 {
        self.last_video_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    fn new_video(&self, name: String) -> Video {
        Video {
            name,
            interested: Vec::new(),
            sequence_number: 0,
        }
    }

    pub async fn start_streaming_video(
        &self,
        video_path: PathBuf,
        video_folder: PathBuf,
    ) -> anyhow::Result<()> {
        let (stream_socket, mut child_process) =
            video::new_video_process(video_path.clone()).await?;
        let video_name = video_path
            .with_extension("")
            .strip_prefix(video_folder)?
            .to_str()
            .unwrap()
            .to_string();

        let id = self.get_next_video_id();
        self.videos.insert(id, self.new_video(video_name.clone()));

        let state = self.clone();

        let child_future = async { child_process.wait().await };

        tokio::pin!(child_future);

        let mut buf = [0u8; 65536];
        loop {
            select! {
                _ = &mut child_future => {
                    error!("Child process ended unexpectedly");
                    return Err(anyhow::anyhow!("Child process ended unexpectedly"));
                }
                packet = stream_socket.recv(&mut buf) => {
                    let n = match packet {
                        Ok(n) => n,
                        Err(e) => {
                            error!("Failed to receive packet: {}", e);
                            continue;
                        }
                    };

                    if let Some(mut video) = state.videos.get_mut(&id) {
                        if video.interested.is_empty() {
                            continue;
                        }

                        video.sequence_number += 1;

                        let stream_data = buf[..n].to_vec();

                        let packet = Packet::VideoPacket {
                            stream_id: id,
                            sequence_number: video.sequence_number,
                            stream_data,
                        };

                        if let Err(e) = state
                            .clients_socket
                            .send_unreliable_broadcast(&packet, &video.interested)
                            .await
                        {
                            error!("Failed to send video packet: {}", e);
                        }

                        trace!(
                            "Sent video packet (video={}, seq={}, size={}) to {} subscribers",
                            id,
                            video.sequence_number,
                            n,
                            video.interested.len()
                        );
                    }
                }
            }
        }
    }

    pub fn get_video_list(&self) -> Vec<(u8, String)> {
        self.videos
            .iter()
            .map(|entry| (*entry.key(), entry.value().name.clone()))
            .collect()
    }
}

pub async fn run_client_socket(state: State) -> anyhow::Result<()> {
    let socket = state.clients_socket;
    info!("Waiting for client packets on {}", socket.local_addr()?);

    let mut buf = [0u8; 65536];
    loop {
        let (packet, socket_addr) = match socket.receive(&mut buf).await {
            Ok(Some(result)) => result,
            Ok(None) => {
                // Acknowledgement packet received
                debug!("Received acknowledgement packet");
                continue;
            }
            Err(e) => {
                error!("Failed to receive packet: {}", e);
                continue;
            }
        };

        match packet {
            Packet::ServerPacket(ServerPacket::RequestVideo(video_id)) => {
                info!(
                    "Received request to start video {} from {}",
                    video_id, socket_addr
                );
                if let Some(mut video) = state.videos.get_mut(&video_id) {
                    if !video.interested.contains(&socket_addr) {
                        video.interested.push(socket_addr);
                    }
                }
            }
            Packet::ServerPacket(ServerPacket::StopVideo(video_id)) => {
                info!(
                    "Received request to stop video {} from {}",
                    video_id, socket_addr
                );
                if let Some(mut subscribers) = state.videos.get_mut(&video_id) {
                    subscribers
                        .interested
                        .retain(|&subscriber| subscriber != socket_addr);
                }
            }
            _ => {
                error!(
                    "Received unexpected packet from {}: {:?}",
                    socket_addr, packet
                );
            }
        }
    }
}

pub async fn watch_videos_folder(state: State, video_folder: PathBuf) -> notify::Result<()> {
    let (mut watcher, mut rx) = async_watcher()?;

    watcher.watch(video_folder.as_ref(), RecursiveMode::Recursive)?;

    while let Some(res) = rx.next().await {
        match res {
            Ok(event) => handle_file_event(event, &state, &video_folder).await,
            Err(e) => error!("watch error: {:?}", e),
        }
    }

    Ok(())
}

async fn handle_file_event(event: Event, state: &State, video_folder: &PathBuf) {
    match event.kind {
        notify::EventKind::Create(_) => {
            for video in event.paths {
                let state = state.clone();
                let video_folder = video_folder.clone();

                info!("Detected new video: {:?}", video);
                tokio::spawn(async move {
                    state
                        .start_streaming_video(video, video_folder)
                        .await
                        .expect("Failed to start streaming video");
                });
            }
        }
        _ => {}
    }
}

fn async_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
    let (mut tx, rx) = channel(1);

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let watcher = RecommendedWatcher::new(
        move |res| {
            futures::executor::block_on(async {
                tx.send(res).await.unwrap();
            })
        },
        Config::default(),
    )?;

    Ok((watcher, rx))
}

pub mod flood {
    use crate::server::State;
    use common::packet::{NodePacket, Packet};
    use log::info;
    use std::time::{Duration, SystemTime};

    pub async fn run_periodic_flood_packets(state: State) -> anyhow::Result<()> {
        info!("Starting periodic flood packets to neighbours");

        loop {
            let packet = Packet::NodePacket(NodePacket::FloodPacket {
                hops: 0,
                created_at_server_time: SystemTime::now(),
                videos_available: state.get_video_list(),
            });

            state
                .clients_socket
                .send_unreliable_broadcast(&packet, &state.neighbours)
                .await?;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
