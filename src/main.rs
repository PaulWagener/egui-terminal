pub mod app;

use app::App;


fn main () -> Result<(), eframe::Error> {
    eframe::run_native(
        "TermTest",
        eframe::NativeOptions::default(),
        Box::new(| cc| Ok(App::setup(cc))),
    )
}

