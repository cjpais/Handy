"""Generate the synthetic "desktop" backgrounds used by the overlay preview.

No real macOS screenshots are available in the build sandbox, so the preview
page puts the recording capsule on procedurally drawn desktops: a dark one, a
light one and a busy one (stripes and fine text) that stresses the glass.
Deterministic; run from the repo root:

    python3 scripts/voxa/gen_desktop_backgrounds.py
"""

import random
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

W, H = 1600, 900
OUT = Path("docs/assets")
MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
SANS = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"


def gradient(stops, angle=0.0):
    """Linear gradient image; `stops` is [(pos, (r, g, b)), ...]."""
    ys, xs = np.mgrid[0:H, 0:W]
    t = (xs * np.cos(angle) + ys * np.sin(angle)) / (W * abs(np.cos(angle)) + H * abs(np.sin(angle)))
    t = (t - t.min()) / (t.max() - t.min())
    out = np.zeros((H, W, 3), dtype=np.float32)
    for (p0, c0), (p1, c1) in zip(stops, stops[1:]):
        mask = (t >= p0) & (t <= p1)
        k = ((t - p0) / (p1 - p0))[..., None]
        out = np.where(mask[..., None], np.array(c0) * (1 - k) + np.array(c1) * k, out)
    return Image.fromarray(out.clip(0, 255).astype(np.uint8))


def glow(img, cx, cy, radius, color, strength):
    layer = Image.new("RGB", (W, H), (0, 0, 0))
    d = ImageDraw.Draw(layer)
    d.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], fill=color)
    layer = layer.filter(ImageFilter.GaussianBlur(radius * 0.55))
    a = np.asarray(img).astype(np.float32)
    b = np.asarray(layer).astype(np.float32) * strength
    return Image.fromarray((a + b).clip(0, 255).astype(np.uint8))


def dock(img, dark):
    d = ImageDraw.Draw(img, "RGBA")
    n, size, gap = 9, 54, 12
    total = n * size + (n - 1) * gap + 28
    x0 = (W - total) // 2
    y0 = H - 86
    d.rounded_rectangle(
        [x0, y0, x0 + total, y0 + 74],
        radius=24,
        fill=(255, 255, 255, 60 if dark else 120),
        outline=(255, 255, 255, 90 if dark else 160),
    )
    palette = [(255, 95, 86), (255, 189, 46), (39, 201, 63), (66, 165, 245), (171, 71, 188),
               (255, 138, 101), (38, 198, 218), (255, 213, 79), (126, 87, 194)]
    for i in range(n):
        x = x0 + 14 + i * (size + gap)
        d.rounded_rectangle([x, y0 + 10, x + size, y0 + 10 + size], radius=13, fill=palette[i] + (235,))
    return img


def menubar(img, dark):
    d = ImageDraw.Draw(img, "RGBA")
    d.rectangle([0, 0, W, 30], fill=(0, 0, 0, 90) if dark else (255, 255, 255, 140))
    f = ImageFont.truetype(SANS, 14)
    ink = (255, 255, 255, 230) if dark else (20, 25, 40, 230)
    for i, label in enumerate(["Voxa", "File", "Edit", "View", "Window", "Help"]):
        d.text((24 + i * 70, 7), label, font=f, fill=ink)
    d.text((W - 150, 7), "Sat 17:42", font=f, fill=ink)
    return img


