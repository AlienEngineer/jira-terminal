use crate::jira::sprint::{self, Pbi};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Cell, Row, Table, TableState},
    Frame,
};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// Messages sent from the background "load all" thread to the UI thread.
enum LoadMsg {
    /// The thread is about to fetch this index.
    Progress(usize),
    /// Successfully fetched; carries the enriched Pbi.
    ItemDone(usize, Pbi),
    /// Fetch failed for this index.
    ItemError(usize, String),
    /// All items finished.
    AllDone,
}

pub struct SprintApp {
    pub sprint_name: String,
    pub sprint_goal: String,
    pub board_id: String,
    pub pbis: Vec<Pbi>,
    pub table_state: TableState,
    pub status_msg: String,
    /// Index currently being fetched (⟳ indicator).
    loading_idx: Option<usize>,
    /// Channel receiver for the async "load all" thread.
    load_rx: Option<mpsc::Receiver<LoadMsg>>,
    exit: bool,
}

impl SprintApp {
    pub fn new(sprint_name: String, sprint_goal: String, board_id: String, pbis: Vec<Pbi>) -> Self {
        let mut table_state = TableState::default();
        if !pbis.is_empty() {
            table_state.select(Some(0));
        }
        Self {
            sprint_name,
            sprint_goal,
            board_id,
            pbis,
            table_state,
            status_msg: String::new(),
            loading_idx: None,
            load_rx: None,
            exit: false,
        }
    }

    pub fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> crate::prelude::Result<()> {
        while !self.exit {
            // Drain any messages from the background "load all" thread first.
            self.process_load_messages();
            terminal.draw(|frame| self.draw(frame))?;

            // Poll with a short timeout so background loads can keep the UI
            // refreshing without requiring a key press.
            if event::poll(Duration::from_millis(50))? {
                self.handle_events(terminal)?;
            }
        }
        Ok(())
    }

    // ── Cache ────────────────────────────────────────────────────────────────

    fn save_cache(&self) {
        sprint::save_sprint_cache(
            &self.board_id,
            &self.sprint_name,
            &self.sprint_goal,
            &self.pbis,
        );
    }

    // ── Single-item load (synchronous, shows ⟳ before blocking) ─────────────

