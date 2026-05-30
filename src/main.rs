use gtk4::prelude::*;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

mod config;
mod input;
mod ui;

use ui::state::AppState;
use ui::window::MainWindow;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("vibeclicker=info")),
        )
        .init();

    // Multi-threaded Tokio runtime — kept alive for the whole process via the
    // EnterGuard, so tokio::spawn / tokio::time::sleep work from GTK callbacks.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build Tokio runtime");
    let _rt_guard = rt.enter();

    let state = Arc::new(Mutex::new(AppState::new()));

    let app = gtk4::Application::builder()
        .application_id("io.github.vibeclicker")
        .build();

    app.connect_activate({
        let state = state.clone();
        move |app| {
            let window = MainWindow::new(app);
            window.setup(state.clone());
            window.present();
        }
    });

    std::process::exit(app.run().value());
}
