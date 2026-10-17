import React from 'react';

export interface PaginationProps {
  currentPage: number;
  totalPages: number;
  totalItems: number;
  itemsPerPage: number;
  onPageChange: (page: number) => void;
  onPreviousPage: () => void;
  onNextPage: () => void;
  disabled?: boolean;
}

export const Pagination: React.FC<PaginationProps> = ({
  currentPage,
  totalPages,
  totalItems,
  itemsPerPage,
  onPageChange,
  onPreviousPage,
  onNextPage,
  disabled = false,
}) => {
  if (totalPages === 0) {
    return null;
  }

  const startItem = (currentPage - 1) * itemsPerPage + 1;
  const endItem = Math.min(currentPage * itemsPerPage, totalItems);

  // Generate page numbers to display (max 7 buttons with ellipsis)
  const getPageNumbers = (): (number | string)[] => {
    if (totalPages <= 7) {
      return Array.from({ length: totalPages }, (_, i) => i + 1);
    }

    const pages: (number | string)[] = [];

    if (currentPage <= 4) {
      for (let i = 1; i <= 5; i++) {
        pages.push(i);
      }
      pages.push('...');
      pages.push(totalPages);
    } else if (currentPage >= totalPages - 3) {
      pages.push(1);
      pages.push('...');
      for (let i = totalPages - 4; i <= totalPages; i++) {
        pages.push(i);
      }
    } else {
      pages.push(1);
      pages.push('...');
      pages.push(currentPage - 1);
      pages.push(currentPage);
      pages.push(currentPage + 1);
      pages.push('...');
      pages.push(totalPages);
    }

    return pages;
  };

  const pageNumbers = getPageNumbers();

  const isPrevDisabled = disabled || currentPage === 1;
  const isNextDisabled = disabled || currentPage === totalPages;

  return (
    <div className="flex flex-col gap-3 p-4 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-900">
      <div className="text-sm text-gray-500 dark:text-gray-400 text-center">
        Showing {startItem}-{endItem} of {totalItems}
      </div>
      <div className="flex items-center justify-center gap-2">
        <button
          onClick={onPreviousPage}
          disabled={isPrevDisabled}
          className={`px-3 py-1.5 text-sm border rounded transition-colors ${
            isPrevDisabled
              ? 'cursor-not-allowed opacity-50 bg-gray-100 dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-400 dark:text-gray-500'
              : 'cursor-pointer bg-white dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
          }`}
        >
          Previous
        </button>

        {pageNumbers.map((page, index) => {
          if (page === '...') {
            return (
              <span key={`ellipsis-${index}`} className="px-2 py-1.5 text-sm text-gray-400 dark:text-gray-500">
                ...
              </span>
            );
          }

          const pageNum = page as number;
          const isActive = pageNum === currentPage;

          return (
            <button
              key={pageNum}
              onClick={() => onPageChange(pageNum)}
              disabled={disabled}
              className={`px-3 py-1.5 text-sm border rounded transition-colors ${
                isActive
                  ? 'bg-blue-600 text-white border-blue-600 cursor-default'
                  : disabled
                    ? 'cursor-not-allowed opacity-50 bg-gray-100 dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-400 dark:text-gray-500'
                    : 'cursor-pointer bg-white dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
              }`}
            >
              {pageNum}
            </button>
          );
        })}

        <button
          onClick={onNextPage}
          disabled={isNextDisabled}
          className={`px-3 py-1.5 text-sm border rounded transition-colors ${
            isNextDisabled
              ? 'cursor-not-allowed opacity-50 bg-gray-100 dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-400 dark:text-gray-500'
              : 'cursor-pointer bg-white dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'
          }`}
        >
          Next
        </button>

        <span className="text-[13px] text-gray-400 dark:text-gray-500 ml-4">
          Page {currentPage} of {totalPages}
        </span>
      </div>
    </div>
  );
};
