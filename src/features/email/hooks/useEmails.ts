import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';

interface EmailSummary {
  id: number | null;
  uid: number;
  subject: string;
  from_address: string;
  from_name: string | null;
  date: number;
  snippet: string | null;
  has_attachments: boolean;
  is_read: boolean;
}

export function useEmails(email: string, count: number = 20) {
  return useQuery({
    queryKey: ['emails', email, count],
    queryFn: async () => {
      const emails = await invoke<EmailSummary[]>('fetch_emails', {
        email,
        count,
      });
      return emails;
    },
    staleTime: 1000 * 60 * 5, // 5 minutes
    refetchInterval: 1000 * 60 * 2, // Auto-refresh every 2 minutes
  });
}

export function useCachedEmails(limit: number = 20) {
  return useQuery({
    queryKey: ['cached-emails', limit],
    queryFn: async () => {
      const emails = await invoke<EmailSummary[]>('get_cached_emails', {
        limit,
      });
      return emails;
    },
  });
}
