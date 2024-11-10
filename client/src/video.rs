use anyhow::Context;
use log::info;
use std::io;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

pub struct VideoPlayer {
    ffplay_stdin: ChildStdin,
}

impl VideoPlayer {
    pub fn launch() -> anyhow::Result<Self> {
        let mut ffplay = Command::new("ffplay")
            .args(
                "-fflags nobuffer -analyzeduration 500000 -probesize 2000000 -f mpegts -i -".split(' '),
            )
            .stdin(Stdio::piped())
            .spawn().context("Couldn't spawn ffplay process.")?;

        let ffplay_stdin = ffplay.stdin.take().context("Couldn't take ffplay stdin.")?;

        tokio::spawn(Self::wait_to_close(ffplay));

        Ok(Self {
            ffplay_stdin,
        })
    }

    pub async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.ffplay_stdin.write_all(data).await
    }

    async fn wait_to_close(mut ffplay: Child) {
        let _ = ffplay.wait().await;
        info!("ffplay process closed.");
    }
}