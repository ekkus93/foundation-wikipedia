import { READER_ACTIONS } from "../src/readerActions.ts";

if (READER_ACTIONS.join(",") !== "Chat,Bookmark,Offline,Settings") {
  throw new Error("Reader action order changed");
}
if (new Set(READER_ACTIONS).size !== READER_ACTIONS.length) {
  throw new Error("Duplicate reader action");
}

import { SETTINGS_SECTIONS } from "../src/settingsSections.ts";
if (SETTINGS_SECTIONS.join(",") !== "General,Appearance & Reading,AI Assistant,Wikipedia & Offline Data,Storage,History & Saved Content,Privacy & Network,Advanced,About") {
  throw new Error("Settings navigation sections changed");
}
if (new Set(SETTINGS_SECTIONS).size !== SETTINGS_SECTIONS.length) {
  throw new Error("Duplicate settings section");
}

import { THEMES, isTheme } from "../src/theme.ts";
if (THEMES.join(",") !== "system,light,dark" || !THEMES.every(isTheme)) {
  throw new Error("Appearance theme options changed");
}
if (isTheme("cloud") || isTheme(null)) {
  throw new Error("Invalid theme preference accepted");
}
