/**
 * The CJK subset font, for Handy's own CJK strings only (language names,
 * Demo's lines): text outside the subset would mix fonts mid-line.
 */
const CJK =
  /[\p{Script=Han}\p{Script=Hangul}\p{Script=Hiragana}\p{Script=Katakana}]/u;

export const cjkSubsetFont = (text: string) =>
  CJK.test(text) ? "font-cjk-subset" : undefined;
