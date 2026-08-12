#!/usr/bin/env python3
"""Render the successful onboarding demo log as a small terminal-style GIF."""

from __future__ import annotations

import argparse
from pathlib import Path
import textwrap

from PIL import Image, ImageDraw, ImageFont


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LOG = REPO_ROOT / "target" / "experience-onboarding-demo" / "session.log"
DEFAULT_OUTPUT = REPO_ROOT / "docs" / "assets" / "oris-experience-onboarding.gif"
FONT_CANDIDATES = [
    Path("/System/Library/Fonts/SFNSMono.ttf"),
    Path("/System/Library/Fonts/SFNSMonoItalic.ttf"),
    Path("/System/Library/Fonts/Menlo.ttc"),
    Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
]


def load_font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for path in FONT_CANDIDATES:
        if path.is_file():
            return ImageFont.truetype(str(path), size=size)
    return ImageFont.load_default()


def wrap_lines(lines: list[str], width: int = 86) -> list[str]:
    wrapped: list[str] = []
    for line in lines:
        if not line:
            wrapped.append("")
            continue
        wrapped.extend(
            textwrap.wrap(
                line,
                width=width,
                subsequent_indent="      " if line.startswith("      ") else "",
                replace_whitespace=False,
                drop_whitespace=False,
            )
        )
    return wrapped


def render_frame(lines: list[str], font: ImageFont.ImageFont) -> Image.Image:
    width, height = 960, 540
    image = Image.new("RGB", (width, height), "#0b1020")
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle((18, 18, width - 18, height - 18), radius=16, fill="#111827")
    for x, color in [(42, "#ff5f57"), (66, "#febc2e"), (90, "#28c840")]:
        draw.ellipse((x - 6, 37 - 6, x + 6, 37 + 6), fill=color)
    draw.text((120, 27), "oris · local verified experience demo", font=font, fill="#94a3b8")
    draw.line((28, 58, width - 28, 58), fill="#253047", width=1)

    visible = wrap_lines(lines)[-20:]
    y = 78
    for line in visible:
        color = "#e5e7eb"
        if line.startswith("ORIS EXPERIENCE"):
            color = "#67e8f9"
        elif line.startswith("["):
            color = "#a5b4fc"
        elif "✓" in line:
            color = "#86efac"
        elif line.startswith("RESULT: PASS"):
            color = "#facc15"
        draw.text((42, y), line, font=font, fill=color)
        y += 22
    return image


def render(log_path: Path, output_path: Path) -> None:
    lines = log_path.read_text(encoding="utf-8").splitlines()
    if not lines or not lines[-1].startswith("RESULT: PASS"):
        raise RuntimeError("refusing to render a GIF from an unsuccessful demo log")
    font = load_font(17)
    frames: list[Image.Image] = []
    durations: list[int] = []
    for index in range(1, len(lines) + 1):
        frames.append(render_frame(lines[:index], font))
        durations.append(650 if index < len(lines) else 2800)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    frames[0].save(
        output_path,
        save_all=True,
        append_images=frames[1:],
        duration=durations,
        loop=0,
        optimize=True,
        disposal=2,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", type=Path, default=DEFAULT_LOG)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    render(args.log.resolve(), args.output.resolve())
    print(f"Rendered {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
