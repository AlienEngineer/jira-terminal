use crate::config;
use crate::jira::api;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

/// A single Product Backlog Item (PBI) from a sprint.
#[derive(Debug, Clone)]
pub struct Pbi {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub assignee: String,
    pub issue_type: String,
    // Rich details — populated when the user presses l/L
    pub description: Option<String>,
    pub priority: Option<String>,
    pub story_points: Option<f64>,
    pub labels: Vec<String>,
    pub loaded: bool,
}

/// Numeric sort key for a status string: lower = earlier in the workflow.
///
/// Order: To Do → In Progress → In Review → Blocked → Done / Closed
fn status_sort_key(status: &str) -> u8 {
    let s = status.to_lowercase();
    if s.contains("done") || s.contains("closed") || s.contains("resolved") {
        4
    } else if s.contains("blocked") {
        3
    } else if s.contains("review") {
        2
    } else if s.contains("progress") {
        1
    } else {
        // "To Do", "Open", "Backlog", anything unrecognised → first
        0
    }
}

/// Sort a PBI slice in ascending workflow order (new → resolved).
pub fn sort_by_status(pbis: &mut [Pbi]) {
    pbis.sort_by_key(|p| status_sort_key(&p.status));
}

fn cache_path(board_id: &str) -> PathBuf {
    let config_file = config::get_config_file_name();
    let config_dir = PathBuf::from(&config_file)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    config_dir.join(format!("sprint_cache_{board_id}.json"))
}

/// Load sprint data from the on-disk cache. Returns `None` if the cache does
/// not exist or is malformed.
pub fn load_sprint_cache(board_id: &str) -> Option<(String, String, Vec<Pbi>)> {
    let path = cache_path(board_id);
    let mut file = fs::File::open(path).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let data = json::parse(&contents).ok()?;

    let sprint_name = data["sprint_name"].as_str()?.to_string();
    let sprint_goal = data["sprint_goal"].as_str().unwrap_or("").to_string();

    let mut pbis = Vec::new();
    for item in data["pbis"].members() {
        let labels = item["labels"]
            .members()
            .filter_map(|l| l.as_str().map(|s| s.to_string()))
            .collect();
        pbis.push(Pbi {
            key: item["key"].as_str().unwrap_or("").to_string(),
            summary: item["summary"].as_str().unwrap_or("").to_string(),
            status: item["status"].as_str().unwrap_or("").to_string(),
            assignee: item["assignee"]
                .as_str()
                .unwrap_or("Unassigned")
                .to_string(),
            issue_type: item["issue_type"].as_str().unwrap_or("").to_string(),
            description: item["description"].as_str().map(|s| s.to_string()),
            priority: item["priority"].as_str().map(|s| s.to_string()),
            story_points: item["story_points"].as_f64(),
            labels,
            loaded: item["loaded"].as_bool().unwrap_or(false),
        });
    }

    Some((sprint_name, sprint_goal, pbis))
}

/// Persist sprint data to the on-disk cache.
pub fn save_sprint_cache(board_id: &str, sprint_name: &str, sprint_goal: &str, pbis: &[Pbi]) {
    let mut pbis_json = json::JsonValue::new_array();
    for pbi in pbis {
        let mut labels_json = json::JsonValue::new_array();
        for label in &pbi.labels {
            let _ = labels_json.push(label.as_str());
        }
        let mut obj = json::object! {
            "key": pbi.key.as_str(),
            "summary": pbi.summary.as_str(),
            "status": pbi.status.as_str(),
            "assignee": pbi.assignee.as_str(),
            "issue_type": pbi.issue_type.as_str(),
            "loaded": pbi.loaded,
            "labels": labels_json,
        };
        obj["description"] = match &pbi.description {
            Some(d) => json::JsonValue::String(d.clone()),
            None => json::JsonValue::Null,
        };
        obj["priority"] = match &pbi.priority {
            Some(p) => json::JsonValue::String(p.clone()),
            None => json::JsonValue::Null,
        };
        obj["story_points"] = match pbi.story_points {
            Some(sp) => json::JsonValue::Number(sp.into()),
            None => json::JsonValue::Null,
        };
        let _ = pbis_json.push(obj);
    }

    let data = json::object! {
        "sprint_name": sprint_name,
        "sprint_goal": sprint_goal,
        "pbis": pbis_json,
    };

    let path = cache_path(board_id);
    if let Ok(mut file) = fs::File::create(path) {
        let _ = file.write_all(json::stringify_pretty(data, 2).as_bytes());
    }
}

