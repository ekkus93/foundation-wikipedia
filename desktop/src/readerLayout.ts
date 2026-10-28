/** Whitelisted app-owned reading controls. Values never become raw CSS. */
export const TEXT_SIZE_OPTIONS = ["small", "standard", "large"] as const;
export const WIDTH_OPTIONS = ["narrow", "standard", "wide"] as const;
export type ReaderTextSize = (typeof TEXT_SIZE_OPTIONS)[number];
export type ReaderWidth = (typeof WIDTH_OPTIONS)[number];

export function isReaderTextSize(value: unknown): value is ReaderTextSize {
  return typeof value === "string" && TEXT_SIZE_OPTIONS.includes(value as ReaderTextSize);
}
export function isReaderWidth(value: unknown): value is ReaderWidth {
  return typeof value === "string" && WIDTH_OPTIONS.includes(value as ReaderWidth);
}

export function parseReaderLayout(value: unknown): [ReaderTextSize, ReaderWidth] {
  if (!Array.isArray(value) || value.length !== 2 ||
      !isReaderTextSize(value[0]) || !isReaderWidth(value[1])) {
    throw new Error("Invalid saved reading layout");
  }
  return [value[0], value[1]];
}

const FONT_PX: Record<ReaderTextSize, number> = { small: 15, standard: 17, large: 20 };
const WIDTH_CH: Record<ReaderWidth, number> = { narrow: 58, standard: 74, wide: 96 };

/** Preview HTML remains sandboxed; presentation choices select only fixed CSS
 * literals. Article HTML is sanitized by the revision-verified Rust adapter.
 * Changing srcDoc may reset in-frame scrolling; no same-origin privileges.
 */
export function isolatedReaderDocument(
  html: string, textSize: unknown, readingWidth: unknown, theme: unknown = "light",
): string {
  const fontSize = FONT_PX[isReaderTextSize(textSize) ? textSize : "standard"];
  const lineWidth = WIDTH_CH[isReaderWidth(readingWidth) ? readingWidth : "standard"];
  const dark = theme === "dark";
  const foreground = dark ? "#eaecf0" : "#202122";
  const background = dark ? "#202122" : "#ffffff";
  const systemDark = theme === "system"
    ? "@media (prefers-color-scheme: dark){html{background:#202122}body{color:#eaecf0}}"
    : "";
  return [
    "<!doctype html><html><head>",
    "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'none'; font-src 'none'; connect-src 'none'; media-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; style-src 'unsafe-inline'\">",
    "<meta name=\"referrer\" content=\"no-referrer\">",
    `<style>html{background:${background}}body{box-sizing:border-box;font:${fontSize}px/1.6 Georgia,serif;padding:1rem;margin:0 auto;max-width:${lineWidth}ch;color:${foreground}}table{max-width:100%;overflow-wrap:anywhere}img,video,audio{display:none}a{pointer-events:none}pre{white-space:pre-wrap}*{max-width:100%}${systemDark}</style>`,
    "</head><body>", html, "</body></html>",
  ].join("");
}