    fn load_selected(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> crate::prelude::Result<()> {
        let Some(i) = self.table_state.selected() else {
            return Ok(());
        };
        let key = self.pbis[i].key.clone();
        self.loading_idx = Some(i);
        self.status_msg = format!("Loading {}…", key);
        terminal.draw(|frame| self.draw(frame))?;

        match sprint::fetch_pbi_details(&mut self.pbis[i]) {
            Ok(()) => {
                sprint::sort_by_status(&mut self.pbis);
                self.status_msg = format!("Loaded {}", key);
                self.save_cache();
            }
            Err(e) => {
                self.status_msg = format!("Error loading {key}: {e}");
            }
        }
        self.loading_idx = None;
        Ok(())
    }

    // ── Bulk load (async thread → mpsc channel) ───────────────────────────────

    fn start_load_all(&mut self) {
        // Clone what the thread needs; it can't borrow self.
        let items: Vec<(usize, Pbi)> = self.pbis.iter().cloned().enumerate().collect();
        let (tx, rx) = mpsc::channel();
        self.load_rx = Some(rx);
        self.status_msg = format!("Loading all {} items…", items.len());

        thread::spawn(move || {
            for (idx, mut pbi) in items {
                let _ = tx.send(LoadMsg::Progress(idx));
                match sprint::fetch_pbi_details(&mut pbi) {
                    Ok(()) => {
                        let _ = tx.send(LoadMsg::ItemDone(idx, pbi));
                    }
                    Err(e) => {
                        let _ = tx.send(LoadMsg::ItemError(idx, e.to_string()));
                    }
                }
            }
            let _ = tx.send(LoadMsg::AllDone);
        });
    }

    /// Drain all pending messages from the background thread without blocking.
    fn process_load_messages(&mut self) {
        let done = {
            let Some(ref rx) = self.load_rx else { return };
            let total = self.pbis.len();
            let mut done = false;
            loop {
                match rx.try_recv() {
                    Ok(LoadMsg::Progress(idx)) => {
                        self.loading_idx = Some(idx);
                    }
                    Ok(LoadMsg::ItemDone(idx, pbi)) => {
                        if idx < self.pbis.len() {
                            self.pbis[idx] = pbi;
                        }
                        self.status_msg = format!("Loaded {}/{total}", idx + 1);
                    }
                    Ok(LoadMsg::ItemError(idx, e)) => {
                        let key = self
                            .pbis
                            .get(idx)
                            .map(|p| p.key.as_str())
                            .unwrap_or("?")
                            .to_string();
                        self.status_msg = format!("Error {key}: {e}");
                    }
                    Ok(LoadMsg::AllDone) => {
                        self.loading_idx = None;
                        sprint::sort_by_status(&mut self.pbis);
                        self.status_msg = format!("All {total} items loaded");
                        done = true;
                        break;
                    }
                    Err(_) => break, // empty or disconnected
                }
            }
            done
        };
        if done {
            self.load_rx = None;
            self.save_cache();
        }
    }

    // ── Rendering ────────────────────────────────────────────────────────────

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let goal_height = if self.sprint_goal.is_empty() { 0u16 } else { 3 };

        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(goal_height),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

        // Title bar
        frame.render_widget(
            Line::from(vec![
                Span::raw(" Sprint: "),
                Span::styled(
                    self.sprint_name.clone(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            layout[0],
        );

        // Sprint goal
        if !self.sprint_goal.is_empty() {
            use ratatui::widgets::{Paragraph, Wrap};
            frame.render_widget(
                Paragraph::new(self.sprint_goal.as_str())
                    .style(Style::default().fg(Color::White).italic())
                    .block(
                        Block::bordered()
                            .title(Span::styled(
                                " Goal ",
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ))
                            .border_style(Style::default().fg(Color::Yellow)),
                    )
                    .wrap(Wrap { trim: true }),
                layout[1],
            );
        }

        // Table
        let header = Row::new(
            ["", "Type", "Key", "Summary", "Status", "Assignee"]
                .iter()
                .map(|h| {
                    Cell::from(*h).style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                }),
        )
        .style(Style::default().bg(Color::DarkGray))
        .height(1);

        let rows: Vec<Row> = self
            .pbis
            .iter()
            .enumerate()
            .map(|(idx, pbi)| {
                let status_style = match pbi.status.to_lowercase().as_str() {
                    s if s.contains("done") || s.contains("closed") => {
                        Style::default().fg(Color::Green)
                    }
                    s if s.contains("progress") => Style::default().fg(Color::Blue),
                    s if s.contains("review") => Style::default().fg(Color::Magenta),
                    s if s.contains("blocked") => Style::default().fg(Color::Red),
                    _ => Style::default().fg(Color::White),
                };

                let indicator = if self.loading_idx == Some(idx) {
                    Cell::from("⟳").style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                } else if pbi.loaded {
                    Cell::from("✓").style(Style::default().fg(Color::Green))
                } else {
                    Cell::from(" ")
                };

                Row::new(vec![
                    indicator,
                    Cell::from(pbi.issue_type.clone()),
                    Cell::from(pbi.key.clone()).style(Style::default().fg(Color::Cyan)),
                    Cell::from(pbi.summary.clone()),
                    Cell::from(pbi.status.clone()).style(status_style),
                    Cell::from(pbi.assignee.clone()),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(2),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Min(40),
                Constraint::Length(18),
                Constraint::Length(20),
            ],
        )
        .header(header)
        .block(
            Block::bordered()
                .title(format!(" {} items ", self.pbis.len()))
                .title_alignment(ratatui::layout::Alignment::Right),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

        frame.render_stateful_widget(table, layout[2], &mut self.table_state);

        // Footer
        let status_span = if self.status_msg.is_empty() {
            Span::raw("")
        } else {
            Span::styled(
                format!("  [{}]", self.status_msg),
                Style::default().fg(Color::Green),
            )
        };
        frame.render_widget(
            Line::from(vec![
                Span::raw(" "),
                Span::styled("k/j", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Navigate  "),
                Span::styled("l", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Load line  "),
                Span::styled("L", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Load all  "),
                Span::styled("q", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Quit"),
                status_span,
            ]),
            layout[3],
        );
    }

    // ── Input ────────────────────────────────────────────────────────────────

    fn handle_events(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> crate::prelude::Result<()> {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        self.exit = true;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let next = self.table_state.selected().map_or(0, |i| {
                            if i >= self.pbis.len().saturating_sub(1) {
                                0
                            } else {
                                i + 1
                            }
                        });
                        self.table_state.select(Some(next));
                        self.status_msg.clear();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        let prev = self.table_state.selected().map_or(0, |i| {
                            if i == 0 {
                                self.pbis.len().saturating_sub(1)
                            } else {
                                i - 1
                            }
                        });
                        self.table_state.select(Some(prev));
                        self.status_msg.clear();
                    }
                    KeyCode::Char('l') => {
                        self.load_selected(terminal)?;
                    }
                    KeyCode::Char('L') => {
                        // Only start if not already loading
                        if self.load_rx.is_none() {
                            self.start_load_all();
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}
