import type { EmailSummary } from '../../types/email';
import type { Tag } from '../../types/feeds';
import { formatEmailDate } from '../../utils/formatEmailDate';

interface FeedEmailRowProps {
  email: EmailSummary;
  tags?: Tag[];
  rowColor: string;
  isSelected?: boolean;
  onClick: () => void;
}

export function FeedEmailRow({ email, tags = [], rowColor, isSelected = false, onClick }: FeedEmailRowProps) {
  return (
    <div
      onClick={onClick}
      style={{ backgroundColor: rowColor }}
      className={`px-8 py-4 cursor-pointer border-b border-gray-100 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors${isSelected ? ' ring-2 ring-blue-500 ring-inset' : ''}`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span
              className={`text-[15px] truncate ${
                !email.is_read ? 'text-gray-900 dark:text-gray-100 font-semibold' : 'text-gray-900 dark:text-gray-100 font-normal'
              }`}
            >
              {email.from_name || email.from_address}
            </span>
          </div>
          <p
            className={`text-[14px] truncate mt-1 ${
              !email.is_read ? 'text-gray-900 dark:text-gray-100 font-semibold' : 'text-gray-700 dark:text-gray-300 font-normal'
            }`}
          >
            {email.subject || '(No Subject)'}
          </p>
          {email.snippet && (
            <p className="text-[13px] text-gray-500 dark:text-gray-400 truncate mt-1">
              {email.snippet}
            </p>
          )}
          {tags.length > 0 && (
            <div className="flex gap-1 mt-1 flex-wrap">
              {tags.map((tag) => (
                <span
                  key={tag.id}
                  className="px-1.5 py-0.5 text-xs rounded inline-flex items-center gap-0.5"
                  style={{
                    backgroundColor: tag.color ? `${tag.color}20` : '#6b728020',
                    color: tag.color || '#9ca3af',
                  }}
                >
                  {tag.icon && <span className="text-xs">{tag.icon}</span>}
                  {tag.name}
                </span>
              ))}
            </div>
          )}
        </div>
        <div className="text-[13px] text-gray-500 dark:text-gray-400 flex-shrink-0 ml-4">
          {formatEmailDate(email.date)}
        </div>
      </div>
    </div>
  );
}
