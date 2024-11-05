use anyhow::Context;
use ffmpeg_sidecar::child::FfmpegChild;
use ffmpeg_sidecar::command::FfmpegCommand;
use ffmpeg_sidecar::version::ffmpeg_version;
use log::info;
use std::io::BufRead;
use std::process::Command;
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

        let codec = auto_detect_codec().context("Failed to auto detect codec")?;
        info!("Auto detected codec: {}", codec);

        let ffmpeg_child_process = launch_video_process(
            &video_path,
            &format!("udp://{}", ffmpeg_socket_addr),
            &codec,
        );

        Ok(VideoProcess {
            ffmpeg_child_process,
            socket: ffmpeg_socket,
        })
    }

    pub async fn recv(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.socket.recv(buf).await
    }
}

fn list_encoders() -> Vec<String> {
    Command::new("ffmpeg")
        .arg("-encoders")
        .output()
        .unwrap()
        .stdout
        .lines()
        .map(|line| line.unwrap().trim().to_string())
        .filter(|line| line.starts_with("V"))
        .map(|line| line.split(' ').nth(1).unwrap().to_string())
        .collect()
}

pub fn auto_detect_codec() -> Option<String> {
    // ffmpeg -encoders

    let encoder_list = list_encoders();

    ["h264_videotoolbox", "h264_nvenc", "h264", "libx264"]
        .iter()
        .find(|codec| encoder_list.contains(&codec.to_string()))
        .map(|codec| codec.to_string())
}

pub fn launch_video_process(
    video_path: &str,
    send_to_path: &str,
    codec_video: &str,
) -> FfmpegChild {
    let string = ffmpeg_version().unwrap();
    info!("Starting ffmpeg process (version: {})", string);

    // todo: auto detect gpu acceleration

    FfmpegCommand::new()
        .realtime()
        // loop video indefinitely
        .arg("-stream_loop")
        .arg("-1")
        .hwaccel("auto")
        .input(video_path)
        .codec_video(codec_video)
        .arg("-b:v")
        .arg("8M")
        .codec_audio("aac")
        // send keyframes every 30 frames
        .arg("-g")
        .arg("30")
        .format("mpegts")
        .output(send_to_path)
        .spawn()
        .unwrap()
}
