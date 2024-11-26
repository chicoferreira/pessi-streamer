use anyhow::Context;
use std::io;
use std::process::Stdio;
use std::str::FromStr;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

pub struct VideoPlayer {
    bytes_written: usize,
    process_child: Child,
    process_stdin: ChildStdin,
}

#[derive(Clone, Copy, Debug)]
pub enum VideoPlayerType {
    Ffplay,
    Mpv,
}

impl VideoPlayerType {
    pub async fn check_installation(&self) -> bool {
        match self {
            Self::Ffplay => check_ffplay_instalation().await,
            Self::Mpv => check_mpv_instalation().await,
        }
    }

    fn launch(&self) -> anyhow::Result<Child> {
        match self {
            Self::Ffplay => launch_ffplay(),
            Self::Mpv => launch_mpv(),
        }
    }
}

impl FromStr for VideoPlayerType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ffplay" => Ok(Self::Ffplay),
            "mpv" => Ok(Self::Mpv),
            _ => Err(anyhow::anyhow!("Invalid video player type.")),
        }
    }
}

impl VideoPlayer {
    pub fn launch(video_player_type: VideoPlayerType) -> anyhow::Result<VideoPlayer> {
        let mut process_child = video_player_type
            .launch()
            .context("Couldn't launch video player process.")?;

        let process_stdin = process_child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Couldn't get stdin of video player process."))?;

        Ok(Self {
            bytes_written: 0,
            process_child,
            process_stdin,
        })
    }

    pub async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.bytes_written += data.len();
        self.process_stdin.write_all(data).await
    }

    pub async fn kill(&mut self) -> io::Result<()> {
        self.process_child.kill().await
    }

    pub fn bytes_written(&self) -> usize {
        self.bytes_written
    }
}

async fn check_ffplay_instalation() -> bool {
    Command::new("ffplay")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

async fn check_mpv_instalation() -> bool {
    Command::new("mpv")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

fn launch_ffplay() -> anyhow::Result<Child> {
    Command::new("ffplay")
        .args("-format mpegts -".split(' '))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("Couldn't spawn ffplay process.")
}

fn launch_mpv() -> anyhow::Result<Child> {
    Command::new("mpv")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("Couldn't spawn mpv process.")
}
