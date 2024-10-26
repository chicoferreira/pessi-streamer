use ffmpeg_sidecar::child::FfmpegChild;
use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::version::ffmpeg_version;
use log::{debug, info, trace};
use tokio::net::UdpSocket;

pub struct Video {
    pub video_path: String,
    ffmpeg_socket_read_task: tokio::task::JoinHandle<()>,
    ffmpeg_child_process: FfmpegChild,
}

impl Drop for Video {
    fn drop(&mut self) {
        self.ffmpeg_child_process.kill().unwrap();
        self.ffmpeg_socket_read_task.abort();
    }
}

impl Video {
    pub async fn new_video_task(video_path: String) -> anyhow::Result<Video> {
        if !std::path::Path::new(&video_path).exists() {
            return Err(anyhow::anyhow!("Video file does not exist: {}", video_path));
        }

        info!("Starting video task for {}", video_path);

        let ffmpeg_socket = UdpSocket::bind("127.0.0.1:0").await?;
        let ffmpeg_socket_addr = ffmpeg_socket.local_addr()?;

        let ffmpeg_child_process = launch_video_process(&video_path, format!("udp://{}", ffmpeg_socket_addr).as_str());

        let ffmpeg_socket_read_task = tokio::spawn(Video::handle_ffmpeg_read_socket(ffmpeg_socket));

        Ok(Video {
            video_path: video_path.clone(),
            ffmpeg_socket_read_task,
            ffmpeg_child_process,
        })
    }

    async fn handle_ffmpeg_read_socket(ffmpeg_socket: UdpSocket) {
        let mut buf = vec![0u8; 16384];
        loop {
            let n = ffmpeg_socket.recv(&mut buf).await.unwrap();
            trace!("Received {} bytes from ffmpeg", n);
        }
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
