use std::collections::HashMap;

use anyhow::Result;
use tokio::process::Command;
use tracing::warn;

#[derive(Debug, Clone, Default)]
pub struct ProcessStats {
    pub cpu_percent: f32,
    pub memory_mb: f32,
}

/// A snapshot of the system's process tree, used for both stats collection
/// and parent-child PID matching.
pub struct ProcessTree {
    children: HashMap<u32, Vec<u32>>,
    proc_data: HashMap<u32, (f32, f32)>, // pid -> (cpu%, rss_kb)
}

impl ProcessTree {
    /// Create an empty process tree (used as fallback when ps fails).
    pub fn empty() -> Self {
        Self {
            children: HashMap::new(),
            proc_data: HashMap::new(),
        }
    }

    /// Build a process tree from a single `ps -eo` call.
    pub async fn snapshot() -> Result<Self> {
        let output = Command::new("ps")
            .args(["-eo", "pid,ppid,%cpu,rss"])
            .output()
            .await?;

        if !output.status.success() {
            return Ok(Self {
                children: HashMap::new(),
                proc_data: HashMap::new(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut proc_data: HashMap<u32, (f32, f32)> = HashMap::new();

        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            let pid: u32 = match parts[0].parse() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let ppid: u32 = parts[1].parse().unwrap_or(0);
            let cpu: f32 = parts[2].parse().unwrap_or(0.0);
            let rss_kb: f32 = parts[3].parse().unwrap_or(0.0);

            children.entry(ppid).or_default().push(pid);
            proc_data.insert(pid, (cpu, rss_kb));
        }

        Ok(Self {
            children,
            proc_data,
        })
    }

    /// Check if `child_pid` is a descendant of `parent_pid`.
    pub fn is_descendant_of(&self, parent_pid: u32, child_pid: u32) -> bool {
        // Walk up from child to see if we reach parent
        let mut current = child_pid;
        let mut visited = std::collections::HashSet::new();
        loop {
            if current == parent_pid {
                return true;
            }
            if current == 0 || !visited.insert(current) {
                return false;
            }
            // Find parent of current
            let parent = self.parent_of(current);
            match parent {
                Some(p) => current = p,
                None => return false,
            }
        }
    }

    fn parent_of(&self, pid: u32) -> Option<u32> {
        for (&ppid, kids) in &self.children {
            if kids.contains(&pid) {
                return Some(ppid);
            }
        }
        None
    }

    /// Collect CPU% and RSS for a batch of PIDs, summing across all descendants.
    pub fn collect_stats(&self, pids: &[u32]) -> HashMap<u32, ProcessStats> {
        let mut result = HashMap::new();

        for &root_pid in pids {
            let mut total_cpu = 0.0f32;
            let mut total_rss = 0.0f32;

            // BFS through process tree
            let mut stack = vec![root_pid];
            while let Some(pid) = stack.pop() {
                if let Some(&(cpu, rss)) = self.proc_data.get(&pid) {
                    total_cpu += cpu;
                    total_rss += rss;
                }
                if let Some(kids) = self.children.get(&pid) {
                    stack.extend(kids);
                }
            }

            result.insert(
                root_pid,
                ProcessStats {
                    cpu_percent: total_cpu,
                    memory_mb: total_rss / 1024.0,
                },
            );
        }

        result
    }
}

/// Collect CPU/RAM stats for a batch of PIDs (convenience wrapper).
#[allow(dead_code)]
pub async fn collect_stats(pids: &[u32]) -> Result<HashMap<u32, ProcessStats>> {
    if pids.is_empty() {
        return Ok(HashMap::new());
    }

    let tree = match ProcessTree::snapshot().await {
        Ok(t) => t,
        Err(e) => {
            warn!(error = %e, "failed to snapshot process tree");
            return Ok(HashMap::new());
        }
    };

    Ok(tree.collect_stats(pids))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a manual process tree for testing:
    ///
    /// ```text
    ///   1 (init)
    ///   ├── 10 (shell)
    ///   │   ├── 100 (node)
    ///   │   │   └── 1000 (worker)
    ///   │   └── 101 (vim)
    ///   └── 20 (sshd)
    ///       └── 200 (bash)
    /// ```
    fn build_test_tree() -> ProcessTree {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        children.insert(1, vec![10, 20]);
        children.insert(10, vec![100, 101]);
        children.insert(100, vec![1000]);
        children.insert(20, vec![200]);

        let mut proc_data: HashMap<u32, (f32, f32)> = HashMap::new();
        proc_data.insert(1, (0.5, 1024.0));
        proc_data.insert(10, (1.0, 2048.0));
        proc_data.insert(100, (15.0, 102400.0));
        proc_data.insert(1000, (5.0, 51200.0));
        proc_data.insert(101, (2.0, 4096.0));
        proc_data.insert(20, (0.1, 512.0));
        proc_data.insert(200, (0.3, 1024.0));

        ProcessTree {
            children,
            proc_data,
        }
    }

    // ── is_descendant_of ─────────────────────────────────────────

    #[test]
    fn descendant_direct_child() {
        let tree = build_test_tree();
        assert!(tree.is_descendant_of(1, 10));
        assert!(tree.is_descendant_of(10, 100));
    }

    #[test]
    fn descendant_grandchild() {
        let tree = build_test_tree();
        assert!(tree.is_descendant_of(1, 100));
        assert!(tree.is_descendant_of(1, 1000));
        assert!(tree.is_descendant_of(10, 1000));
    }

    #[test]
    fn descendant_same_pid() {
        let tree = build_test_tree();
        assert!(tree.is_descendant_of(10, 10));
    }

    #[test]
    fn not_descendant_sibling() {
        let tree = build_test_tree();
        assert!(!tree.is_descendant_of(10, 20));
        assert!(!tree.is_descendant_of(100, 101));
    }

    #[test]
    fn not_descendant_reverse() {
        let tree = build_test_tree();
        assert!(!tree.is_descendant_of(100, 10));
        assert!(!tree.is_descendant_of(1000, 1));
    }

    #[test]
    fn not_descendant_different_branches() {
        let tree = build_test_tree();
        assert!(!tree.is_descendant_of(10, 200));
        assert!(!tree.is_descendant_of(20, 100));
    }

    #[test]
    fn not_descendant_unknown_pid() {
        let tree = build_test_tree();
        assert!(!tree.is_descendant_of(10, 9999));
        assert!(!tree.is_descendant_of(9999, 10));
    }

    #[test]
    fn descendant_empty_tree() {
        let tree = ProcessTree::empty();
        assert!(!tree.is_descendant_of(1, 2));
        // Same PID still returns true (trivially)
        assert!(tree.is_descendant_of(1, 1));
    }

    // ── collect_stats ────────────────────────────────────────────

    #[test]
    fn collect_stats_single_leaf() {
        let tree = build_test_tree();
        let stats = tree.collect_stats(&[101]);
        let s = stats.get(&101).unwrap();
        assert!((s.cpu_percent - 2.0).abs() < 0.01);
        assert!((s.memory_mb - 4096.0 / 1024.0).abs() < 0.01);
    }

    #[test]
    fn collect_stats_subtree_sums_descendants() {
        let tree = build_test_tree();
        let stats = tree.collect_stats(&[10]);
        let s = stats.get(&10).unwrap();
        // PID 10: 1.0 cpu, PID 100: 15.0, PID 1000: 5.0, PID 101: 2.0
        let expected_cpu = 1.0 + 15.0 + 5.0 + 2.0;
        assert!((s.cpu_percent - expected_cpu).abs() < 0.01);
        let expected_rss = (2048.0 + 102400.0 + 51200.0 + 4096.0) / 1024.0;
        assert!((s.memory_mb - expected_rss).abs() < 0.01);
    }

    #[test]
    fn collect_stats_root_sums_all() {
        let tree = build_test_tree();
        let stats = tree.collect_stats(&[1]);
        let s = stats.get(&1).unwrap();
        let expected_cpu = 0.5 + 1.0 + 15.0 + 5.0 + 2.0 + 0.1 + 0.3;
        assert!((s.cpu_percent - expected_cpu).abs() < 0.01);
    }

    #[test]
    fn collect_stats_unknown_pid_returns_zeros() {
        let tree = build_test_tree();
        let stats = tree.collect_stats(&[9999]);
        let s = stats.get(&9999).unwrap();
        assert!((s.cpu_percent).abs() < 0.01);
        assert!((s.memory_mb).abs() < 0.01);
    }

    #[test]
    fn collect_stats_empty_pids() {
        let tree = build_test_tree();
        let stats = tree.collect_stats(&[]);
        assert!(stats.is_empty());
    }

    #[test]
    fn collect_stats_multiple_pids() {
        let tree = build_test_tree();
        let stats = tree.collect_stats(&[100, 200]);
        assert_eq!(stats.len(), 2);
        assert!(stats.contains_key(&100));
        assert!(stats.contains_key(&200));
    }

    // ── parent_of ────────────────────────────────────────────────

    #[test]
    fn parent_of_known() {
        let tree = build_test_tree();
        assert_eq!(tree.parent_of(10), Some(1));
        assert_eq!(tree.parent_of(100), Some(10));
        assert_eq!(tree.parent_of(1000), Some(100));
        assert_eq!(tree.parent_of(200), Some(20));
    }

    #[test]
    fn parent_of_root_is_none() {
        let tree = build_test_tree();
        // PID 1 has no parent in this tree
        assert_eq!(tree.parent_of(1), None);
    }

    #[test]
    fn parent_of_unknown_is_none() {
        let tree = build_test_tree();
        assert_eq!(tree.parent_of(9999), None);
    }
}
