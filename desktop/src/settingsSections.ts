export const SETTINGS_SECTIONS = ["Wikipedia & Offline Data", "AI Providers", "Appearance", "Privacy", "About"] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];
