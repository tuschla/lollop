#!/usr/bin/env python3
"""Render the iOS app icon and launch logo from the Android vector drawables.

    iosApp/make-assets.py        # rewrites iosApp/iosApp/Assets.xcassets/{AppIcon,LaunchLogo}

Sources (android/lollop/src/androidMain/res):
  mipmap/ic_launcher.xml           the full-square launcher icon (white ground,
                                   black mark): the iOS icon, and recoloured
                                   white on a clear ground for the dark and
                                   tinted appearances (iOS 18)
  drawable/splash_logo_{light,dark}.xml
                                   the splash marks: the launch screen logo for
                                   the light and dark appearance

One 1024 px icon per appearance is all Xcode 14+ needs; actool derives the
smaller sizes at build time. Needs rsvg-convert (librsvg) and Pillow. The
output PNGs are committed, so a Mac build doesn't need either.
"""
import json
import subprocess
import xml.etree.ElementTree as ET
from io import BytesIO
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
RES = ROOT / "android/lollop/src/androidMain/res"
ASSETS = ROOT / "iosApp/iosApp/Assets.xcassets"
A = "{http://schemas.android.com/apk/res/android}"


def svg_color(argb, recolor):
    """Android #RGB/#RRGGBB/#AARRGGBB → an SVG paint (and its opacity)."""
    c = argb.upper()
    c = recolor.get(c, c)
    if c is None:
        return "none", "1"
    h = c.lstrip("#")
    if len(h) == 3:
        h = "".join(ch * 2 for ch in h)
    alpha = "1"
    if len(h) == 8:
        alpha = f"{int(h[:2], 16) / 255:.4f}"
        h = h[2:]
    return "#" + h, alpha


def group_transform(g):
    """Android's group order: scale and rotate about the pivot, then translate."""
    f = lambda k, d: float(g.get(A + k, d))
    px, py = f("pivotX", 0), f("pivotY", 0)
    sx, sy = f("scaleX", 1), f("scaleY", 1)
    tx, ty = f("translateX", 0), f("translateY", 0)
    rot = f("rotation", 0)
    return (f"translate({tx + px} {ty + py}) rotate({rot}) "
            f"scale({sx} {sy}) translate({-px} {-py})")


def to_svg(node, recolor):
    out = []
    for child in node:
        if child.tag == "group":
            out.append(f'<g transform="{group_transform(child)}">{to_svg(child, recolor)}</g>')
        elif child.tag == "path":
            fill, fill_a = svg_color(child.get(A + "fillColor", "#00000000"), recolor)
            stroke, stroke_a = svg_color(child.get(A + "strokeColor", "#00000000"), recolor)
            cap = child.get(A + "strokeLineCap", "butt")
            width = child.get(A + "strokeWidth", "0")
            out.append(
                f'<path d="{child.get(A + "pathData")}" fill="{fill}" fill-opacity="{fill_a}" '
                f'stroke="{stroke}" stroke-opacity="{stroke_a}" stroke-width="{width}" '
                f'stroke-linecap="{cap}"/>'
            )
    return "".join(out)


def render(drawable, px, recolor=None):
    """Rasterise an Android <vector> to an RGBA image px×px."""
    root = ET.parse(RES / drawable).getroot()
    vw, vh = root.get(A + "viewportWidth"), root.get(A + "viewportHeight")
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{px}" height="{px}" '
           f'viewBox="0 0 {vw} {vh}">{to_svg(root, recolor or {})}</svg>')
    png = subprocess.run(["rsvg-convert", "--format=png"], input=svg.encode(),
                         capture_output=True, check=True).stdout
    return Image.open(BytesIO(png)).convert("RGBA")


def write_set(dirname, contents, images):
    d = ASSETS / dirname
    d.mkdir(parents=True, exist_ok=True)
    for old in d.glob("*.png"):
        old.unlink()
    for name, image in images.items():
        image.save(d / name, optimize=True)
    (d / "Contents.json").write_text(json.dumps(contents, indent=2) + "\n")


def main():
    ASSETS.mkdir(parents=True, exist_ok=True)
    (ASSETS / "Contents.json").write_text(json.dumps({"info": {"author": "xcode", "version": 1}}, indent=2) + "\n")

    # Light: the launcher icon as is, opaque (iOS fills an icon's alpha with black).
    light = render("mipmap/ic_launcher.xml", 1024).convert("RGB")
    # Dark and tinted: the mark in white on a clear ground; iOS supplies the
    # dark ground, and the tint from the image's luminance.
    on_clear = render("mipmap/ic_launcher.xml", 1024, {"#FFFFFF": None, "#000000": "#FFFFFF"})
    tinted = on_clear.convert("LA")
    icon = lambda f, appearance=None: {
        "filename": f, "idiom": "universal", "platform": "ios", "size": "1024x1024",
        **({"appearances": [{"appearance": "luminosity", "value": appearance}]} if appearance else {}),
    }
    write_set(
        "AppIcon.appiconset",
        {"images": [icon("AppIcon.png"), icon("AppIcon-dark.png", "dark"), icon("AppIcon-tinted.png", "tinted")],
         "info": {"author": "xcode", "version": 1}},
        {"AppIcon.png": light, "AppIcon-dark.png": on_clear, "AppIcon-tinted.png": tinted},
    )

    # The splash mark is 96 dp; as a launch image it is 96 pt at the scale's pixels.
    images, entries = {}, []
    for variant, appearance in (("light", None), ("dark", "dark")):
        for scale in (2, 3):
            name = f"launch-logo-{variant}@{scale}x.png"
            images[name] = render(f"drawable/splash_logo_{variant}.xml", 96 * scale)
            entries.append({
                "filename": name, "idiom": "universal", "scale": f"{scale}x",
                **({"appearances": [{"appearance": "luminosity", "value": appearance}]} if appearance else {}),
            })
    write_set("LaunchLogo.imageset", {"images": entries, "info": {"author": "xcode", "version": 1}}, images)

    # The launch ground: the splash's white, black in dark mode (values/colors.xml splash_bg_*).
    color = lambda v, appearance=None: {
        "idiom": "universal",
        "color": {"color-space": "srgb", "components": {"red": v, "green": v, "blue": v, "alpha": "1.000"}},
        **({"appearances": [{"appearance": "luminosity", "value": appearance}]} if appearance else {}),
    }
    d = ASSETS / "LaunchBackground.colorset"
    d.mkdir(parents=True, exist_ok=True)
    (d / "Contents.json").write_text(json.dumps(
        {"colors": [color("1.000"), color("0.000", "dark")], "info": {"author": "xcode", "version": 1}},
        indent=2) + "\n")


if __name__ == "__main__":
    main()
