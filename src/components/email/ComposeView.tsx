import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTabStore } from '../../store/tabStore';
import { getAccountColor, extractAccountId } from '../../utils/accountColors';
import { CustomSelect } from '../ui/CustomSelect';

interface Account {
  id: string;
  account_type: string;
  email: string;
  display_name: string;
}

interface ComposeViewProps {
  tabId: string;
  accountEmail: string;
  replyTo?: string;
  replySubject?: string;
  inReplyTo?: string;
  replyQuotedBody?: string;
  replyFromName?: string;
  replyDate?: number;
}

function accountSelectOptions(accounts: Account[]): import('../ui/CustomSelect').SelectOption[] {
  return accounts.map((account) => {
    const accId = extractAccountId({ account_email: account.email });
    const accColor = getAccountColor(accId);
    return {
      value: account.email,
      label: `${account.display_name} (${account.email})`,
      color: accColor ?? undefined,
    };
  });
}

function formatQuotedReply(quotedBody: string, fromName: string, date?: number): string {
  const dateStr = date
    ? new Date(date * 1000).toLocaleString()
    : '';
  const header = dateStr
    ? `On ${dateStr}, ${fromName} wrote:`
    : `${fromName} wrote:`;
  const quoted = quotedBody
    .split('\n')
    .map((line) => `> ${line}`)
    .join('\n');
  return `\n\n${header}\n${quoted}`;
}

export function ComposeView({ tabId, accountEmail, replyTo, replySubject, inReplyTo, replyQuotedBody, replyFromName, replyDate }: ComposeViewProps) {
  const initialBody = replyQuotedBody
    ? formatQuotedReply(replyQuotedBody, replyFromName ?? replyTo ?? '', replyDate)
    : '';

  const [to, setTo] = useState(replyTo ?? '');
  const [subject, setSubject] = useState(replySubject ?? '');
  const [body, setBody] = useState(initialBody);
  const [selectedAccount, setSelectedAccount] = useState(accountEmail);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState('');
  const { closeAndFocusPrevious } = useTabStore();

  useEffect(() => {
    invoke<Account[]>('get_accounts')
      .then(setAccounts)
      .catch((err) => console.error('Failed to load accounts:', err));
  }, []);

  const accountId = extractAccountId({ account_email: selectedAccount });
  const accountColor = getAccountColor(accountId);

  const handleSend = async () => {
    if (!to.trim()) {
      setError('Recipient is required');
      return;
    }
    if (!subject.trim()) {
      setError('Subject is required');
      return;
    }

    setSending(true);
    setError('');

    try {
      await invoke('send_email', {
        accountEmail: selectedAccount,
        to: to.trim(),
        subject: subject.trim(),
        body,
        inReplyTo: inReplyTo ?? null,
      });
      closeAndFocusPrevious(tabId);
    } catch (err) {
      setError(String(err));
    } finally {
      setSending(false);
    }
  };

  const handleDiscard = () => {
    closeAndFocusPrevious(tabId);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="flex flex-col h-full" onKeyDown={handleKeyDown}>
      {/* Header */}
      <div className="px-8 py-5 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900">
        <div className="flex items-center justify-between mb-4">
          <h3 className="m-0 text-xl text-gray-900 dark:text-gray-100 font-semibold">
            {replyTo ? 'Reply' : 'New Email'}
          </h3>
          <div className="flex items-center gap-2">
            <button
              onClick={handleDiscard}
              disabled={sending}
              className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-md cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors disabled:opacity-50"
            >
              Discard
            </button>
            <button
              onClick={handleSend}
              disabled={sending}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 border-none rounded-md cursor-pointer hover:bg-blue-700 transition-colors disabled:opacity-50 flex items-center gap-2"
            >
              {sending ? (
                <>
                  <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                  Sending...
                </>
              ) : (
                <>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <line x1="22" y1="2" x2="11" y2="13" />
                    <polygon points="22 2 15 22 11 13 2 9 22 2" />
                  </svg>
                  Send
                </>
              )}
            </button>
          </div>
        </div>

        {/* Error */}
        {error && (
          <div className="mb-4 p-3 bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 text-sm rounded-md">
            {error}
          </div>
        )}

        {/* Form Fields */}
        <div className="space-y-3">
          {/* From */}
          <div className="flex items-center gap-3">
            <label className="text-sm font-medium text-gray-500 dark:text-gray-400 w-16 shrink-0">From</label>
            <CustomSelect
              value={selectedAccount}
              onChange={setSelectedAccount}
              disabled={sending}
              options={accountSelectOptions(accounts)}
              placeholder={selectedAccount}
              className="flex-1"
            />
          </div>

          {/* To */}
          <div className="flex items-center gap-3">
            <label className="text-sm font-medium text-gray-500 dark:text-gray-400 w-16 shrink-0">To</label>
            <input
              type="email"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              disabled={sending}
              placeholder="recipient@example.com"
              className="flex-1 px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50"
              autoFocus={!replyTo}
            />
          </div>

          {/* Subject */}
          <div className="flex items-center gap-3">
            <label className="text-sm font-medium text-gray-500 dark:text-gray-400 w-16 shrink-0">Subject</label>
            <input
              type="text"
              value={subject}
              onChange={(e) => setSubject(e.target.value)}
              disabled={sending}
              placeholder="Subject"
              className="flex-1 px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50"
              autoFocus={!!replyTo}
            />
          </div>
        </div>
      </div>

      {/* Account Color Bar */}
      {accountColor && (
        <div className="h-1" style={{ backgroundColor: accountColor }} />
      )}

      {/* Body */}
      <div className="flex-1 p-8 overflow-hidden">
        <textarea
          value={body}
          onChange={(e) => setBody(e.target.value)}
          disabled={sending}
          placeholder="Write your message..."
          className="w-full h-full resize-none px-4 py-3 text-sm border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 font-sans leading-relaxed"
        />
      </div>

      {/* Footer hint */}
      <div className="px-8 py-2 border-t border-gray-200 dark:border-gray-700 text-xs text-gray-400 dark:text-gray-500">
        Press {navigator.platform.includes('Mac') ? 'Cmd' : 'Ctrl'}+Enter to send
      </div>
    </div>
  );
}
