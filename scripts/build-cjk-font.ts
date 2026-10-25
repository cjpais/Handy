/**
 * Builds src/assets/fonts/cjk-subset.woff2: Noto Sans CJK (variable, every
 * weight) cut to the Chinese, Japanese and Korean characters Handy sets
 * beside other languages (see src/lib/utils/cjkFont.ts):
 *
 * - every language's own name Intl can give in those scripts, with their
 *   script and region forms (a webview's ICU may spell one differently)
 * - the app-language menu's and Chinese script choice's names
 * - Demo's sample lines
 *
 * Run after changing any of those: `bun run build:cjk-font`.
 * `--check` fails if the committed font is out of date.
 */
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import subsetFont from "subset-font";
import { LANGUAGE_METADATA } from "../src/i18n/languages";
import { CHINESE_SCRIPTS } from "../src/components/onboarding/data/languages";
import { SAMPLES } from "../src/components/onboarding/data/samples";

const SOURCE_URL =
  "https://github.com/notofonts/noto-cjk/raw/Sans2.004/Sans/Variable/OTF/NotoSansCJKsc-VF.otf";
const SOURCE_SHA256 =
  "2745e9681cb9d8a5c8901b62c9e1bd98c9c774365fc3b84dd467621013b51cd3";
const CACHE = join(
  import.meta.dir,
  "../node_modules/.cache/noto-cjk/NotoSansCJKsc-VF.otf",
);
const OUT = join(import.meta.dir, "../src/assets/fonts/cjk-subset.woff2");

// The scripts by their extensions, so marks they share count too (ー, 。),
// and the full-width forms (？).
const CJK =
  /[\p{scx=Han}\p{scx=Hangul}\p{scx=Hiragana}\p{scx=Katakana}\uff00-\uffef]/gu;

const intlNames = (): string[] => {
  const a = "abcdefghijklmnopqrstuvwxyz";
  const bases: string[] = [];
  for (const x of a)
    for (const y of a) {
      bases.push(x + y);
      for (const z of a) bases.push(x + y + z);
    }
  const names: string[] = [];
  for (const base of bases)
    for (const tag of ["", "-Hans", "-Hant", "-CN", "-HK", "-TW"]) {
      const code = base + tag;
      try {
        const name = new Intl.DisplayNames([code], {
          type: "language",
          fallback: "none",
        }).of(code);
        if (name) names.push(name);
      } catch {
        // Not a valid tag.
      }
    }
  return names;
};

const characters = (): string => {
  const text = [
    ...intlNames(),
    ...Object.values(LANGUAGE_METADATA).map((l) => l.nativeName),
    ...CHINESE_SCRIPTS.map((s) => s.native),
    ...Object.values(SAMPLES),
  ].join("");
  return [...new Set(text.match(CJK))].sort().join("");
};

const source = async (): Promise<Buffer> => {
  if (!existsSync(CACHE)) {
    console.log(`Downloading ${SOURCE_URL}`);
    const res = await fetch(SOURCE_URL);
    if (!res.ok) throw new Error(`Download failed: ${res.status}`);
    mkdirSync(dirname(CACHE), { recursive: true });
    writeFileSync(CACHE, Buffer.from(await res.arrayBuffer()));
  }
  const buf = readFileSync(CACHE);
  const sha = createHash("sha256").update(buf).digest("hex");
  if (sha !== SOURCE_SHA256)
    throw new Error(`${CACHE} has sha256 ${sha}, expected ${SOURCE_SHA256}`);
  return buf;
};

const chars = characters();
const font = await subsetFont(await source(), chars, {
  targetFormat: "woff2",
  // Keep the copyright and license (OFL) with the font.
  preserveNameIds: [0, 1, 2, 3, 4, 5, 6, 13, 14],
});

if (process.argv.includes("--check")) {
  if (!existsSync(OUT) || !readFileSync(OUT).equals(font)) {
    console.error(
      "src/assets/fonts/cjk-subset.woff2 is out of date: run `bun run build:cjk-font`.",
    );
    process.exit(1);
  }
  console.log("cjk-subset.woff2 is up to date.");
} else {
  writeFileSync(OUT, font);
  console.log(
    `Wrote ${OUT}: ${[...chars].length} characters, ${font.length} bytes.`,
  );
}
