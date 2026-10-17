/**
 * WCAG contrast of the capsule's text against its glass.
 *
 * The glass is translucent, so the colour behind the text depends on the
 * desktop. The check runs the text over the fill blended with the extreme
 * backdrops the system HUD material produces in each appearance (dark
 * appearance: near-black to mid-dark grey; light appearance: mid-light grey to
 * white) and demands 4.5:1 in the worst case.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

type Rgba = [number, number, number, number];

const css = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "glass-pill.css"),
  "utf8",
);

function tokens(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of block.matchAll(/--(vgp-[a-z-]+):\s*([^;]+);/g)) {
    out[m[1]] = m[2].trim();
  }
  return out;
}

const lightBlock = css.slice(
  css.indexOf(".vg-pill {\n  /* ---- light"),
  css.indexOf("@media (prefers-color-scheme: dark)"),
);
const darkStart = css.indexOf("@media (prefers-color-scheme: dark)");
const darkBlock = css.slice(
  darkStart,
  css.indexOf('[data-theme="light"] .vg-pill'),
);
const light = tokens(lightBlock);
const dark = { ...light, ...tokens(darkBlock) };

function parse(value: string): Rgba {
  const rgba = /^rgba?\(([^)]+)\)$/.exec(value);
  if (rgba) {
    const [r, g, b, a = "1"] = rgba[1].split(",").map((s) => s.trim());
    return [Number(r), Number(g), Number(b), Number(a)];
  }
  const hex = /^#([0-9a-f]{6})$/i.exec(value);
  assert.ok(hex, `cannot parse colour ${value}`);
  const n = parseInt(hex[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
}

/** `top` painted over an opaque `bottom`. */
function over(top: Rgba, bottom: Rgba): Rgba {
  const a = top[3];
  return [
    top[0] * a + bottom[0] * (1 - a),
    top[1] * a + bottom[1] * (1 - a),
    top[2] * a + bottom[2] * (1 - a),
    1,
  ];
}

function luminance([r, g, b]: Rgba): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(fg: Rgba, bg: Rgba): number {
  const text = over(fg, bg);
  const [hi, lo] = [luminance(text), luminance(bg)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const BACKDROPS = {
  dark: [parse("#101114"), parse("#4a4c52")],
  light: [parse("#b8bcc4"), parse("#ffffff")],
} as const;

const themes = { dark, light } as const;
let checks = 0;

for (const [name, t] of Object.entries(themes) as [
  keyof typeof themes,
  Record<string, string>,
][]) {
  for (const backdrop of BACKDROPS[name]) {
    const glass = over(parse(t["vgp-fill"]), backdrop);
    for (const ink of ["vgp-ink", "vgp-ink-soft"]) {
      const ratio = contrast(parse(t[ink]), glass);
      assert.ok(
        ratio >= 4.5,
        `${name} ${ink} on ${backdrop} glass: ${ratio.toFixed(2)}:1`,
      );
      checks += 1;
    }
    // error state: the tint sits on the glass, the error ink on both
    const tinted = over(parse(t["vgp-error-tint"]), glass);
    const errorRatio = contrast(parse(t["vgp-error-ink"]), tinted);
    assert.ok(
      errorRatio >= 4.5,
      `${name} error text on ${backdrop}: ${errorRatio.toFixed(2)}:1`,
    );
    // the green check is a graphic: 3:1 is the bar for non-text
    const okRatio = contrast(parse(t["vgp-ok"]), glass);
    assert.ok(
      okRatio >= 3,
      `${name} check mark on ${backdrop}: ${okRatio.toFixed(2)}:1`,
    );
    checks += 2;
  }
  // the opaque fallbacks (no native glass / reduced transparency)
  for (const solid of ["vgp-solid", "vgp-fill-fallback"]) {
    const bg = over(parse(t[solid]), BACKDROPS[name][name === "dark" ? 1 : 0]);
    assert.ok(
      contrast(parse(t["vgp-ink"]), bg) >= 4.5,
      `${name} ink on ${solid}`,
    );
    assert.ok(
      contrast(parse(t["vgp-ink-soft"]), bg) >= 4.5,
      `${name} soft ink on ${solid}`,
    );
    checks += 2;
  }
}

console.log(`voxa overlay contrast: ${checks} checks passed`);
