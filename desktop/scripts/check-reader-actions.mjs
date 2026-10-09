import { READER_ACTIONS } from "../src/readerActions.ts";

if (READER_ACTIONS.join(",") !== "Chat,Bookmark,Offline,Settings") {
  throw new Error("Reader action order changed");
}
if (new Set(READER_ACTIONS).size !== READER_ACTIONS.length) {
  throw new Error("Duplicate reader action");
}
