use ffmpeg_sidecar::child::FfmpegChild;
use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::version::ffmpeg_version;
use log::{debug, error, info};
use std::io::Read;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

#[tokio::main]
async fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();

    ffmpeg_sidecar::download::auto_download().unwrap();

    let (sender, _) = broadcast::channel(16);

    let (_ffmpeg_process, _handle) = launch_video_process(sender.clone(), "video.mp4");

    let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
    info!("Listening on {}", listener.local_addr().unwrap());
    loop {
        let (socket, _) = listener.accept().await.unwrap();

        let receiver = sender.subscribe();

        info!("Accepted connection from {}", socket.peer_addr().unwrap());

        tokio::spawn(handle_client(socket, receiver));
    }
}

async fn handle_client(mut socket: tokio::net::TcpStream, mut receiver: broadcast::Receiver<Vec<u8>>) {
    loop {
        if let Ok(video_buf) = receiver.recv().await {
            if let Err(error) = socket.write_all(&video_buf).await {
                error!("Failed to write data to socket: {}", error);
                break;
            }
            debug!("Sent {} bytes", video_buf.len());
        }
    }
}

fn launch_video_process(sender: broadcast::Sender<Vec<u8>>, video_path: &str) -> (FfmpegChild, tokio::task::JoinHandle<()>) {
    let string = ffmpeg_version().unwrap();
    info!("FFmpeg version: {}", string);

    // todo: pre-encode video before stream
    // todo: auto detect gpu acceleration

    let mut video_src = FfmpegCommand::new()
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
        .output("-")
        .spawn()
        .unwrap();

    let mut video_src_stdout = video_src.take_stdout().unwrap();

    let handle = tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; 65536];
        loop {
            let n = video_src_stdout.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            // ignore the result
            let _ = sender.send(buf[..n].to_vec());
        }
    });

    (video_src, handle)
}
