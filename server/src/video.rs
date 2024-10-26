use ffmpeg_sidecar::child::FfmpegChild;
use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::version::ffmpeg_version;
use log::{info};
use tokio::net::UdpSocket;

pub struct VideoProcess {
    ffmpeg_child_process: FfmpegChild,
    socket: UdpSocket,
}

impl Drop for VideoProcess {
    fn drop(&mut self) {
        self.ffmpeg_child_process.kill().unwrap();
    }
}

impl VideoProcess {
    pub async fn new_video_process(video_path: &str) -> anyhow::Result<VideoProcess> {
        if !std::path::Path::new(&video_path).exists() {
            return Err(anyhow::anyhow!("Video file does not exist: {}", video_path));
        }

        info!("Starting video task for {}", video_path);

        let ffmpeg_socket = UdpSocket::bind("127.0.0.1:0").await?;
        let ffmpeg_socket_addr = ffmpeg_socket.local_addr()?;

        let ffmpeg_child_process = launch_video_process(&video_path, format!("udp://{}", ffmpeg_socket_addr).as_str());

        Ok(VideoProcess {
            ffmpeg_child_process,
            socket: ffmpeg_socket,
        })
    }

    pub async fn recv(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; 16384];
        self.socket.recv(&mut buf).await.map(|n| buf[..n].to_vec())
    }
}

pub fn launch_video_process(video_path: &str, send_to_path: &str) -> FfmpegChild {
    let string = ffmpeg_version().unwrap();
    info!("Starting ffmpeg process (version: {})", string);

    // todo: pre-encode video before stream
    // todo: auto detect gpu acceleration

    FfmpegCommand::new()
        .realtime()
        // loop video indefinitely
        .arg("-stream_loop").arg("-1")
        .hwaccel("auto")
        .input(video_path)
        .codec_video("h264")
        .codec_audio("aac")
        // send keyframes every 30 frames
        .arg("-g").arg("30")
        .format("mpegts")
        .output(send_to_path)
        .spawn()
        .unwrap()
}
