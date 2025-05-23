// Special thanks to Speak2Erase for the code used as reference for this implementation (and for
// some code taken) :)
//
// You can find her on Github, she does good work; This project is based on luminol-term, from the
// Luminol project, at https://github.com/Astrabit-ST/Luminol. Go check it out!

use std::ffi::OsString;
use std::io::prelude::*;
use std::ops::Range;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

pub use portable_pty::CommandBuilder;
pub use termwiz::Error;

use portable_pty::{ExitStatus, PtySize};
use termwiz::cellcluster::CellCluster;
use wezterm_term::{Terminal as WezTerm, TerminalSize};

use egui::{Color32, Event, FontId, InputState, Modifiers, Response, TextFormat, Ui, Vec2};

use crate::config::definitions::TermResult;
use crate::config::term_config::{Config, Style};
use crate::into::*;

pub struct TermHandler {
    terminal: WezTerm,
    reader: Receiver<Vec<termwiz::escape::Action>>,
    style: Style,
    wez_config: Arc<Config>,

    child: Box<dyn portable_pty::Child + Send + Sync>,
    pair: portable_pty::PtyPair,
    text_width: f32,
    text_height: f32,
    size: TerminalSize,
}

impl Drop for TermHandler {
    fn drop(&mut self) {
        self.kill()
    }
}

impl TermHandler {
    pub fn new(command: CommandBuilder) -> Self {
        Self::try_new(command).expect("should be able to create terminal")
    }

    pub fn new_from_str(command: &str) -> Self {
        Self::try_new_from_str(command).expect("should be able to create terminal")
    }

    pub fn try_new_from_str(command: &str) -> Result<Self, termwiz::Error> {
        Self::try_new(CommandBuilder::new(command))
    }

    pub fn try_new(command: CommandBuilder) -> Result<Self, termwiz::Error> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system.openpty(portable_pty::PtySize::default())?;
        let child = pair.slave.spawn_command(command.clone())?;

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let style = Style::default();

        let wez_config = style.default_wez_config();

        let terminal = WezTerm::new(
            TerminalSize::default(),
            wez_config.clone(),
            command
                .get_argv()
                .join(OsString::from(" ").as_os_str())
                .into_string()
                .expect("should be able to convert command to String")
                .as_ref(),
            "1.0",
            writer,
        );

        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let mut buf = [0; 2usize.pow(10)];
            let mut reader = std::io::BufReader::new(reader);
            let mut parser = termwiz::escape::parser::Parser::new();

