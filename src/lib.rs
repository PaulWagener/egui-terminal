pub mod config;
pub mod error;
mod into;
pub mod term;

use egui::{Response, Ui, Vec2, Widget};

pub use crate::config::definitions::TermResult;
pub use crate::config::term_config::{Config, Style};
pub use crate::term::TermHandler;

pub struct Terminal<'a> {
    terminal: &'a mut TermHandler,
    size: Option<Vec2>,
    style: Style,
}

impl Widget for Terminal<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = match self.size {
            Some(s) => s,
            None => ui.available_size(),
        };
        self.terminal
            .draw(ui, size)
            .expect("terminal should not error")
    }
}

impl<'a> Terminal<'a> {
    pub fn new(terminal: &'a mut TermHandler) -> Self {
        Self {
            terminal,
            size: None,
            style: Style::default(),
        }
    }

    pub fn with_size(mut self, size: Vec2) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}