// ── API helpers ──────────────────────────────────────────────────────────────

/// Fetch issues for the active sprint on the given board.
///
/// Returns a tuple of (sprint_name, sprint_goal, Vec<Pbi>).
pub fn fetch_active_sprint_issues(
    board_id: &str,
) -> Result<(String, String, Vec<Pbi>), Box<dyn Error>> {
    // 1. Find the active sprint for the board
    let sprints_response = api::get_agile_call(format!("board/{board_id}/sprint?state=active"))?;
    let sprints = &sprints_response["values"];
    if !sprints.is_array() || sprints.is_empty() {
        return Err("No active sprint found for the given board.".into());
    }
    let sprint = &sprints[0];
    let sprint_id = sprint["id"].as_u64().ok_or("Could not read sprint id")?;
    let sprint_name = sprint["name"]
        .as_str()
        .unwrap_or("Active Sprint")
        .to_string();
    let sprint_goal = sprint["goal"].as_str().unwrap_or("").to_string();

    // 2. Fetch all issues for that sprint (up to 500)
    let issues_response = api::get_agile_call(format!("sprint/{sprint_id}/issue?maxResults=500"))?;
    let issues = &issues_response["issues"];

    let mut pbis = Vec::new();
    if issues.is_array() {
        for issue in issues.members() {
            let key = issue["key"].as_str().unwrap_or("").to_string();
            let fields = &issue["fields"];
            let summary = fields["summary"].as_str().unwrap_or("").to_string();
            let status = fields["status"]["name"].as_str().unwrap_or("-").to_string();
            let assignee = fields["assignee"]["displayName"]
                .as_str()
                .unwrap_or("Unassigned")
                .to_string();
            let issue_type = fields["issuetype"]["name"]
                .as_str()
                .unwrap_or("-")
                .to_string();
            pbis.push(Pbi {
                key,
                summary,
                status,
                assignee,
                issue_type,
                description: None,
                priority: None,
                story_points: None,
                labels: Vec::new(),
                loaded: false,
            });
        }
    }

    sort_by_status(&mut pbis);
    Ok((sprint_name, sprint_goal, pbis))
}

/// Fetch and populate rich details for a single PBI in place.
pub fn fetch_pbi_details(pbi: &mut Pbi) -> Result<(), Box<dyn Error>> {
    let response = api::get_call_v2(format!("issue/{}", pbi.key))?;
    let fields = &response["fields"];

    // Description: v2 returns plain text or ADF; try string first
    let desc = &fields["description"];
    pbi.description = if desc.is_string() {
        desc.as_str().map(|s| s.to_string())
    } else if desc.is_object() {
        Some(extract_adf_text(desc))
    } else {
        None
    };

    pbi.priority = fields["priority"]["name"].as_str().map(|s| s.to_string());

    // Story points live in different custom fields depending on the JIRA setup
    pbi.story_points = fields["story_points"]
        .as_f64()
        .or_else(|| fields["customfield_10016"].as_f64())
        .or_else(|| fields["customfield_10028"].as_f64());

    pbi.labels = fields["labels"]
        .members()
        .filter_map(|l| l.as_str().map(|s| s.to_string()))
        .collect();

    // Refresh status and assignee in case they changed
    if let Some(s) = fields["status"]["name"].as_str() {
        pbi.status = s.to_string();
    }
    if let Some(a) = fields["assignee"]["displayName"].as_str() {
        pbi.assignee = a.to_string();
    }

    pbi.loaded = true;
    Ok(())
}

/// Recursively extract plain text from an Atlassian Document Format (ADF) node.
fn extract_adf_text(node: &json::JsonValue) -> String {
    let mut text = String::new();
    if let Some(t) = node["text"].as_str() {
        text.push_str(t);
    }
    for child in node["content"].members() {
        let child_text = extract_adf_text(child);
        if !child_text.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&child_text);
        }
    }
    text
}
