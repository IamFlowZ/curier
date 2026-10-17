import { useState, useEffect } from 'react';
import type { Tag } from '../../types/feeds';
import { TAG_COLORS } from '../../types/feeds';
import { useFeedsStore } from '../../store/feedsStore';

/** Common emojis for tag icons */
const EMOJI_OPTIONS = [
  '📧', '⭐', '🚩', '❤️', '🔥', '💼', '📌', '🔔',
  '💰', '🛒', '📰', '🎉', '🐛', '✅', '⚠️', '🔒',
];

export function TagManager() {
  const { tags, loadTags, createTag, updateTag, deleteTag, isLoading } = useFeedsStore();
  const [newTagName, setNewTagName] = useState('');
  const [newTagColor, setNewTagColor] = useState<string>(TAG_COLORS[0]);
  const [newTagIcon, setNewTagIcon] = useState<string>('');
  const [editingTag, setEditingTag] = useState<Tag | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showEmojiPicker, setShowEmojiPicker] = useState(false);
  const [showEditEmojiPicker, setShowEditEmojiPicker] = useState(false);

  useEffect(() => {
    loadTags();
  }, [loadTags]);

  const handleCreateTag = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!newTagName.trim()) {
      setError('Tag name is required');
      return;
    }

    try {
      await createTag(newTagName.trim(), newTagColor, newTagIcon || undefined);
      setNewTagName('');
      setNewTagColor(TAG_COLORS[0]);
      setNewTagIcon('');
    } catch (err) {
      setError(String(err));
    }
  };

  const handleUpdateTag = async () => {
    if (!editingTag) return;
    setError(null);

    try {
      await updateTag(editingTag.id, editingTag.name, editingTag.color, editingTag.icon);
      setEditingTag(null);
      setShowEditEmojiPicker(false);
    } catch (err) {
      setError(String(err));
    }
  };

  const handleDeleteTag = async (tagId: number, tagName: string) => {
    const confirmed = window.confirm(
      `Delete tag "${tagName}"? This will remove it from all emails.`
    );
    if (!confirmed) return;

    try {
      await deleteTag(tagId);
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="space-y-4">
      {error && (
        <div className="p-3 bg-red-100 dark:bg-red-900/50 border border-red-300 dark:border-red-700 rounded text-sm text-red-700 dark:text-red-200">
          {error}
        </div>
      )}

      <form onSubmit={handleCreateTag} className="flex gap-2 items-end">
        <input
          type="text"
          value={newTagName}
          onChange={(e) => setNewTagName(e.target.value)}
          placeholder="New tag name..."
          className="flex-1 px-3 py-2 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded focus:border-blue-500 focus:outline-none text-gray-900 dark:text-white text-sm"
        />
        {/* Emoji picker toggle */}
        <div className="relative">
          <button
            type="button"
            onClick={() => setShowEmojiPicker(!showEmojiPicker)}
            className="w-8 h-8 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-lg flex items-center justify-center cursor-pointer hover:bg-gray-100 dark:hover:bg-gray-600"
            title="Pick an icon"
          >
            {newTagIcon || '😀'}
          </button>
          {showEmojiPicker && (
            <div className="absolute bottom-10 left-0 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-lg p-2 z-10 shadow-xl">
              <div className="grid grid-cols-8 gap-1 mb-2">
                {EMOJI_OPTIONS.map((emoji) => (
                  <button
                    key={emoji}
                    type="button"
                    onClick={() => {
                      setNewTagIcon(emoji);
                      setShowEmojiPicker(false);
                    }}
                    className={`w-8 h-8 text-lg rounded cursor-pointer hover:bg-gray-100 dark:hover:bg-gray-600 ${
                      newTagIcon === emoji ? 'bg-gray-200 dark:bg-gray-600 ring-1 ring-blue-500' : 'bg-transparent'
                    }`}
                  >
                    {emoji}
                  </button>
                ))}
              </div>
              <div className="flex gap-1">
                <input
                  type="text"
                  value={newTagIcon}
                  onChange={(e) => setNewTagIcon(e.target.value)}
                  placeholder="Custom emoji..."
                  className="flex-1 px-2 py-1 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded text-sm text-gray-900 dark:text-white focus:outline-none focus:border-blue-500"
                  maxLength={4}
                />
                <button
                  type="button"
                  onClick={() => {
                    setNewTagIcon('');
                    setShowEmojiPicker(false);
                  }}
                  className="px-2 py-1 text-xs text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
                >
                  Clear
                </button>
              </div>
            </div>
          )}
        </div>
        <div className="flex gap-1">
          {TAG_COLORS.map((color) => (
            <button
              key={color}
              type="button"
              onClick={() => setNewTagColor(color)}
              className={`w-6 h-8 rounded ${
                newTagColor === color ? 'ring-2 ring-gray-900 dark:ring-white ring-offset-1 ring-offset-white dark:ring-offset-gray-800' : ''
              }`}
              style={{ backgroundColor: color }}
              title={color}
            />
          ))}
        </div>
        <button
          type="submit"
          disabled={isLoading || !newTagName.trim()}
          className="px-4 py-2 bg-blue-600 text-white text-sm rounded hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          Add
        </button>
      </form>

      <div className="space-y-2">
        {tags.length === 0 && !isLoading && (
          <p className="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
            No tags created yet. Create your first tag above.
          </p>
        )}

        {tags.map((tag) => (
          <div
            key={tag.id}
            className="flex items-center gap-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded border border-gray-200 dark:border-gray-600"
          >
            {editingTag?.id === tag.id ? (
              <>
                <input
                  type="text"
                  value={editingTag.name}
                  onChange={(e) =>
                    setEditingTag({ ...editingTag, name: e.target.value })
                  }
                  className="flex-1 px-2 py-1 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded focus:border-blue-500 focus:outline-none text-gray-900 dark:text-white text-sm"
                  autoFocus
                />
                {/* Edit emoji picker */}
                <div className="relative">
                  <button
                    type="button"
                    onClick={() => setShowEditEmojiPicker(!showEditEmojiPicker)}
                    className="w-7 h-7 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-700 text-base flex items-center justify-center cursor-pointer hover:bg-gray-100 dark:hover:bg-gray-600"
                  >
                    {editingTag.icon || '😀'}
                  </button>
                  {showEditEmojiPicker && (
                    <div className="absolute bottom-9 left-0 bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-lg p-2 z-10 shadow-xl">
                      <div className="grid grid-cols-8 gap-1 mb-2">
                        {EMOJI_OPTIONS.map((emoji) => (
                          <button
                            key={emoji}
                            type="button"
                            onClick={() => {
                              setEditingTag({ ...editingTag, icon: emoji });
                              setShowEditEmojiPicker(false);
                            }}
                            className={`w-7 h-7 text-base rounded cursor-pointer hover:bg-gray-100 dark:hover:bg-gray-600 ${
                              editingTag.icon === emoji ? 'bg-gray-200 dark:bg-gray-600 ring-1 ring-blue-500' : 'bg-transparent'
                            }`}
                          >
                            {emoji}
                          </button>
                        ))}
                      </div>
                      <div className="flex gap-1">
                        <input
                          type="text"
                          value={editingTag.icon ?? ''}
                          onChange={(e) => setEditingTag({ ...editingTag, icon: e.target.value })}
                          placeholder="Custom..."
                          className="flex-1 px-2 py-1 bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded text-sm text-gray-900 dark:text-white focus:outline-none focus:border-blue-500"
                          maxLength={4}
                        />
                        <button
                          type="button"
                          onClick={() => {
                            setEditingTag({ ...editingTag, icon: undefined });
                            setShowEditEmojiPicker(false);
                          }}
                          className="px-2 py-1 text-xs text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white"
                        >
                          Clear
                        </button>
                      </div>
                    </div>
                  )}
                </div>
                <div className="flex gap-1">
                  {TAG_COLORS.map((color) => (
                    <button
                      key={color}
                      type="button"
                      onClick={() => setEditingTag({ ...editingTag, color })}
                      className={`w-5 h-5 rounded ${
                        editingTag.color === color
                          ? 'ring-2 ring-gray-900 dark:ring-white ring-offset-1 ring-offset-white dark:ring-offset-gray-800'
                          : ''
                      }`}
                      style={{ backgroundColor: color }}
                    />
                  ))}
                </div>
                <button
                  onClick={handleUpdateTag}
                  className="px-2 py-1 text-sm text-green-600 dark:text-green-400 hover:text-green-700 dark:hover:text-green-300"
                >
                  Save
                </button>
                <button
                  onClick={() => { setEditingTag(null); setShowEditEmojiPicker(false); }}
                  className="px-2 py-1 text-sm text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300"
                >
                  Cancel
                </button>
              </>
            ) : (
              <>
                {tag.icon && (
                  <span className="text-base flex-shrink-0">{tag.icon}</span>
                )}
                {!tag.icon && (
                  <span
                    className="w-3 h-3 rounded-full flex-shrink-0"
                    style={{ backgroundColor: tag.color || '#6b7280' }}
                  />
                )}
                <span className="flex-1 text-sm text-gray-900 dark:text-white">{tag.name}</span>
                <button
                  onClick={() => setEditingTag(tag)}
                  className="p-1 text-gray-400 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white transition-colors"
                  title="Edit tag"
                >
                  <svg
                    className="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z"
                    />
                  </svg>
                </button>
                <button
                  onClick={() => handleDeleteTag(tag.id, tag.name)}
                  className="p-1 text-gray-400 dark:text-gray-400 hover:text-red-500 dark:hover:text-red-400 transition-colors"
                  title="Delete tag"
                >
                  <svg
                    className="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                    />
                  </svg>
                </button>
              </>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
