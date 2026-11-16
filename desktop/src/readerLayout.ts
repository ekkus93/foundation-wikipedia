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

/** Options for an article document. Both default to off (text only). */
export interface ReaderDocumentOptions {
  /** Load images from Wikimedia's media and math hosts (online only). */
  images?: boolean;
  /** Include the app's link-forwarding script under this CSP nonce. */
  linkNonce?: string;
}

/** Image hosts the article frame may load from; matches the Rust sanitizer. */
const IMAGE_SOURCES = "https://upload.wikimedia.org https://wikimedia.org";
const NONCE = /^[A-Za-z0-9]{16,64}$/;

/** A fresh CSP nonce for one article document. */
export function readerDocumentNonce(): string {
  const bytes = new Uint8Array(18);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(36).padStart(2, "0")).join("").replace(/[^A-Za-z0-9]/g, "a");
}

/** Message a clicked link sends from the article frame to the reader. */
export const LINK_MESSAGE_KEY = "foundationReaderLink";

/** The only script in an article frame: forward link activations (click or
 * keyboard) to the parent, which decides what to open. In-page fragments
 * scroll normally. It reads nothing else and has no same-origin access. */
const LINK_SCRIPT = `document.addEventListener("click",function(e){var t=e.target;var a=t&&t.closest?t.closest("a[href]"):null;if(!a)return;var h=a.getAttribute("href")||"";if(h.charAt(0)==="#")return;e.preventDefault();parent.postMessage({${LINK_MESSAGE_KEY}:h},"*");},true);`;

/** Wikipedia-like presentation for the classes MediaWiki emits. */
function wikipediaStyle(dark: boolean): string {
  const rule = dark ? "#54595d" : "#a2a9b1";
  const box = dark ? "#1b1f24" : "#f8f9fa";
  const link = dark ? "#88a3e8" : "#3366cc";
  const muted = dark ? "#a2a9b1" : "#54595d";
  return [
    `a{color:${link};text-decoration:none}a:hover,a:focus{text-decoration:underline}`,
    `h1,h2{font-family:Georgia,'Linux Libertine',serif;font-weight:400;border-bottom:1px solid ${rule};margin:1em 0 .25em;line-height:1.3}`,
    "h2{font-size:1.5em}h3{font-size:1.2em;margin:.3em 0 0}h4{font-size:1.05em}",
    `.infobox{float:right;clear:right;width:22em;margin:0 0 1em 1em;border:1px solid ${rule};background:${box};font-size:88%;line-height:1.5;border-collapse:collapse;display:table}`,
    ".infobox th,.infobox td{padding:.2em .4em;vertical-align:top;text-align:left;border:0}",
    ".infobox .infobox-above,.infobox caption{font-size:125%;font-weight:bold;text-align:center}",
    `.thumb,figure[typeof~="mw:File/Thumb"],figure[typeof~="mw:File/Frame"]{float:right;clear:right;margin:.5em 0 1em 1em;border:1px solid ${rule};background:${box};padding:3px;font-size:88%;max-width:min(22em,45%)}`,
    ".tleft,.mw-halign-left{float:left;clear:left;margin:.5em 1em 1em 0}",
    `.thumbcaption,figcaption{padding:.3em .2em;line-height:1.4;color:inherit}`,
    `table.wikitable{border-collapse:collapse;background:${box};border:1px solid ${rule};margin:1em 0}`,
    `.wikitable th,.wikitable td{border:1px solid ${rule};padding:.2em .4em}`,
    `.wikitable th{background:${dark ? "#27292d" : "#eaecf0"}}`,
    "table{overflow-x:auto}",
    `.hatnote,.dablink{font-style:italic;padding-left:1.6em;margin-bottom:.5em;color:${muted}}`,
    ".reflist,.references,ol.references{font-size:90%}",
    ".reference{line-height:1;font-size:80%}",
    `.toc,#toc{display:inline-block;border:1px solid ${rule};background:${box};padding:.4em .8em;font-size:95%}`,
    ".toc ul,#toc ul{list-style:none;margin:.3em 0 0 1em;padding:0}",
    ".navbox,.navbox-styles,.metadata,.noprint,.mw-empty-elt,.mw-editsection,.sistersitebox,.portalbox,.ambox,.mbox-small{display:none}",
    ".mwe-math-fallback-image-inline{display:inline-block;vertical-align:middle}",
    ".mwe-math-fallback-image-display{display:block;margin:.5em auto}",
    "@media (max-width:640px){.infobox,.thumb,figure[typeof]{float:none;width:auto;max-width:100%;margin:1em 0}}",
  ].join("");
}

/** Preview HTML remains sandboxed (no same-origin access); presentation
 * choices select only fixed CSS literals. Article HTML is sanitized by the
 * revision-verified Rust adapter. Images load only when `options.images` is
 * set and only from Wikimedia hosts; the only script is the app's link
 * forwarder, and only under a well-formed nonce.
 */
export function isolatedReaderDocument(
  html: string, textSize: unknown, readingWidth: unknown, theme: unknown = "light",
  options: ReaderDocumentOptions = {},
): string {
  const fontSize = FONT_PX[isReaderTextSize(textSize) ? textSize : "standard"];
  const lineWidth = WIDTH_CH[isReaderWidth(readingWidth) ? readingWidth : "standard"];
  const dark = theme === "dark";
  const foreground = dark ? "#eaecf0" : "#202122";
  const background = dark ? "#202122" : "#ffffff";
  const systemDark = theme === "system"
    ? "@media (prefers-color-scheme: dark){html{background:#202122}body{color:#eaecf0}}"
    : "";
  const images = options.images === true;
  const nonce = typeof options.linkNonce === "string" && NONCE.test(options.linkNonce)
    ? options.linkNonce : null;
  const csp = [
    "default-src 'none'",
    `img-src ${images ? IMAGE_SOURCES : "'none'"}`,
    "font-src 'none'", "connect-src 'none'", "media-src 'none'", "frame-src 'none'",
    "form-action 'none'", "base-uri 'none'", "style-src 'unsafe-inline'",
    `script-src ${nonce ? `'nonce-${nonce}'` : "'none'"}`,
  ].join("; ");
  const imageStyle = images ? "img{max-width:100%;height:auto}" : "img,video,audio{display:none}";
  const linkStyle = nonce ? "" : "a{pointer-events:none}";
  return [
    "<!doctype html><html><head>",
    `<meta http-equiv="Content-Security-Policy" content="${csp}">`,
    "<meta name=\"referrer\" content=\"no-referrer\">",
    `<style>html{background:${background}}body{box-sizing:border-box;font:${fontSize}px/1.6 sans-serif;padding:1rem;margin:0 auto;max-width:${lineWidth}ch;color:${foreground}}table{max-width:100%;overflow-wrap:anywhere}${imageStyle}${linkStyle}pre{white-space:pre-wrap;overflow-x:auto}${wikipediaStyle(dark)}${systemDark}</style>`,
    nonce ? `<script nonce="${nonce}">${LINK_SCRIPT}</script>` : "",
    "</head><body>", html, "</body></html>",
  ].join("");
}
