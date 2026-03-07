use clap::{App, SubCommand};

pub fn subcommand() -> App<'static, 'static> {
    SubCommand::with_name("sprint")
        .about("Display the items in the active sprint as an interactive TUI. Requires board_id to be set in config.")
}
