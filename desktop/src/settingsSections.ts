export const SETTINGS_SECTIONS = ["General","Appearance & Reading","AI Assistant","Wikipedia & Offline Data","Storage","History & Saved Content","Privacy & Network","Advanced","About"] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];
