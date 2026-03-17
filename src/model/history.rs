#[derive(Debug, Clone, serde::Deserialize)]
pub struct HistoryEntry {
    pub display: String,
    pub timestamp: i64,
    pub project: String,
    #[serde(default)]
    pub session_id: Option<String>,
}
