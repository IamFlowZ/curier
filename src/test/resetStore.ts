import type { StoreApi, UseBoundStore } from 'zustand';

/**
 * Snapshot a Zustand store's initial state and return a function that
 * restores it. Call the returned function in `beforeEach` so each test
 * starts from a pristine store.
 */
export function createStoreReset<TState extends object>(
  store: UseBoundStore<StoreApi<TState>>,
): () => void {
  const initialState: TState = store.getState();
  return (): void => {
    store.setState(initialState, true);
  };
}
