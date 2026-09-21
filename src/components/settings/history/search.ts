export interface TextSegment {
  text: string;
  match: boolean;
}

// Mirrors the backend's case-insensitive substring match; an empty query matches everything.
export const matchesSearch = (text: string, query: string): boolean =>
  text.toLowerCase().includes(query.trim().toLowerCase());

const escapeRegExp = (value: string) =>
  value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Splits `text` into plain and matching segments so search hits can be
 * highlighted. Uses a Unicode case-insensitive regex rather than lowercasing,
 * because lowercasing can change string length and shift match offsets.
 */
export const splitByQuery = (text: string, query: string): TextSegment[] => {
  const needle = query.trim();
  if (!needle) return [{ text, match: false }];

  const segments: TextSegment[] = [];
  let lastIndex = 0;
  for (const match of text.matchAll(new RegExp(escapeRegExp(needle), "giu"))) {
    const start = match.index;
    if (start > lastIndex) {
      segments.push({ text: text.slice(lastIndex, start), match: false });
    }
    segments.push({ text: match[0], match: true });
    lastIndex = start + match[0].length;
  }
  if (lastIndex < text.length) {
    segments.push({ text: text.slice(lastIndex), match: false });
  }
  return segments;
};
