"""Regenerate every SAYELF brand asset from branding/sayelf-logo-master.png.

Outputs (all committed, so normal builds do not need to run this):
  src-tauri/icons/icon.ico            app / tray / installer / uninstaller icon, 16-256 px
  src-tauri/icons/nsis-header.bmp     installer page header, 150x57, 24-bit
  src-tauri/icons/nsis-sidebar.bmp    installer welcome/finish sidebar, 164x314, 24-bit
  src-tauri/icons/tray-32.rgba        tray icon, raw 32x32 RGBA (embedded with include_bytes!, no PNG decoder needed)
  src/assets/sayelf-logo.webp         wheel hub logo, 192x192 (covers 200% DPI)

Usage (Windows or Linux, Python 3.9+):  pip install pillow  &&  python tools/make-brand-assets.py
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont, ImageEnhance

ROOT = Path(__file__).resolve().parent.parent
MASTER = ROOT / 'branding' / 'sayelf-logo-master.png'
ICONS = ROOT / 'src-tauri' / 'icons'
FONT_CANDIDATES = [
    ('C:/Windows/Fonts/msyhbd.ttc', 0), ('C:/Windows/Fonts/msyh.ttc', 0),
    ('/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc', 2), ('/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc', 2),
]
FOREST = (20, 46, 30)      # deep moss green from the logo background
FOREST_2 = (37, 74, 44)
CREAM = (244, 241, 230)


def font(size: int) -> ImageFont.FreeTypeFont:
    for path, index in FONT_CANDIDATES:
        if Path(path).exists():
            return ImageFont.truetype(path, size, index=index)
    return ImageFont.load_default(size)


def circle(image: Image.Image, size: int) -> Image.Image:
    """Circular crop with anti-aliased edge (drawn at 4x, scaled down)."""
    big = image.resize((size * 4, size * 4), Image.LANCZOS)
    mask = Image.new('L', big.size, 0)
    ImageDraw.Draw(mask).ellipse((0, 0, big.width - 1, big.height - 1), fill=255)
    out = Image.new('RGBA', big.size, (0, 0, 0, 0))
    out.paste(big, (0, 0), mask)
    return out.resize((size, size), Image.LANCZOS)


def main() -> None:
    master = Image.open(MASTER).convert('RGB')
    w, h = master.size
    # At 16-32 px the SAYELF wordmark is unreadable noise; the elf-with-camera crop still reads.
    elf = master.crop((int(w * 0.10), int(h * 0.20), int(w * 0.60), int(h * 0.74)))
    elf = ImageEnhance.Contrast(elf).enhance(1.25)

    sizes = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
    frames = [circle(elf if s <= 32 else master, s) for s in sizes]
    ICONS.mkdir(parents=True, exist_ok=True)
    frames[-1].save(ICONS / 'icon.ico', format='ICO', sizes=[(s, s) for s in sizes], append_images=frames[:-1])

    # Header: 150x57, logo on the left, wordmark on the right, white background (NSIS page header is white).
    header = Image.new('RGB', (150, 57), (255, 255, 255))
    logo = circle(master, 49)
    header.paste(logo, (4, 4), logo)
    d = ImageDraw.Draw(header)
    d.text((60, 8), 'SAYELF', font=font(17), fill=FOREST)
    d.text((60, 31), '山野精灵', font=font(13), fill=FOREST_2)
    header.save(ICONS / 'nsis-header.bmp', format='BMP')

    # Sidebar: 164x314, moss-green gradient, logo + product name.
    side = Image.new('RGB', (164, 314), FOREST)
    g = ImageDraw.Draw(side)
    for y in range(314):
        t = y / 313
        g.line([(0, y), (163, y)], fill=tuple(int(FOREST_2[i] * (1 - t) + FOREST[i] * t) for i in range(3)))
    big = circle(master, 128)
    ring = Image.new('RGBA', (136, 136), (0, 0, 0, 0))
    ImageDraw.Draw(ring).ellipse((0, 0, 135, 135), fill=CREAM + (255,))
    side.paste(ring, (14, 30), ring)
    side.paste(big, (18, 34), big)
    def centered(text: str, y: int, size: int, fill) -> None:
        f = font(size)
        width = g.textlength(text, font=f)
        g.text(((164 - width) / 2, y), text, font=f, fill=fill)
    centered('Context Wheel', 186, 17, CREAM)
    centered('CAD 鼠标轮盘', 212, 14, (200, 222, 196))
    g.line([(42, 246), (122, 246)], fill=(120, 160, 112), width=1)
    centered('SAYELF · 山野精灵', 258, 12, (200, 222, 196))
    centered('Watch an AI become a Self', 280, 9, (150, 184, 146))
    side.save(ICONS / 'nsis-sidebar.bmp', format='BMP')

    tray = circle(elf, 32)
    (ICONS / 'tray-32.rgba').write_bytes(tray.tobytes())  # 32*32*4 = 4096 bytes, row-major RGBA

    hub = master.resize((192, 192), Image.LANCZOS)
    hub.save(ROOT / 'src' / 'assets' / 'sayelf-logo.webp', format='WEBP', quality=88, method=6)
    for p in [ICONS / 'icon.ico', ICONS / 'nsis-header.bmp', ICONS / 'nsis-sidebar.bmp', ICONS / 'tray-32.rgba', ROOT / 'src' / 'assets' / 'sayelf-logo.webp']:
        print(f'{p.relative_to(ROOT)}  {p.stat().st_size:,} bytes')


if __name__ == '__main__':
    main()
