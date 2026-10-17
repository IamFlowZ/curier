interface AddFeedButtonProps {
  onClick: () => void;
}

export function AddFeedButton({ onClick }: AddFeedButtonProps) {
  return (
    <button
      onClick={onClick}
      className="flex flex-col items-center justify-center w-16 h-full min-h-[200px] bg-gray-100/50 dark:bg-gray-800/50 hover:bg-gray-200 dark:hover:bg-gray-800 rounded-lg border-2 border-dashed border-gray-300 dark:border-gray-600 hover:border-gray-400 dark:hover:border-gray-500 transition-colors flex-shrink-0"
      title="Add new feed column"
    >
      <svg
        className="w-8 h-8 text-gray-400 dark:text-gray-500"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M12 4v16m8-8H4"
        />
      </svg>
      <span className="text-xs text-gray-400 dark:text-gray-500 mt-2">Add Feed</span>
    </button>
  );
}
