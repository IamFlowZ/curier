import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface MemoryInfo {
  used_mb: number;
  limit_mb: number;
  warning_threshold_mb: number;
  is_approaching_limit: boolean;
  is_over_limit: boolean;
  percentage_used: number;
  level: string;
}

const POLL_INTERVAL_MS = 5000;

export function useMemoryMonitor() {
  const [memoryInfo, setMemoryInfo] = useState<MemoryInfo | null>(null);

  useEffect(() => {
    const fetchMemoryInfo = async () => {
      try {
        const info = await invoke<MemoryInfo>('get_memory_info');
        setMemoryInfo(info);
      } catch (error) {
        console.error('Failed to fetch memory info:', error);
      }
    };

    fetchMemoryInfo();
    const interval = setInterval(fetchMemoryInfo, POLL_INTERVAL_MS);

    return () => clearInterval(interval);
  }, []);

  return memoryInfo;
}
