use anyhow::Result;
use log::debug;
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};

const MB: u64 = 1024 * 1024;

// New tiered memory thresholds
const MEMORY_GREEN_MAX: u64 = 150;      // <150MB green
const MEMORY_YELLOW_MAX: u64 = 250;     // 150-250MB yellow
const MEMORY_ORANGE_MAX: u64 = 400;     // 250-400MB orange
// >400MB red

const MEMORY_LIMIT_MB: u64 = 400;       // Hard limit
const WARNING_THRESHOLD_MB: u64 = 250;  // Yellow warning

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    /// Current memory usage in MB
    pub used_mb: f64,
    /// Memory limit in MB
    pub limit_mb: u64,
    /// Warning threshold in MB
    pub warning_threshold_mb: u64,
    /// Whether memory usage is approaching the limit
    pub is_approaching_limit: bool,
    /// Whether memory usage has exceeded the limit
    pub is_over_limit: bool,
    /// Percentage of limit being used
    pub percentage_used: f64,
    /// Memory level: "green", "yellow", "orange", or "red"
    pub level: String,
}

/// Tracks memory usage for the Rust backend process only.
///
/// **Production note:** In a bundled Tauri app, the webview (rendering engine) runs
/// as a separate OS process. `sysinfo::Process::memory()` only reports the Rust
/// process's resident set size, so the webview's memory is *not* included in these
/// figures. Aggregating child-process memory is platform-specific and fragile, so
/// we intentionally scope this to the backend process and document the limitation
/// in the UI (MemoryDetailsModal).
pub struct MemoryService {
    system: System,
}

impl MemoryService {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
        }
    }

    /// Get current memory usage information
    pub fn get_memory_info(&mut self) -> Result<MemoryInfo> {
        // Get current process PID
        let current_pid = sysinfo::get_current_pid()
            .map_err(|e| anyhow::anyhow!("Failed to get current PID: {}", e))?;

        // Refresh this specific process to get accurate memory info
        self.system.refresh_processes(ProcessesToUpdate::Some(&[current_pid]), true);

        let process = self.system.process(current_pid)
            .ok_or_else(|| anyhow::anyhow!("Failed to get process info"))?;

        // Get memory usage in bytes and convert to MB
        let used_bytes = process.memory();
        let used_mb = used_bytes as f64 / MB as f64;

        let percentage_used = (used_mb / MEMORY_LIMIT_MB as f64) * 100.0;
        let is_approaching_limit = used_mb >= WARNING_THRESHOLD_MB as f64;
        let is_over_limit = used_mb >= MEMORY_LIMIT_MB as f64;

        // Determine memory level
        let level = if used_mb < MEMORY_GREEN_MAX as f64 {
            "green".to_string()
        } else if used_mb < MEMORY_YELLOW_MAX as f64 {
            "yellow".to_string()
        } else if used_mb < MEMORY_ORANGE_MAX as f64 {
            "orange".to_string()
        } else {
            "red".to_string()
        };

        debug!("Memory Usage: {:.2} MB / {} MB ({:.1}%) - {}",
                 used_mb, MEMORY_LIMIT_MB, percentage_used, level);

        Ok(MemoryInfo {
            used_mb,
            limit_mb: MEMORY_LIMIT_MB,
            warning_threshold_mb: WARNING_THRESHOLD_MB,
            is_approaching_limit,
            is_over_limit,
            percentage_used,
            level,
        })
    }
}
