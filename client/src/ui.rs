#![allow(rustdoc::missing_crate_level_docs)]

use crate::client::{NodeStatus, State};
use eframe::egui;
use egui::Spinner;
use std::sync::atomic::Ordering;

pub fn run_ui(state: State) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Pessi Streamer",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::new(MyApp { state }))
        }),
    )
}

struct MyApp {
    state: State,
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Nodes:");
            for server_entry in self.state.nodes.iter() {
                let node = server_entry.value();

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Node: {}", node.addr));

                        match node.status {
                            NodeStatus::Connecting => {
                                ui.add(Spinner::new());
                                ui.label("Connecting...");
                            }
                            NodeStatus::Connected => {
                                ui.label(format!("Average RTT: {:?}", node.average_rtt()));
                            }
                            NodeStatus::Unresponsive => {
                                ui.add(Spinner::new());
                                ui.label("Unresponsive");
                            }
                        }
                    });
                });

                ui.add_space(10.0);
            }

            ui.heading("Available Streams:");
            let streams = self.state.available_videos();
            if !streams.is_empty() {
                for stream_id in streams.iter() {
                    ui.horizontal(|ui| {
                        let stream_name = &*self.state.video_names.get(stream_id).unwrap();
                        ui.label(format!("{}: {}", stream_id, stream_name));

                        let playing = self.state.playing_videos.contains_key(stream_id);
                        if !playing && ui.button("Play").clicked() {
                            self.state.ui_start_playing(*stream_id);
                        }
                        if playing && ui.button("Stop").clicked() {
                            self.state.ui_stop_playing_id(*stream_id);
                        }

                        if let Some(video) = self.state.playing_videos.get(stream_id) {
                            let bytes =
                                bytefmt::format(video.bytes_written.load(Ordering::Relaxed) as u64);
                            ui.label(format!("Bytes received: {bytes}"));

                            match video.source {
                                None => ui.add(Spinner::new()),
                                Some(source) => ui.label(format!("from {source}")),
                            };
                        }
                    });
                }
            } else {
                ui.spinner();
            }
        });
    }
}