def window(img, box, title, dark, lines=None, code=False, seed=1):
    d = ImageDraw.Draw(img, "RGBA")
    x0, y0, x1, y1 = box
    d.rounded_rectangle(
        [x0, y0, x1, y1], radius=14,
        fill=(28, 30, 38, 245) if dark else (252, 252, 254, 250),
        outline=(255, 255, 255, 40) if dark else (0, 0, 0, 40),
    )
    for i, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        d.ellipse([x0 + 16 + i * 22, y0 + 14, x0 + 28 + i * 22, y0 + 26], fill=c + (255,))
    f = ImageFont.truetype(SANS, 13)
    d.text(((x0 + x1) // 2 - 40, y0 + 12), title, font=f, fill=(190, 195, 210, 255) if dark else (70, 75, 90, 255))
    rnd = random.Random(seed)
    font = ImageFont.truetype(MONO if code else SANS, 14)
    y = y0 + 50
    for line in lines or []:
        d.text((x0 + 24, y), line, font=font, fill=(150, 235, 170, 255) if (dark and code) else ((200, 205, 220, 255) if dark else (60, 65, 80, 255)))
        y += 22
    while y < y1 - 30:
        w = rnd.randint(120, int((x1 - x0) * 0.8))
        d.rounded_rectangle([x0 + 24, y + 4, x0 + 24 + w, y + 12], radius=4,
                            fill=(255, 255, 255, 38) if dark else (0, 0, 0, 28))
        y += 24
    return img


def dark_desktop():
    img = gradient([(0, (10, 14, 34)), (0.5, (28, 18, 70)), (1, (56, 26, 92))], angle=0.9)
    img = glow(img, 260, 160, 380, (0, 120, 255), 0.55)
    img = glow(img, 1350, 700, 420, (230, 60, 160), 0.45)
    img = glow(img, 900, 80, 300, (0, 210, 190), 0.25)
    img = window(img, (150, 120, 780, 520), "zsh - voxa", True,
                 ["$ git log --oneline", "554eec7 ci: add voxa-macos workflow",
                  "af52749 docs: record blocker", "$ bun run tauri build", "   Compiling handy v0.9.8"], code=True)
    img = window(img, (840, 200, 1460, 640), "Claude", True,
                 ["Настроил RAG-пайплайн на LangChain,", "промпт для Claude, заметка в Obsidian."], seed=4)
    return dock(menubar(img, True), True)


def light_desktop():
    img = gradient([(0, (205, 222, 255)), (0.5, (246, 226, 255)), (1, (255, 236, 220))], angle=0.8)
    img = glow(img, 300, 700, 360, (120, 160, 255), 0.35)
    img = glow(img, 1300, 150, 340, (255, 170, 190), 0.3)
    img = window(img, (200, 110, 900, 560), "Obsidian - Заметки", False,
                 ["# Дейлик", "- настроить хоткей диктовки", "- проверить задержку на M2", "- записать выводы в заметки"], seed=2)
    img = window(img, (960, 220, 1480, 680), "Safari", False, ["voxa - личная диктовка"], seed=7)
    return dock(menubar(img, False), False)


def busy_desktop():
    img = Image.new("RGB", (W, H))
    d = ImageDraw.Draw(img)
    rnd = random.Random(11)
    colors = [(255, 87, 51), (255, 195, 0), (46, 204, 113), (52, 152, 219), (155, 89, 182), (236, 240, 241), (20, 20, 30)]
    for i in range(0, W, 40):
        d.rectangle([i, 0, i + 40, H], fill=colors[(i // 40) % len(colors)])
    for _ in range(90):
        x, y, r = rnd.randint(0, W), rnd.randint(0, H), rnd.randint(30, 160)
        d.ellipse([x - r, y - r, x + r, y + r], fill=rnd.choice(colors))
    f = ImageFont.truetype(SANS, 15)
    for y in range(40, H - 120, 20):
        d.text((rnd.randint(0, 200), y), "voxa dictation overlay legibility stress test " * 8, font=f, fill=(0, 0, 0))
    img = img.filter(ImageFilter.GaussianBlur(1.2))
    return dock(menubar(img, True), False)


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for name, make in [("desktop-dark", dark_desktop), ("desktop-light", light_desktop), ("desktop-busy", busy_desktop)]:
        image = make()
        image.save(OUT / f"{name}.jpg", quality=86, optimize=True)
        print("wrote", OUT / f"{name}.jpg")
