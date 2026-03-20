use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

use crate::agent::{AgentProvider, AgentSessionFile};
use crate::config::Config;
use crate::model::session::{AgentActivity, AgentType};
use crate::model::tmux::TmuxPane;

pub struct OpenCodeProvider;

impl AgentProvider for OpenCodeProvider {
    fn agent_type(&self) -> AgentType {
        AgentType::OpenCode
    }

    fn detect_pane(&self, pane: &TmuxPane) -> bool {
        pane.current_command == "opencode"
    }

    fn detect_activity(&self, _pane: &TmuxPane) -> AgentActivity {
        AgentActivity::Unknown
    }

    fn clean_title<'a>(&self, title: &'a str) -> &'a str {
        title
    }

    fn gather_sessions(
        &self,
        _config: &Config,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<AgentSessionFile>>> + Send + '_>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
