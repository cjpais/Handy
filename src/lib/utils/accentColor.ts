function hexToHsl(hex: string): [number, number, number] {
  const n = parseInt(hex.replace("#", ""), 16);
  const r = ((n >> 16) & 255) / 255,
    g = ((n >> 8) & 255) / 255,
    b = (n & 255) / 255;
  const max = Math.max(r, g, b),
    min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l * 100];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h =
    max === r
      ? ((g - b) / d + (g < b ? 6 : 0)) / 6
      : max === g
        ? ((b - r) / d + 2) / 6
        : ((r - g) / d + 4) / 6;
  return [h * 360, s * 100, l * 100];
}

function hslToHex(h: number, s: number, l: number): string {
  const s1 = s / 100;
  const l1 = l / 100;
  const a = s1 * Math.min(l1, 1 - l1);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const c = l1 - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
    return Math.round(255 * Math.max(0, Math.min(1, c)))
      .toString(16)
      .padStart(2, "0");
  };
  return `#${f(0)}${f(8)}${f(4)}`;
}

const clamp = (v: number, min: number, max: number) =>
  Math.min(max, Math.max(min, v));

export function applyAccentColor(hex: string | undefined | null): void {
  const root = document.documentElement;
  const props = [
    "--color-background-ui",
    "--color-logo-primary",
    "--color-logo-stroke",
    "--color-accent-bar",
    "--color-accent-hover",
  ];

  if (!hex || !/^#[0-9a-fA-F]{6}$/.test(hex)) {
    props.forEach((p) => root.style.removeProperty(p));
    return;
  }

  const [h, s, l] = hexToHsl(hex);

  root.style.setProperty(
    "--color-background-ui",
    hslToHex(h, s, clamp(l - 15, 15, 85)),
  );
  root.style.setProperty("--color-logo-primary", hex);
  root.style.setProperty(
    "--color-logo-stroke",
    hslToHex(h, s * 0.5, clamp(l - 35, 5, 70)),
  );
  root.style.setProperty(
    "--color-accent-bar",
    hslToHex(h, s * 0.35, clamp(l + 25, 50, 93)),
  );
  root.style.setProperty("--color-accent-hover", hex + "33");
}
