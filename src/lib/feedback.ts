/**
 * How long each class of transient message stays on screen, in every window.
 *
 * The same news used to be shown for six different lengths - a failed action for
 * 2.2s here and 3.8s there, the identical share result for 2.6s in the ring and
 * 3.6s in the main window - which reads as a different app each time. One ladder,
 * and a message of the same kind takes the same time wherever it lands.
 */
export const FEEDBACK = {
  /** A thing finished. */
  done: 1400,
  /** A short prompt about the gesture in progress. */
  hint: 1800,
  /** A result the user asked for, with no follow-up needed. */
  notice: 2600,
  /** Something failed; the sentence is long and has to be read. */
  error: 3800,
  /** A task is running; the next progress line replaces this one. */
  running: 12_000,
  /** A task is running and reports progress slowly, so nothing may expire. */
  hold: 120_000,
} as const;

/**
 * The outcome keys Rust is allowed to name.
 *
 * A result used to arrive as an English sentence written in Rust, so a Chinese
 * interface answered "Convert" with `Saved 1 file.` while it answered every other
 * action in Chinese. Rust now names the outcome and this decides the wording, so
 * the count comes from the outputs rather than a number baked into a sentence.
 */
const PLAIN_OUTCOMES = new Set([
  "outcomeRevealed",
  "outcomeOpened",
  "outcomeMergedPdfs",
  "outcomeImagesToPdf",
  "outcomePdfText",
]);
/** The two that count outputs, so one output needs no number at all. */
const COUNTED_OUTCOMES = new Set(["outcomeSaved", "outcomePdfPages"]);

/**
 * Word an action's result. Anything Rust does not name - a plain save, or a build
 * whose Rust is older than this list - is simply "saved", never a raw token.
 */
export function outcomeMessage(
  message: string,
  outputs: number,
  translate: (key: string, vars?: Record<string, string | number>) => string,
): string {
  if (PLAIN_OUTCOMES.has(message)) return translate(message);
  if (COUNTED_OUTCOMES.has(message) && outputs !== 1) {
    return translate(message, { count: outputs });
  }
  return translate("saved");
}
