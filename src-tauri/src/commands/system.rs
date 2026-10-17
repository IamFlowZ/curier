use crate::services::memory::{MemoryInfo, MemoryService};
use std::sync::Mutex;
use tauri::State;

// Global memory service (thread-safe)
pub struct MemoryState {
    service: Mutex<MemoryService>,
}

impl MemoryState {
    pub fn new() -> Self {
        Self {
            service: Mutex::new(MemoryService::new()),
        }
    }
}

/// Get current memory usage information
#[tauri::command]
pub async fn get_memory_info(state: State<'_, MemoryState>) -> Result<MemoryInfo, String> {
    let mut service = state
        .service
        .lock()
        .map_err(|e| format!("Failed to lock memory service: {}", e))?;

    service
        .get_memory_info()
        .map_err(|e| format!("Failed to get memory info: {}", e))
}
