use std::collections::{HashMap, HashSet};

use tracing::{debug, warn};

use crate::model::session::{AgentActivity, AgentType};

/// Extra context about a session for richer notifications.
pub struct SessionInfo {
    pub agent_type: AgentType,
    pub project_name: String,
}

/// Tracks previous activity states to detect transitions.
/// When an agent transitions to WaitingForPermission, fires a macOS notification.
/// Skips notifications for sessions whose pane is currently active (user is looking at it).
pub struct NotificationTracker {
    previous_activities: HashMap<String, AgentActivity>,
    enabled: bool,
}

impl NotificationTracker {
    pub fn new(enabled: bool) -> Self {
        Self {
            previous_activities: HashMap::new(),
            enabled,
        }
    }

    /// Check for activity transitions and send notifications.
    /// `session_info` maps session_id -> (agent_type, project_name) for richer messages.
    /// `active_session_ids` contains sessions whose pane is currently focused — these are skipped.
    /// Returns the count of notifications sent.
    pub fn check_transitions(
        &mut self,
        current: &HashMap<String, AgentActivity>,
        session_info: &HashMap<String, SessionInfo>,
        active_session_ids: &HashSet<String>,
    ) -> usize {
        if !self.enabled {
            self.previous_activities = current.clone();
            return 0;
        }

        let mut sent = 0;

        for (session_id, new_activity) in current {
            let prev = self.previous_activities.get(session_id);
            let transitioned_to_permission =
                matches!(new_activity, AgentActivity::WaitingForPermission)
                    && !matches!(prev, Some(AgentActivity::WaitingForPermission));

            if transitioned_to_permission && !active_session_ids.contains(session_id) {
                debug!(session_id, "agent needs permission, sending notification");
                let info = session_info.get(session_id);
                send_notification(session_id, info);
                sent += 1;
            }
        }

        self.previous_activities = current.clone();
        sent
    }
}

fn send_notification(session_id: &str, info: Option<&SessionInfo>) {
    let (summary, body) = match info {
        Some(info) => (
            format!("{} needs attention", info.agent_type.label()),
            format!(
                "{} agent in \"{}\" is waiting for permission",
                info.agent_type.label(),
                info.project_name,
            ),
        ),
        None => (
            "Agent needs attention".to_string(),
            format!("Session {} is waiting for permission", session_id),
        ),
    };

    if let Err(e) = notify_rust::Notification::new()
        .summary(&summary)
        .body(&body)
        .sound_name("Funk")
        .show()
    {
        warn!(error = %e, "failed to send notification");
    }
}
