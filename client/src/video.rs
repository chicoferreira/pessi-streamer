use anyhow::Context;
use std::io;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

pub struct VideoPlayer {
    ffplay_child: Child,
    ffplay_stdin: ChildStdin,
    bytes_received: usize,
}

impl VideoPlayer {
    pub fn launch() -> anyhow::Result<Self> {
        let mut ffplay = Command::new("ffplay")
            .args("-f mpegts -i -".split(' '))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("Couldn't spawn ffplay process.")?;

        let ffplay_stdin = ffplay.stdin.take().context("Couldn't take ffplay stdin.")?;

        Ok(Self {
            ffplay_child: ffplay,
            ffplay_stdin,
            bytes_received: 0,
        })
    }

    pub async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.bytes_received += data.len();
        self.ffplay_stdin.write_all(data).await
    }

    pub fn bytes_received(&self) -> usize {
        self.bytes_received
    }

    pub async fn kill(&mut self) -> io::Result<()> {
        self.ffplay_child.start_kill()
    }
}
