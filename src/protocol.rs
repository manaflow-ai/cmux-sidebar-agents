use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use cmux_client::{CmuxClient, Tree};
use serde_json::{Map, Value};

use crate::model::{AgentRecord, SurfaceTarget, UnreadNotification, index_surfaces};

pub struct Snapshot {
    pub agents: Vec<AgentRecord>,
    pub targets: HashMap<u64, SurfaceTarget>,
    pub unread: Vec<UnreadNotification>,
}

pub fn load_snapshot(client: &mut CmuxClient) -> Result<Snapshot> {
    let tree_data = raw_command(client, "list-workspaces")?;
    let tree: Tree =
        serde_json::from_value(tree_data.clone()).context("invalid list-workspaces response")?;
    let targets = index_surfaces(&tree);
    let unread = unread_notifications(&tree_data);

    let agents_data = raw_command(client, "list-agents")?;
    let agents = parse_agents(&agents_data)?;

    Ok(Snapshot {
        agents,
        targets,
        unread,
    })
}

fn raw_command(client: &mut CmuxClient, cmd: &str) -> Result<Value> {
    let mut request = Map::new();
    request.insert("cmd".to_string(), Value::from(cmd));
    let response = client.send_raw(request)?;
    if response.get("ok") != Some(&Value::Bool(true)) {
        let message = response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown cmux error");
        bail!("{cmd}: {message}");
    }
    Ok(response
        .get("data")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new())))
}

fn parse_agents(data: &Value) -> Result<Vec<AgentRecord>> {
    let records = data
        .get("agents")
        .and_then(Value::as_array)
        .context("list-agents response has no agents array")?;
    records
        .iter()
        .map(|record| {
            Ok(AgentRecord {
                surface: record
                    .get("surface")
                    .and_then(Value::as_u64)
                    .context("agent record has no numeric surface")?,
                state: record
                    .get("state")
                    .and_then(Value::as_str)
                    .context("agent record has no state")?
                    .to_string(),
                session: record
                    .get("session")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                updated_at_ms: record
                    .get("updated_at_ms")
                    .and_then(Value::as_u64)
                    .context("agent record has no updated_at_ms")?,
            })
        })
        .collect()
}

fn unread_notifications(data: &Value) -> Vec<UnreadNotification> {
    let Some(workspaces) = data.get("workspaces").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut notifications = Vec::new();
    for workspace in workspaces {
        let Some(screens) = workspace.get("screens").and_then(Value::as_array) else {
            continue;
        };
        for screen in screens {
            let Some(panes) = screen.get("panes").and_then(Value::as_array) else {
                continue;
            };
            for pane in panes {
                let Some(tabs) = pane.get("tabs").and_then(Value::as_array) else {
                    continue;
                };
                for tab in tabs {
                    let Some(surface) = tab.get("surface").and_then(Value::as_u64) else {
                        continue;
                    };
                    let Some(notification) = tab.get("notification").and_then(Value::as_object)
                    else {
                        continue;
                    };
                    let Some(id) = notification.get("notification").and_then(Value::as_u64) else {
                        continue;
                    };
                    notifications.push(UnreadNotification {
                        id,
                        surface,
                        unread: notification
                            .get("unread")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        level: notification
                            .get("level")
                            .and_then(Value::as_str)
                            .unwrap_or("info")
                            .to_string(),
                    });
                }
            }
        }
    }
    notifications
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_tab_notification_metadata() {
        let data = json!({
            "workspaces": [{"screens": [{"panes": [{"tabs": [
                {"surface": 7, "notification": {"notification": 9, "unread": true, "level": "warning"}},
                {"surface": 8, "notification": null}
            ]}]}]}]
        });

        assert_eq!(
            unread_notifications(&data),
            vec![UnreadNotification {
                id: 9,
                surface: 7,
                unread: true,
                level: "warning".to_string(),
            }]
        );
    }
}
