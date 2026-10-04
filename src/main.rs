#![windows_subsystem = "windows"]

mod app;
mod decode;
mod files;
mod gesture;
mod panel;
mod render;
mod settings;
mod slider;
mod view;

fn main() {
    app::run();
}
