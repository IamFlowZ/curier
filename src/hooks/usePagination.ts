import { useState, useCallback } from 'react';

export interface PaginationControls {
  currentPage: number;
  itemsPerPage: number;
  goToPage: (page: number) => void;
  goToNextPage: () => void;
  goToPreviousPage: () => void;
  canGoNext: (totalPages: number) => boolean;
  canGoPrevious: () => boolean;
  resetToFirstPage: () => void;
}

export interface UsePaginationOptions {
  initialPage?: number;
  itemsPerPage: number;
}

export function usePagination(options: UsePaginationOptions): PaginationControls {
  const { initialPage = 1, itemsPerPage } = options;
  const [currentPage, setCurrentPage] = useState(initialPage);

  const goToPage = useCallback((page: number) => {
    if (page >= 1) {
      setCurrentPage(page);
    }
  }, []);

  const goToNextPage = useCallback(() => {
    setCurrentPage(prev => prev + 1);
  }, []);

  const goToPreviousPage = useCallback(() => {
    setCurrentPage(prev => Math.max(1, prev - 1));
  }, []);

  const canGoNext = useCallback((totalPages: number) => {
    return currentPage < totalPages;
  }, [currentPage]);

  const canGoPrevious = useCallback(() => {
    return currentPage > 1;
  }, [currentPage]);

  const resetToFirstPage = useCallback(() => {
    setCurrentPage(1);
  }, []);

  return {
    currentPage,
    itemsPerPage,
    goToPage,
    goToNextPage,
    goToPreviousPage,
    canGoNext,
    canGoPrevious,
    resetToFirstPage,
  };
}
