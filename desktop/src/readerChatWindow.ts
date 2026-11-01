/** Keep a movable, resizable desktop chat inside the current viewport.
 * CSS caps the panel's actual size; this helper never trusts stored coordinates.
 * A margin is used only when both sides have enough space for it. */
export type ReaderChatPosition = { left: number; top: number };

export function clampReaderChatPosition(
  left: number, top: number, width: number, height: number,
  viewportWidth: number, viewportHeight: number,
): ReaderChatPosition {
  if (![left, top, width, height, viewportWidth, viewportHeight].every(Number.isFinite) ||
      width <= 0 || height <= 0 || viewportWidth <= 0 || viewportHeight <= 0) {
    throw new Error("Invalid chat window geometry");
  }
  const xMargin = viewportWidth >= width + 16 ? 8 : 0;
  const yMargin = viewportHeight >= height + 16 ? 8 : 0;
  return {
    left: Math.max(xMargin, Math.min(left, Math.max(xMargin, viewportWidth - width - xMargin))),
    top: Math.max(yMargin, Math.min(top, Math.max(yMargin, viewportHeight - height - yMargin))),
  };
}
