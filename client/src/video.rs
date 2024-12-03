use anyhow::Context;
use std::process::Stdio;
use std::str::FromStr;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc::error::SendError;

pub struct VideoPlayer {
    _process_child: Child,
    process_sender: tokio::sync::mpsc::Sender<Vec<u8>>,
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

        let mut process_stdin = process_child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Couldn't get stdin of video player process."))?;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);

        // this task will end when tx is dropped
        tokio::spawn(async move {
            while let Some(data) = rx.recv().await {
                if let Err(e) = process_stdin.write_all(&data).await {
                    log::error!("Couldn't write to video player stdin: {}", e);
                    break;
                }
            }
        });

        Ok(Self {
            _process_child: process_child,
            process_sender: tx,
        })
    }

    pub async fn write(&self, data: Vec<u8>) -> Result<(), SendError<Vec<u8>>> {
        self.process_sender.send(data).await
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
        .kill_on_drop(true)
        .spawn()
        .context("Couldn't spawn ffplay process.")
}

fn launch_mpv() -> anyhow::Result<Child> {
    Command::new("mpv")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("Couldn't spawn mpv process.")
}
