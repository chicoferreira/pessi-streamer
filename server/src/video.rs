use anyhow::Context;
use ffmpeg_sidecar::version::ffmpeg_version;
use log::info;
use std::io;
use std::io::BufRead;
use std::path::PathBuf;
use tokio::net::UdpSocket;
use tokio::process::{Child, Command};

pub async fn new_video_process(video_path: PathBuf) -> anyhow::Result<(UdpSocket, Child)> {
    if !std::path::Path::new(&video_path).exists() {
        return Err(anyhow::anyhow!(
            "Video file does not exist: {:?}",
            video_path
        ));
    }

    info!("Starting video task for {:?}", video_path);

    let ffmpeg_socket = UdpSocket::bind("127.0.0.1:0").await?;
    let ffmpeg_socket_addr = ffmpeg_socket.local_addr()?;

    let codec = auto_detect_codec()
        .await
        .context("Failed to auto detect codec")?;
    info!("Auto detected codec: {}", codec);

    let ffmpeg_child_process =
        launch_video_process(video_path, &format!("udp://{}", ffmpeg_socket_addr), &codec)
            .context("Failed to launch ffmpeg process")?;

    Ok((ffmpeg_socket, ffmpeg_child_process))
}

async fn list_encoders() -> Vec<String> {
    Command::new("ffmpeg")
        .arg("-encoders")
        .output()
        .await
        .unwrap()
        .stdout
        .lines()
        .map(|line| line.unwrap().trim().to_string())
        .filter(|line| line.starts_with('V'))
        .map(|line| line.split(' ').nth(1).unwrap().to_string())
        .collect()
}

const PREFERED_CODECS: [&str; 5] = [
    "hevc_videotoolbox",
    "h264_nvenc",
    "hevc_amf",
    "h264",
    "libx264",
];

pub async fn auto_detect_codec() -> Option<String> {
    let encoder_list = list_encoders().await;

    PREFERED_CODECS
        .iter()
        .find(|codec| encoder_list.contains(&codec.to_string()))
        .map(|codec| codec.to_string())
}

pub fn launch_video_process(
    video_path: PathBuf,
    send_to_path: &str,
    codec_video: &str,
) -> io::Result<Child> {
    let string = ffmpeg_version().unwrap();
    info!("Starting ffmpeg process (version: {})", string);

    Command::new("ffmpeg")
        // .arg("-hide_banner")
        // .arg("-loglevel").arg("error")
        .arg("-re")
        .arg("-stream_loop")
        .arg("-1")
        .arg("-i")
        .arg(video_path.to_str().unwrap())
        .arg("-c:v")
        .arg(codec_video)
        .arg("-b:v")
        .arg("8M")
        .arg("-c:a")
        .arg("aac")
        .arg("-g")
        .arg("30")
        .arg("-f")
        .arg("mpegts")
        .arg(send_to_path)
        .spawn()
}
