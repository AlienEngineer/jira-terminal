use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::Frame;

#[derive(Debug, Default)]
pub struct App {
    pub counter: u8,
    exit: bool,
}

impl App {
    pub fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> crate::prelude::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> crate::prelude::Result<()> {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => self.exit = true,
                    KeyCode::Left => self.counter = self.counter.saturating_sub(1),
                    KeyCode::Right => self.counter = self.counter.saturating_add(1),
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
