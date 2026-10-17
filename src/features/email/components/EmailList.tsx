import { useVirtualizer } from '@tanstack/react-virtual';
import { useRef } from 'react';

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

interface EmailListProps {
  emails: EmailSummary[];
  onEmailSelect: (email: EmailSummary) => void;
  selectedEmailId: number | null;
}

export default function EmailList({ emails, onEmailSelect, selectedEmailId }: EmailListProps) {
  const parentRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: emails.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 80,
    overscan: 5,
  });

  const formatDate = (timestamp: number) => {
    const date = new Date(timestamp * 1000);
    const now = new Date();
    const isToday = date.toDateString() === now.toDateString();

    if (isToday) {
      return date.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
    }

    return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
  };

  if (emails.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-gray-500">
        <div className="text-center">
          <p className="text-lg font-medium">No emails</p>
          <p className="text-sm">Your inbox is empty</p>
        </div>
      </div>
    );
  }

  return (
    <div ref={parentRef} className="h-full overflow-auto">
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          width: '100%',
          position: 'relative',
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const email = emails[virtualRow.index];
          const isSelected = email.id === selectedEmailId;

          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              ref={virtualizer.measureElement}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              <div
                onClick={() => onEmailSelect(email)}
                className={`px-4 py-3 cursor-pointer border-b border-gray-200 hover:bg-gray-50 transition-colors ${
                  isSelected ? 'bg-blue-50 border-blue-200' : ''
                } ${!email.is_read ? 'font-semibold' : ''}`}
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <p className="text-sm truncate">
                        {email.from_name || email.from_address}
                      </p>
                      {!email.is_read && (
                        <span className="w-2 h-2 bg-blue-600 rounded-full flex-shrink-0"></span>
                      )}
                    </div>
                    <p className="text-sm text-gray-900 truncate mt-1">
                      {email.subject || '(No Subject)'}
                    </p>
                    {email.snippet && (
                      <p className="text-xs text-gray-500 truncate mt-1">
                        {email.snippet}
                      </p>
                    )}
                  </div>
                  <div className="text-xs text-gray-500 flex-shrink-0">
                    {formatDate(email.date)}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
