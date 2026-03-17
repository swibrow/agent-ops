use std::collections::HashMap;

use tracing::{debug, warn};

use crate::model::session::AgentActivity;

/// Tracks previous activity states to detect transitions.
/// When an agent transitions to WaitingForPermission, fires a macOS notification.
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
    /// Returns the count of notifications sent.
    pub fn check_transitions(&mut self, current: &HashMap<String, AgentActivity>) -> usize {
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

            if transitioned_to_permission {
                debug!(session_id, "agent needs permission, sending notification");
                send_macos_notification(
                    "Agent needs attention",
                    &format!("Session {} is waiting for permission", session_id),
                );
                sent += 1;
            }
        }

        self.previous_activities = current.clone();
        sent
    }
}

fn send_macos_notification(title: &str, message: &str) {
    let script = format!(
        "display notification \"{}\" with title \"{}\" sound name \"Funk\"",
        message.replace('"', "\\\""),
        title.replace('"', "\\\""),
    );

    match std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
    {
        Ok(output) if !output.status.success() => {
            warn!("osascript notification failed: {:?}", output.status);
        }
        Err(e) => {
            warn!(error = %e, "failed to send notification");
        }
        _ => {}
    }
}
