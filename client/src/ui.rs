#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
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
                let server = server_entry.value().clone();

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("Server: {}", server.addr));

                        if let Some(score) = server.get_score() {
                            ui.label(format!("Average RTT: {:.2} ms", score));
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
                if let Some(streams) = streams.as_ref() {
                    for (stream_id, stream_name) in streams.iter() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}: {}", stream_id, stream_name));
                            if ui.button("Play").clicked() {}
                            if ui.button("Stop").clicked() {}
                        });
                    }
                } else {
                    ui.spinner();
                }
            }
        });
    }
}