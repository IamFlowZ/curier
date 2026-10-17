import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTabStore } from '../../store/tabStore';
import { getAccountColor, extractAccountId } from '../../utils/accountColors';
import { EmailHtmlRenderer } from './EmailHtmlRenderer';
import { useTheme } from '../../hooks/useTheme';
import { formatByteSize } from '../../utils/formatByteSize';
import { htmlToPlainText } from '../../utils/htmlToPlainText';
import type { AttachmentInfo } from '../../types/email';

interface EmailViewerProps {
  emailId: string;
  subject: string;
  accountEmail: string;
  fromAddress: string;
  fromName?: string;
  date: number;
}

interface EmailBody {
  html_body?: string;
  text_body?: string;
}

interface AppSettings {
  approved_image_domains: string[];
}

export function EmailViewer({ emailId, subject, accountEmail, fromAddress, fromName, date }: EmailViewerProps) {
  const [emailBody, setEmailBody] = useState<EmailBody | null>(null);
  const [approvedDomains, setApprovedDomains] = useState<string[]>([]);
  const [attachments, setAttachments] = useState<AttachmentInfo[]>([]);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const { closeAndFocusPrevious, addComposeTab } = useTabStore();

  const accountId = extractAccountId({ account_email: accountEmail });
  const accountColor = getAccountColor(accountId);
  const { preference } = useTheme();
  const isDarkMode = preference === 'dark' || (preference === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);

  useEffect(() => {
    const fetchEmailBody = async () => {
      setLoading(true);
      setError('');
      try {
        const [body, settings, attachmentList] = await Promise.all([
          invoke<EmailBody>('get_email_body', {
            messageId: emailId,
            accountEmail: accountEmail,
          }),
          invoke<AppSettings>('get_settings').catch(() => ({ approved_image_domains: [] })),
          invoke<AttachmentInfo[]>('list_attachments', {
            messageId: emailId,
            accountEmail: accountEmail,
          }).catch(() => []),
        ]);
        setEmailBody(body);
        setApprovedDomains(settings.approved_image_domains);
        setAttachments(attachmentList);
      } catch (err) {
        setError(String(err));
        console.error('Failed to fetch email body:', err);
      } finally {
        setLoading(false);
      }
    };

    fetchEmailBody();
  }, [emailId]);

  const handleApproveImageDomain = async (domain: string) => {
    try {
      const currentSettings = await invoke<AppSettings & Record<string, unknown>>('get_settings');
      const existingDomains: string[] = currentSettings.approved_image_domains ?? [];
      if (existingDomains.includes(domain)) {
        // Already approved — just update local state to re-render
        setApprovedDomains((prev) => (prev.includes(domain) ? prev : [...prev, domain]));
        return;
      }
      const updatedSettings = {
        ...currentSettings,
        approved_image_domains: [...existingDomains, domain],
      };
      await invoke('update_settings', { settings: updatedSettings });
      setApprovedDomains((prev) => [...prev, domain]);
    } catch (err) {
      console.error('Failed to approve image domain:', err);
    }
  };

  const handleDownload = async (attachment: AttachmentInfo) => {
    setDownloadingId(attachment.id);
    try {
      await invoke<boolean>('download_attachment', {
        messageId: emailId,
        attachmentId: attachment.id,
        filename: attachment.filename,
        accountEmail,
      });
    } catch (err) {
      console.error('Failed to download attachment:', err);
    } finally {
      setDownloadingId(null);
    }
  };

  const handleBack = () => {
    closeAndFocusPrevious(`email-${emailId}`);
  };

  const handleReply = () => {
    // Extract plain text for the quoted reply chain
    let quotedText = '';
    if (emailBody?.text_body) {
      quotedText = emailBody.text_body;
    } else if (emailBody?.html_body) {
      // Strip HTML tags for a plain text quote. DOMParser is inert: unlike
      // innerHTML it does not fetch remote images (tracking pixels).
      quotedText = htmlToPlainText(emailBody.html_body);
    }

    addComposeTab(
      accountEmail,
      fromAddress,
      subject,
      emailId,
      quotedText,
      fromName ?? fromAddress,
      date,
    );
  };

  // Listen for keyboard shortcut reply event
  useEffect(() => {
    const handler = () => handleReply();
    window.addEventListener('curier:reply', handler);
    return () => window.removeEventListener('curier:reply', handler);
  }, [emailBody, accountEmail, fromAddress, subject, emailId, fromName, date]);

  return (
    <div className="flex flex-col h-full">
      {/* Email Header */}
      <div className="px-8 py-5 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900">
        <div className="flex items-center gap-2 mb-4">
          <button
            onClick={handleBack}
            className="inline-flex items-center gap-1.5 py-1.5 px-3 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-300 border border-gray-300 dark:border-gray-600 rounded-md text-sm cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="15 18 9 12 15 6" />
            </svg>
            Back
          </button>
          <button
            onClick={handleReply}
            className="inline-flex items-center gap-1.5 py-1.5 px-3 bg-white dark:bg-gray-800 text-gray-700 dark:text-gray-300 border border-gray-300 dark:border-gray-600 rounded-md text-sm cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="9 17 4 12 9 7" />
              <path d="M20 18v-2a4 4 0 0 0-4-4H4" />
            </svg>
            Reply
          </button>
        </div>
        <h3 className="m-0 mb-2 text-xl text-gray-900 dark:text-gray-100 font-semibold">
          {subject || 'No Subject'}
        </h3>
        <div className="mt-3 text-sm text-gray-500 dark:text-gray-400">
          <div className="mb-1">
            <span className="font-medium text-gray-700 dark:text-gray-300">From: </span>
            {fromName ? `${fromName} <${fromAddress}>` : fromAddress}
          </div>
          <div>
            <span className="font-medium text-gray-700 dark:text-gray-300">Date: </span>
            {new Date(date * 1000).toLocaleString()}
          </div>
        </div>
      </div>

      {/* Account Color Bar */}
      {accountColor && (
        <div className="h-1" style={{ backgroundColor: accountColor }} />
      )}

      {/* Attachments Bar */}
      {attachments.length > 0 && (
        <div className="px-8 py-3 border-b border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900">
          <div className="flex items-center gap-2 mb-2 text-sm text-gray-600 dark:text-gray-400">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48" />
            </svg>
            <span>{attachments.length} attachment{attachments.length !== 1 ? 's' : ''}</span>
          </div>
          <div className="flex flex-wrap gap-2">
            {attachments.map((att) => (
              <button
                key={att.id}
                onClick={() => handleDownload(att)}
                disabled={downloadingId === att.id}
                className="inline-flex items-center gap-2 px-3 py-2 rounded-md border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors cursor-pointer disabled:opacity-50"
              >
                <span className="font-medium truncate max-w-[200px]">{att.filename}</span>
                <span className="text-xs text-gray-500 dark:text-gray-400">
                  {formatByteSize(att.size)}
                </span>
                {downloadingId === att.id ? (
                  <div className="w-4 h-4 border-2 border-gray-300 dark:border-gray-600 border-t-blue-600 rounded-full animate-spin" />
                ) : (
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                    <polyline points="7 10 12 15 17 10" />
                    <line x1="12" y1="15" x2="12" y2="3" />
                  </svg>
                )}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Email Body */}
      <div className="flex-1 overflow-y-auto p-8">
        {loading ? (
          <div className="flex items-center justify-center p-16 text-gray-500 dark:text-gray-400">
            <div className="w-6 h-6 border-2 border-gray-200 dark:border-gray-600 border-t-blue-600 rounded-full animate-spin" />
            <span className="ml-3">Loading email...</span>
          </div>
        ) : error ? (
          <div className="bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 p-4 rounded-lg text-sm">
            {error}
          </div>
        ) : emailBody ? (
          <div>
            {emailBody.html_body || emailBody.text_body ? (
              <EmailHtmlRenderer
                html={emailBody.html_body}
                plainText={emailBody.text_body}
                isDarkMode={isDarkMode}
                approvedDomains={approvedDomains}
                onApproveImageDomain={handleApproveImageDomain}
              />
            ) : (
              <div className="text-gray-500 dark:text-gray-400 italic">
                No content available
              </div>
            )}
          </div>
        ) : null}
      </div>
    </div>
  );
}
