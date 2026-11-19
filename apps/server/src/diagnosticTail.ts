/** Errors are diagnostics, not transcripts. Full dispatch output belongs in CLI artifacts. */
export const MAX_DIAGNOSTIC_CHARS = 16 * 1024;
export const DIAGNOSTIC_TRUNCATION = '[Earlier diagnostic output omitted]\n';

export function diagnosticTail(text: string): string {
  if (text.length <= MAX_DIAGNOSTIC_CHARS) return text;
  // Copy the slice so a small diagnostic cannot retain a much larger backing string.
  return DIAGNOSTIC_TRUNCATION + Buffer.from(text.slice(-MAX_DIAGNOSTIC_CHARS), 'utf16le').toString('utf16le');
}
