/**
 * Chinese, Japanese and Korean text that Handy writes itself and sets beside
 * other languages (languages' own names, Demo's lines) takes a font cut to
 * just those characters, on Windows and Linux (App.css; rebuilt by
 * scripts/build-cjk-font.ts). Not for anything else: text the cut doesn't
 * hold would mix it with the system font mid-line.
 */
const CJK =
  /[\p{Script=Han}\p{Script=Hangul}\p{Script=Hiragana}\p{Script=Katakana}]/u;

export const cjkSubsetFont = (text: string) =>
  CJK.test(text) ? "font-cjk-subset" : undefined;
