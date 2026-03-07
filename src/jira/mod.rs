pub mod api;
mod assign;
pub mod comments;
pub mod details;
mod fields;
pub mod lists;
mod logout;
mod new_issue;
pub mod sprint;
pub mod transitions;
mod update;
pub mod utils;

extern crate clap;
use clap::ArgMatches;

pub fn handle_transition_matches(matches: &ArgMatches) {
    let ticket = matches.value_of("transition_ticket").unwrap();
    if matches.is_present("transition_list") {
        transitions::print_transition_lists(ticket.to_string());
    } else {
        let status = matches.value_of("STATUS").unwrap();
        transitions::move_ticket_status(ticket.to_string(), status.to_string());
    }
}

pub fn handle_logout(_matches: &ArgMatches) {
    logout::delete_configuration();
}

pub fn handle_fields_matches(matches: &ArgMatches) {
    let ticket = String::from(matches.value_of("TICKET").unwrap());
    fields::display_all_fields(ticket);
}

pub fn handle_list_matches(matches: &ArgMatches) {
    lists::list_issues(matches);
}

pub fn handle_detail_matches(matches: &ArgMatches) {
    let ticket = String::from(matches.value_of("TICKET").unwrap());
    let fields = String::from(
        matches
            .value_of("fields")
            .unwrap_or("key,summary,description"),
    );
    details::show_details(ticket, fields);
}

pub fn handle_update_matches(matches: &ArgMatches) {
    let ticket = String::from(matches.value_of("TICKET").unwrap());
    let field = String::from(matches.value_of("field").unwrap());
    let value = String::from(matches.value_of("value").unwrap());
    update::update_jira_ticket(ticket, field, value);
}

pub fn handle_new_matches(matches: &ArgMatches) {
    new_issue::handle_issue_creation(matches);
}

pub fn handle_assign_matches(matches: &ArgMatches) {
    let ticket = String::from(matches.value_of("ticket").unwrap());
    let user = String::from(matches.value_of("user").unwrap());
    assign::assign_task(ticket, user);
}

pub fn handle_comments_matches(matches: &ArgMatches) {
    let ticket = String::from(matches.value_of("ticket").unwrap());
    if matches.is_present("list") {
        comments::get_all_comments(ticket);
        return;
    }
    comments::add_new_comment(ticket, matches);
}

pub fn handle_sprint(_matches: &ArgMatches) {
    use crate::config;
    use crate::ui::sprint_list::SprintApp;

    let board_id = config::get_config("board_id".to_string());
    if board_id.is_empty() {
        eprintln!(
            "No board_id found in configuration.\n\
             Please set it first with:\n\
             \n\
             jira-terminal config set board_id <YOUR_BOARD_ID>"
        );
        std::process::exit(1);
    }

    // Try the on-disk cache first; fall back to a fresh API fetch
    let (sprint_name, sprint_goal, pbis) =
        if let Some(cached) = sprint::load_sprint_cache(&board_id) {
            println!("Loaded sprint from cache. Press L to refresh all items.");
            cached
        } else {
            println!("Fetching active sprint for board {board_id}...");
            match sprint::fetch_active_sprint_issues(&board_id) {
                Err(e) => {
                    eprintln!("Error fetching sprint: {e}");
                    std::process::exit(1);
                }
                Ok(data) => {
                    sprint::save_sprint_cache(&board_id, &data.0, &data.1, &data.2);
                    data
                }
            }
        };

    let mut terminal = ratatui::init();
    let result = SprintApp::new(sprint_name, sprint_goal, board_id, pbis).run(&mut terminal);
    ratatui::restore();
    if let Err(e) = result {
        eprintln!("TUI error: {e}");
        std::process::exit(1);
    }
}
