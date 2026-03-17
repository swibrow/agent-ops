use ratatui::style::Color;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum StalenessLevel {
    Hot,
    Warm,
    Cool,
    Cold,
    Frozen,
    Forgotten,
}

impl StalenessLevel {
    pub fn from_hours(hours: i64) -> Self {
        match hours {
            ..=0 => Self::Hot,
            1..=24 => Self::Warm,
            25..=168 => Self::Cool,     // 1-7 days
            169..=720 => Self::Cold,    // 7-30 days
            721..=2160 => Self::Frozen, // 30-90 days
            _ => Self::Forgotten,       // > 90 days
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Hot => Color::Green,
            Self::Warm => Color::Yellow,
            Self::Cool => Color::Blue,
            Self::Cold => Color::Cyan,
            Self::Frozen => Color::Gray,
            Self::Forgotten => Color::Red,
        }
    }

    #[allow(dead_code)]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hot => "HOT",
            Self::Warm => "WARM",
            Self::Cool => "COOL",
            Self::Cold => "COLD",
            Self::Frozen => "FROZEN",
            Self::Forgotten => "FORGOTTEN",
        }
    }

    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Hot => "●",
            Self::Warm => "◉",
            Self::Cool => "○",
            Self::Cold => "◌",
            Self::Frozen => "❄",
            Self::Forgotten => "⚠",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub path: String,
    pub name: String,
    pub first_seen: i64,
    pub last_activity: i64,
    pub total_sessions: u32,
    pub total_messages: u32,
    pub is_active: bool,
    pub staleness: StalenessLevel,
    pub daily_activity: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectSort {
    Name,
    LastActivity,
    Sessions,
    Staleness,
}

impl ProjectSort {
    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::LastActivity,
            Self::LastActivity => Self::Sessions,
            Self::Sessions => Self::Staleness,
            Self::Staleness => Self::Name,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::LastActivity => "Last Activity",
            Self::Sessions => "Sessions",
            Self::Staleness => "Staleness",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── StalenessLevel::from_hours boundary tests ────────────────

    #[test]
    fn staleness_negative_hours_is_hot() {
        assert_eq!(StalenessLevel::from_hours(-100), StalenessLevel::Hot);
        assert_eq!(StalenessLevel::from_hours(-1), StalenessLevel::Hot);
    }

    #[test]
    fn staleness_zero_is_hot() {
        assert_eq!(StalenessLevel::from_hours(0), StalenessLevel::Hot);
    }

    #[test]
    fn staleness_one_hour_is_warm() {
        assert_eq!(StalenessLevel::from_hours(1), StalenessLevel::Warm);
    }

    #[test]
    fn staleness_24_hours_is_warm() {
        assert_eq!(StalenessLevel::from_hours(24), StalenessLevel::Warm);
    }

    #[test]
    fn staleness_25_hours_is_cool() {
        assert_eq!(StalenessLevel::from_hours(25), StalenessLevel::Cool);
    }

    #[test]
    fn staleness_168_hours_is_cool() {
        assert_eq!(StalenessLevel::from_hours(168), StalenessLevel::Cool);
    }

    #[test]
    fn staleness_169_hours_is_cold() {
        assert_eq!(StalenessLevel::from_hours(169), StalenessLevel::Cold);
    }

    #[test]
    fn staleness_720_hours_is_cold() {
        assert_eq!(StalenessLevel::from_hours(720), StalenessLevel::Cold);
    }

    #[test]
    fn staleness_721_hours_is_frozen() {
        assert_eq!(StalenessLevel::from_hours(721), StalenessLevel::Frozen);
    }

    #[test]
    fn staleness_2160_hours_is_frozen() {
        assert_eq!(StalenessLevel::from_hours(2160), StalenessLevel::Frozen);
    }

    #[test]
    fn staleness_2161_hours_is_forgotten() {
        assert_eq!(StalenessLevel::from_hours(2161), StalenessLevel::Forgotten);
    }

    #[test]
    fn staleness_very_large_is_forgotten() {
        assert_eq!(
            StalenessLevel::from_hours(100_000),
            StalenessLevel::Forgotten
        );
    }

    // ── StalenessLevel ordering ──────────────────────────────────

    #[test]
    fn staleness_ordering() {
        assert!(StalenessLevel::Hot < StalenessLevel::Warm);
        assert!(StalenessLevel::Warm < StalenessLevel::Cool);
        assert!(StalenessLevel::Cool < StalenessLevel::Cold);
        assert!(StalenessLevel::Cold < StalenessLevel::Frozen);
        assert!(StalenessLevel::Frozen < StalenessLevel::Forgotten);
    }

    // ── StalenessLevel::indicator / label / color ────────────────

    #[test]
    fn staleness_indicators_are_non_empty() {
        let levels = [
            StalenessLevel::Hot,
            StalenessLevel::Warm,
            StalenessLevel::Cool,
            StalenessLevel::Cold,
            StalenessLevel::Frozen,
            StalenessLevel::Forgotten,
        ];
        for level in &levels {
            assert!(!level.indicator().is_empty());
            assert!(!level.label().is_empty());
        }
    }

    // ── ProjectSort::next cycles ─────────────────────────────────

    #[test]
    fn project_sort_cycles_through_all_variants() {
        let start = ProjectSort::Name;
        let second = start.next();
        assert_eq!(second, ProjectSort::LastActivity);
        let third = second.next();
        assert_eq!(third, ProjectSort::Sessions);
        let fourth = third.next();
        assert_eq!(fourth, ProjectSort::Staleness);
        let back = fourth.next();
        assert_eq!(back, ProjectSort::Name);
    }

    #[test]
    fn project_sort_labels_are_non_empty() {
        let sorts = [
            ProjectSort::Name,
            ProjectSort::LastActivity,
            ProjectSort::Sessions,
            ProjectSort::Staleness,
        ];
        for s in &sorts {
            assert!(!s.label().is_empty());
        }
    }
}
