import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAuthStore } from './authStore';
import { createStoreReset } from '../test/resetStore';

const EMAIL = 'alice@example.com';

const resetAuthStore = createStoreReset(useAuthStore);

describe('authStore', () => {
  beforeEach(() => {
    resetAuthStore();
    vi.spyOn(console, 'log').mockImplementation(() => undefined);
  });

  it('starts unauthenticated with no email', () => {
    expect(useAuthStore.getState()).toMatchObject({ isAuthenticated: false, email: null });
  });

  it('setAuthenticated stores the flag and email', () => {
    useAuthStore.getState().setAuthenticated(true, EMAIL);
    expect(useAuthStore.getState()).toMatchObject({ isAuthenticated: true, email: EMAIL });
  });

  it('setAuthenticated normalises a missing or empty email to null', () => {
    useAuthStore.getState().setAuthenticated(true);
    expect(useAuthStore.getState().email).toBeNull();

    useAuthStore.getState().setAuthenticated(true, '');
    expect(useAuthStore.getState().email).toBeNull();
  });

  it('logout clears authentication and email', () => {
    useAuthStore.getState().setAuthenticated(true, EMAIL);
    useAuthStore.getState().logout();
    expect(useAuthStore.getState()).toMatchObject({ isAuthenticated: false, email: null });
  });
});
