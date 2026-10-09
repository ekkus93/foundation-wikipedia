export const READER_ACTIONS = ["Chat", "Bookmark", "Offline", "Settings"] as const;
export type ReaderAction = (typeof READER_ACTIONS)[number];
