import { create } from 'zustand';

interface AuthState {
  isAuthenticated: boolean;
  email: string | null;
  setAuthenticated: (isAuthenticated: boolean, email?: string) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  isAuthenticated: false,
  email: null,
  setAuthenticated: (isAuthenticated, email) => {
    console.log('AuthStore: setAuthenticated called with:', { isAuthenticated, email });
    set({ isAuthenticated, email: email || null });
  },
  logout: () => {
    console.log('AuthStore: logout called');
    set({ isAuthenticated: false, email: null });
  },
}));
