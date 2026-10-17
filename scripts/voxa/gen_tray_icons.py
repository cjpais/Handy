"""Generate Voxa tray icons (64x64) into src-tauri/resources.

Template-style glyphs: white (dark menu bar) and black (light menu bar) on
transparent, plus coloured variants for Linux. Run from the repo root:

    python3 scripts/voxa/gen_tray_icons.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

OUT = Path("src-tauri/resources")
SIZE = 64
SCALE = 8  # supersample for smooth edges


def canvas():
    s = SIZE * SCALE
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img), s


def bars(draw, s, heights, color, x_off=0.0, y_off=0.0, scale=1.0):
    n = len(heights)
    bar_w = 7 * SCALE * scale
    gap = 4.6 * SCALE * scale
    total = n * bar_w + (n - 1) * gap
    x = (s - total) / 2 + x_off * SCALE
    cy = s / 2 + y_off * SCALE
    for h in heights:
        hh = h * SCALE * scale
        draw.rounded_rectangle(
            [x, cy - hh / 2, x + bar_w, cy + hh / 2], radius=bar_w / 2, fill=color
        )
        x += bar_w + gap


def dots(draw, s, color):
    r = 5.2 * SCALE
    cy = s / 2
    for dx in (-17, 0, 17):
        cx = s / 2 + dx * SCALE
        draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)


def finish(img, name):
    img.resize((SIZE, SIZE), Image.LANCZOS).save(OUT / name)


def idle(color):
    img, d, s = canvas()
    bars(d, s, [16, 30, 42, 30, 16], color)
    return img


def recording(color):
    img, d, s = canvas()
    bars(d, s, [22, 40, 54, 40, 22], color)
    return img


def transcribing(color):
    img, d, s = canvas()
    dots(d, s, color)
    return img


def warning(color, badge):
    img, d, s = canvas()
    bars(d, s, [14, 26, 36, 26, 14], color, x_off=-3, y_off=2)
    r = 9 * SCALE
    cx, cy = s - 14 * SCALE, 14 * SCALE
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=badge)
    return img


WHITE = (255, 255, 255, 255)
BLACK = (0, 0, 0, 255)
ACCENT = (110, 168, 255, 255)
CORAL = (255, 122, 122, 255)
AMBER = (251, 191, 36, 255)

# dark menu bar -> light glyphs, light menu bar -> dark glyphs (`_dark` suffix)
finish(idle(WHITE), "tray_idle.png")
finish(idle(BLACK), "tray_idle_dark.png")
finish(recording(WHITE), "tray_recording.png")
finish(recording(BLACK), "tray_recording_dark.png")
finish(transcribing(WHITE), "tray_transcribing.png")
finish(transcribing(BLACK), "tray_transcribing_dark.png")
finish(warning(WHITE, AMBER), "tray_idle_warning.png")
finish(warning(BLACK, AMBER), "tray_idle_warning_dark.png")
# coloured set (Linux "colored" theme, window icon)
finish(idle(ACCENT), "handy.png")
finish(recording(CORAL), "recording.png")
finish(transcribing(ACCENT), "transcribing.png")
finish(warning(ACCENT, AMBER), "handy_warning.png")
print("tray icons written to", OUT)
