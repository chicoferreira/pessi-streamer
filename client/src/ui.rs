#![allow(rustdoc::missing_crate_level_docs)]

use crate::State;
use eframe::egui;
use egui::Spinner;

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
            ui.heading("Servers:");
            for server_entry in self.state.servers.iter() {
                let node = server_entry.value();

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Server: {}", node.addr));

                        if let Some(score) = node.average_rtt() {
                            ui.label(format!("Average RTT: {:?}", score));
                        } else {
                            ui.add(Spinner::new());
                            ui.label("Connecting...");
                        }
                    });
                });

                ui.add_space(10.0);
            }

            ui.heading("Available Streams:");
            if let Ok(streams) = self.state.video_list.read() {
                if streams.len() > 0 {
                    for (stream_id, stream_name) in streams.iter() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}: {}", stream_id, stream_name));
                            if ui.button("Play").clicked() {
                                self.state.start_playing_sync(*stream_id);
                            }
                            if ui.button("Stop").clicked() {
                                self.state.stop_playing_id_sync(*stream_id);
                            }

                            if let Some(video) = self.state.playing_videos.get(stream_id) {
                                let bytes =
                                    bytefmt::format(video.video_player.bytes_written() as u64);
                                ui.label(format!("Bytes received: {bytes}"));
                            }
                        });
                    }
                } else {
                    ui.spinner();
                }
            }
        });
    }
}
