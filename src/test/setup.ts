/**
 * Global test setup. Mocks the Tauri IPC bridge so store/util code that
 * calls `invoke` or subscribes to events can run under jsdom without the
 * Tauri runtime. Individual tests configure `invoke` behaviour via
 * `vi.mocked(invoke).mockResolvedValue(...)`.
 */
import { vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (): Promise<unknown> => undefined),
  convertFileSrc: vi.fn((path: string): string => path),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (): Promise<() => void> => () => undefined),
  once: vi.fn(async (): Promise<() => void> => () => undefined),
  emit: vi.fn(async (): Promise<void> => undefined),
}));

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn(async (): Promise<void> => undefined),
}));

beforeEach(() => {
  vi.clearAllMocks();
});