            loop {
                let Ok(len) = reader.read(&mut buf) else {
                    return;
                };
                if len == 0 {
                    return;
                }
                let actions = parser.parse_as_vec(&buf[0..len]);
                let Ok(_) = sender.send(actions) else { return };
            }
        });

        Ok(Self {
            terminal,
            style,
            wez_config,
            reader: receiver,
            child,
            pair,
            text_width: 0.0,
            text_height: 0.0,
            size: TerminalSize::default(),
        })
    }

    pub fn title(&self, title: &str) -> String {
        self.terminal.get_title().replace("wezterm", title)
    }

    pub fn id(&self) -> egui::Id {
        if let Some(id) = self.child.process_id() {
            egui::Id::new(id)
        } else {
            egui::Id::new(self.terminal.get_title())
        }
    }

    fn format_to_egui(&self, cluster: &CellCluster) -> TextFormat {
        let palette = self.terminal.get_config().color_palette();

        let fg_color = palette.resolve_fg(cluster.attrs.foreground()).into_egui();
        let bg_color = palette.resolve_bg(cluster.attrs.background()).into_egui();
        let underline = if !matches!(cluster.attrs.underline(), wezterm_term::Underline::None) {
            egui::Stroke::new(
                1.0,
                palette
                    .resolve_fg(cluster.attrs.underline_color())
                    .into_egui(),
            )
        } else {
            egui::Stroke::NONE
        };
        let strikethrough = if cluster.attrs.strikethrough() {
            egui::Stroke::new(
                1.0,
                palette.resolve_fg(cluster.attrs.foreground()).into_egui(),
            )
        } else {
            egui::Stroke::NONE
        };

        egui::TextFormat {
            font_id: egui::FontId::monospace(12.0),
            color: fg_color,
            background: bg_color,
            italics: cluster.attrs.italic(),
            underline,
            strikethrough,
            ..Default::default()
        }
    }

    fn event_pointer_move(
        &mut self,
        e: &Event,
        response: &Response,
        modifiers: Modifiers,
    ) -> TermResult {
        let Event::PointerMoved(pos) = e else {
            unreachable!()
        };
        let relative_pos = *pos - response.rect.min;
        let char_x = (relative_pos.x / 12.0) as usize;
        let char_y = (relative_pos.y / 12.0) as i64;
        self.terminal.mouse_event(wezterm_term::MouseEvent {
            kind: wezterm_term::MouseEventKind::Move,
            x: char_x,
            y: char_y,
            x_pixel_offset: 0,
            y_pixel_offset: 0,
            button: wezterm_term::MouseButton::None,
            modifiers: modifiers.into_wez(),
        })?;

        Ok(())
    }

    fn event_pointer_button(&mut self, e: &Event, response: &Response) -> TermResult {
        let Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        } = e
        else {
            unreachable!()
        };

        let relative_pos = *pos - response.rect.min;
        let char_x = (relative_pos.x / self.text_width) as usize;
        let char_y = (relative_pos.y / self.text_height) as i64;
        self.terminal.mouse_event(wezterm_term::MouseEvent {
            kind: if *pressed {
                wezterm_term::MouseEventKind::Press
            } else {
                wezterm_term::MouseEventKind::Release
            },
            x: char_x,
            y: char_y,
            x_pixel_offset: 0,
            y_pixel_offset: 0,
            button: button.into_wez(),
            modifiers: modifiers.into_wez(),
        })?;

        Ok(())
    }

    fn event_scroll(&mut self, e: &Event, _: Modifiers, pointer_position: Vec2) -> TermResult {
        let Event::MouseWheel {
            unit: _,
            delta,
            modifiers,
        } = e
        else {
            unreachable!()
        };
        let char_x = (pointer_position.x / self.text_width) as usize;
        let char_y = (pointer_position.y / self.text_height) as i64;
        self.terminal.mouse_event(wezterm_term::MouseEvent {
            kind: wezterm_term::MouseEventKind::Press,
            x: char_x,
            y: char_y,
            x_pixel_offset: 0,
            y_pixel_offset: 0,
            button: if delta.y.is_sign_positive() {
                wezterm_term::MouseButton::WheelUp(delta.y as usize)
            } else {
                wezterm_term::MouseButton::WheelDown(-delta.y as usize)
            },
            modifiers: modifiers.into_wez(),
        })?;

        Ok(())
    }

    fn event_key(&mut self, e: &Event) -> TermResult {
        let Event::Key {
            key,
            modifiers,
            pressed,
            ..
        } = e
        else {
            unreachable!()
        };

        if let Ok(key) = key.try_into_wez() {
            if *pressed {
                self.terminal.key_down(key, modifiers.into_wez())?;
            } else {
                self.terminal.key_up(key, modifiers.into_wez())?;
            }
        } else {
            // dbg!(e); @todo figure out why this prints almost every keypress
        }

        Ok(())
    }

    fn event_text(&mut self, e: &Event, modifiers: Modifiers) -> TermResult {
        let Event::Text(t) = e else { unreachable!() };

        t.chars()
            .try_for_each(|c| {
                self.terminal
                    .key_down(wezterm_term::KeyCode::Char(c), modifiers.into_wez())
            })
            .and_then(|_| {
                t.chars().try_for_each(|c| {
                    self.terminal
                        .key_up(wezterm_term::KeyCode::Char(c), modifiers.into_wez())
                })
            })?;

        Ok(())
    }

    fn relative_pointer_pos(&self, response: &Response, i: &InputState) -> Vec2 {
        i.pointer.interact_pos().unwrap() - response.rect.min
    }

    fn manage_event(
        &mut self,
        event: &Event,
        response: &Response,
        i: &InputState,
    ) -> Result<(), Error> {
        match event {
            Event::PointerMoved(_) => self.event_pointer_move(event, response, i.modifiers),
            Event::PointerButton { .. } => self.event_pointer_button(event, response),
            Event::MouseWheel { .. } => {
                self.event_scroll(event, i.modifiers, self.relative_pointer_pos(response, i))
            }
            Event::Key { .. } => self.event_key(event),
            Event::Text(_) => self.event_text(event, i.modifiers),
            _ => Ok(()),
        }
    }

    fn manage_inputs(&mut self, i: &InputState, response: &Response) -> Vec<Result<(), Error>> {
        i.events
            .iter()
            .map(|event| self.manage_event(event, response, i))
            .collect()
    }

    fn generate_rows(&mut self, ui: &mut Ui, rows: Range<usize>) -> Response {
        let cursor_pos = self.terminal.cursor_pos();
        let palette = self.terminal.get_config().color_palette();
        let size = self.size;

        let mono_format = TextFormat {
            font_id: FontId::monospace(12.0),
            ..Default::default()
        };

        let mut job = egui::text::LayoutJob::default();
        let mut iter = self
            .terminal
            .screen()
            .lines_in_phys_range(rows)
            .into_iter()
            .peekable();
        while let Some(line) = iter.next() {
            line.cluster(None)
                .iter()
                .for_each(|c| job.append(&c.text, 0.0, self.format_to_egui(c)));
            if iter.peek().is_some() {
                job.append("\n", 0.0, mono_format.clone());
            }
        }

        let galley = ui.fonts(|f| f.layout_job(job));
        let mut galley_rect = galley.rect;
        galley_rect.set_width(self.text_width * size.cols as f32);

        let cursor = galley.cursor_from_pos(egui::vec2(cursor_pos.x as f32, cursor_pos.y as f32));
        let cursor_pos = galley.pos_from_cursor(&cursor);

        let (response, painter) =
            ui.allocate_painter(galley_rect.size(), egui::Sense::click_and_drag());

        if response.clicked() && !response.has_focus() {
            ui.memory_mut(|mem| mem.request_focus(response.id));
        }

        painter.rect_filled(
            galley_rect.translate(response.rect.min.to_vec2()),
            0.0,
            palette.background.into_egui(),
        );

        painter.galley(response.rect.min, galley, Color32::WHITE);

        painter.rect_stroke(
            egui::Rect::from_min_size(
                cursor_pos.min,
                egui::vec2(self.text_width, self.text_height),
            ),
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0, egui::Color32::WHITE),
            egui::StrokeKind::Middle,
        );

        // if ui.memory(|mem| mem.has_focus(response.id)) {

        ui.output_mut(|o| o.mutable_text_under_cursor = true);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        // ui.memory_mut(|mem| mem.lock_focus(response.id, true));

        if response.has_focus() {
            ui.input(|i| {
                self.manage_inputs(i, &response).iter().for_each(|res| {
                    let Err(e) = res else { return };
                    eprintln!("terminal input error {e:?}");
                });
            });
        }

        response
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, widget_size: egui::Vec2) -> Response {
        while let Ok(actions) = self.reader.try_recv() {
            self.terminal.perform_actions(actions);
        }

        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

        self.text_width = ui.fonts(|f| f.glyph_width(&egui::FontId::monospace(12.0), '?'));
        self.text_height = ui.text_style_height(&egui::TextStyle::Monospace);

        self.size.cols = (widget_size.x / self.text_width) as usize;
        self.size.rows = (widget_size.y / self.text_height) as usize;

        self.resize_rc();
        self.config(ui);

        let r = egui::ScrollArea::vertical()
            .max_height((self.size.rows + 1) as f32 * self.text_height)
            .stick_to_bottom(true)
            .id_salt(ui.next_auto_id())
            .show_rows(
                ui,
                self.text_height,
                self.terminal.screen().scrollback_rows(),
                |ui, rows| self.generate_rows(ui, rows),
            )
            .inner;

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));

        r
    }

    #[inline(never)]
    pub fn kill(&mut self) {
        if let Err(e) = self.child.kill() {
            eprintln!("error killing child: {e}");
        }
    }

    pub fn exit_status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    fn resize_rc(&mut self) {
        if self.terminal.get_size() == self.size {
            return;
        }

        self.terminal.resize(self.size);
        let r = self.pair.master.resize(PtySize {
            rows: self.size.rows as u16,
            cols: self.size.cols as u16,
            ..Default::default()
        });

        if let Err(e) = r {
            eprintln!("error resizing terminal: {e}");
        }
    }

    fn config(&mut self, ui: &Ui) {
        if *self.wez_config == *self.style.generate_wez_config(ui) {
            return;
        }
        self.wez_config = self.style.generate_wez_config(ui).clone();
        self.terminal.set_config(self.wez_config.clone());
    }
}
