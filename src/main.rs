mod constants;
mod game;
mod ui;
mod variants;

use gtk4::prelude::*;
use gtk4::Application;
use ui::Ubersweeper;

fn main() {
    let app = Application::builder()
        .application_id("com.hydrogenhallide.ubersweeper")
        .build();

    app.connect_activate(|app| {
        let ubersweeper = Ubersweeper::new();
        ubersweeper.build_ui(app);
    });

    app.run();
}
