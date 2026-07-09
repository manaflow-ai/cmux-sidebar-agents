use std::{cmp::Ordering, collections::HashMap};

use cmux_client::Tree;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRecord {
    pub surface: u64,
    pub state: String,
    pub session: Option<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceTarget {
    pub surface: u64,
    pub workspace_index: usize,
    pub screen_index: usize,
    pub pane: u64,
    pub tab_index: usize,
    pub breadcrumb: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    NeedsAttention,
    Running,
    Done,
}

impl AgentStatus {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::NeedsAttention => "⚠",
            Self::Running => "●",
            Self::Done => "✔",
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::NeedsAttention => 0,
            Self::Running => 1,
            Self::Done => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub surface: u64,
    pub status: AgentStatus,
    pub name: String,
    pub breadcrumb: String,
    pub age: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentNotification {
    pub id: u64,
    pub title: String,
    pub level: String,
    pub surface: Option<u64>,
    pub breadcrumb: String,
    pub unread: bool,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadNotification {
    pub id: u64,
    pub surface: u64,
    pub unread: bool,
    pub level: String,
}

pub fn index_surfaces(tree: &Tree) -> HashMap<u64, SurfaceTarget> {
    let mut targets = HashMap::new();

    for (workspace_index, workspace) in tree.workspaces.iter().enumerate() {
        for (screen_index, screen) in workspace.screens.iter().enumerate() {
            let screen_name = screen
                .name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("screen {}", screen_index + 1));
            let breadcrumb = format!("{} > {screen_name}", workspace.name);

            for pane in &screen.panes {
                for (tab_index, tab) in pane.tabs.iter().enumerate() {
                    let display_name = tab
                        .name
                        .clone()
                        .filter(|name| !name.is_empty())
                        .or_else(|| (!tab.title.is_empty()).then(|| tab.title.clone()))
                        .unwrap_or_else(|| format!("surface {}", tab.surface));
                    targets.insert(
                        tab.surface,
                        SurfaceTarget {
                            surface: tab.surface,
                            workspace_index,
                            screen_index,
                            pane: pane.id,
                            tab_index,
                            breadcrumb: breadcrumb.clone(),
                            display_name,
                        },
                    );
                }
            }
        }
    }

    targets
}

pub fn rows_from_records(
    records: &[AgentRecord],
    targets: &HashMap<u64, SurfaceTarget>,
    now_ms: u64,
) -> Vec<AgentRow> {
    let mut rows = records
        .iter()
        .map(|record| record_to_row(record, targets.get(&record.surface), now_ms))
        .collect::<Vec<_>>();
    rows.sort_by(compare_rows);
    rows
}

fn record_to_row(record: &AgentRecord, target: Option<&SurfaceTarget>, now_ms: u64) -> AgentRow {
    let name = record
        .session
        .as_ref()
        .filter(|name| !name.is_empty())
        .cloned()
        .or_else(|| target.map(|target| target.display_name.clone()))
        .unwrap_or_else(|| format!("agent {}", record.surface));
    let breadcrumb = target
        .map(|target| target.breadcrumb.clone())
        .unwrap_or_else(|| format!("surface {}", record.surface));

    AgentRow {
        surface: record.surface,
        status: status_from_state(&record.state),
        name,
        breadcrumb,
        age: format_age(now_ms.saturating_sub(record.updated_at_ms)),
        updated_at_ms: record.updated_at_ms,
    }
}

fn compare_rows(left: &AgentRow, right: &AgentRow) -> Ordering {
    left.status
        .priority()
        .cmp(&right.status.priority())
        .then_with(|| right.updated_at_ms.cmp(&left.updated_at_ms))
        .then_with(|| left.name.cmp(&right.name))
}

pub fn status_from_state(state: &str) -> AgentStatus {
    match state {
        "working" => AgentStatus::Running,
        "done" | "idle" => AgentStatus::Done,
        "blocked" | "unknown" => AgentStatus::NeedsAttention,
        _ => AgentStatus::NeedsAttention,
    }
}

pub fn format_age(age_ms: u64) -> String {
    let seconds = age_ms / 1_000;
    match seconds {
        0 => "now".to_string(),
        1..=59 => format!("{seconds}s"),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_client::{Layout, Pane, Screen, Tab, Workspace};

    #[test]
    fn maps_record_to_glyph_name_breadcrumb_and_age() {
        let targets = index_surfaces(&sample_tree());
        let rows = rows_from_records(
            &[AgentRecord {
                surface: 42,
                state: "working".to_string(),
                session: Some("reviewer".to_string()),
                updated_at_ms: 1_000,
            }],
            &targets,
            126_000,
        );

        assert_eq!(rows[0].status.glyph(), "●");
        assert_eq!(rows[0].name, "reviewer");
        assert_eq!(rows[0].breadcrumb, "project > tests");
        assert_eq!(rows[0].age, "2m");
    }

    #[test]
    fn falls_back_to_surface_title_for_agent_name() {
        let targets = index_surfaces(&sample_tree());
        let rows = rows_from_records(
            &[AgentRecord {
                surface: 42,
                state: "blocked".to_string(),
                session: None,
                updated_at_ms: 1,
            }],
            &targets,
            1,
        );

        assert_eq!(rows[0].name, "codex");
        assert_eq!(rows[0].status.glyph(), "⚠");
    }

    #[test]
    fn sorts_attention_then_running_then_done_and_newest_within_group() {
        let records = vec![
            record(1, "done", 500),
            record(2, "working", 200),
            record(3, "blocked", 100),
            record(4, "blocked", 900),
            record(5, "working", 800),
        ];
        let rows = rows_from_records(&records, &HashMap::new(), 1_000);

        assert_eq!(
            rows.iter().map(|row| row.surface).collect::<Vec<_>>(),
            [4, 3, 5, 2, 1]
        );
    }

    #[test]
    fn formats_age_units() {
        assert_eq!(format_age(999), "now");
        assert_eq!(format_age(12_000), "12s");
        assert_eq!(format_age(180_000), "3m");
        assert_eq!(format_age(7_200_000), "2h");
        assert_eq!(format_age(345_600_000), "4d");
    }

    #[test]
    fn maps_all_wire_states_to_display_groups() {
        assert_eq!(status_from_state("working"), AgentStatus::Running);
        assert_eq!(status_from_state("done"), AgentStatus::Done);
        assert_eq!(status_from_state("idle"), AgentStatus::Done);
        assert_eq!(status_from_state("blocked"), AgentStatus::NeedsAttention);
        assert_eq!(status_from_state("unknown"), AgentStatus::NeedsAttention);
    }

    fn record(surface: u64, state: &str, updated_at_ms: u64) -> AgentRecord {
        AgentRecord {
            surface,
            state: state.to_string(),
            session: Some(format!("agent-{surface}")),
            updated_at_ms,
        }
    }

    fn sample_tree() -> Tree {
        Tree {
            workspaces: vec![Workspace {
                id: 1,
                name: "project".to_string(),
                active: true,
                screens: vec![Screen {
                    id: 2,
                    name: Some("tests".to_string()),
                    active: true,
                    active_pane: 3,
                    layout: Layout::Leaf { pane: 3 },
                    panes: vec![Pane {
                        id: 3,
                        name: None,
                        active_tab: 0,
                        tabs: vec![Tab {
                            surface: 42,
                            kind: "terminal".to_string(),
                            browser_source: None,
                            name: None,
                            title: "codex".to_string(),
                            size: None,
                            dead: false,
                        }],
                        dead: false,
                    }],
                }],
            }],
        }
    }
}
