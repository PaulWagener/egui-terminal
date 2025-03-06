use eframe::{CreationContext, egui};
use egui_terminal::{TermHandler, Terminal};

pub struct App {
    terminal: TermHandler,
}
impl App {
    pub fn new() -> Self {
        let terminal = TermHandler::new_from_str(&"/bin/zsh");

        Self { terminal }
    }

    pub fn setup(_cc: &CreationContext) -> Box<dyn eframe::App> {
        Box::new(App::new())
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(Terminal::new(&mut self.terminal).with_size(ui.available_size()));
        });
    }
}
