#!/usr/bin/env python3
"""Slice the mascot contact sheet (corgit.png) into individual PNGs.

The sheet is a single raster image containing every Corgit pose, the app mark in
five contexts and the compact mini indicators. This cuts each cell out on its
own, trimmed to the artwork's real bounding box.

Poses, the round app mark and the minis are written twice: once as a faithful
crop with the sheet's paper background, and once with that background (plus the
soft halo and floor shadow, which are the same family of greys) knocked out to
alpha. The knock-out floods in from the crop's border rather than thresholding
globally, because the cream markings -- muzzle, chest, paws -- are within a
hair of the paper colour and a global threshold punches holes through them.
The five app-mark context tiles are crop-only: their own backgrounds are part
of the artwork.

Three things come out of a run: the slices themselves, the subset the app
actually imports (`src/lib/mascot/`), and the icon pipeline's square source
(`src-tauri/icon-source.png`, expanded by `npm run icon`). The app assets are
copies rather than imports out of `docs/` so that shipping code never reaches
into the documentation tree.

`--eyes` is a second, independent run that splits the resting pose into an
eyeless base and its two pupils, for the idle gaze (see EYES). It reads the
app asset rather than the sheet, so it can be run without regenerating
anything else.

    python scripts/extract-mascot.py [sheet.png] [out-dir]
    python scripts/extract-mascot.py --eyes

Requires Pillow and numpy. Nothing in the build runs this; it is an asset step
kept in the repo so the slicing is reproducible.
"""

import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

# x-range of the cell, and the y-range to search within, on the 1536x1024 sheet.
# The exact bounding box is found inside these; they only need to isolate the cell.
CELLS = [
    # name,                              x0,   x1,   y0,   y1,   alpha
    ("pose-resting",                      10,  285,   60,  410, True),
    ("pose-content",                     292,  650,   60,  410, True),
    ("pose-working",                     656, 1032,   60,  410, True),
    ("pose-sorry",                      1034, 1262,   60,  410, True),
    ("app-mark",                        1270, 1535,   60,  410, True),

    ("app-mark-tile-dark",                35,  202,  640,  812, False),
    ("app-mark-tile-light",              235,  402,  640,  812, False),
    ("app-mark-tile-small",              450,  532,  690,  775, False),
    ("app-mark-mono-light-on-dark",      606,  728,  665,  790, False),
    ("app-mark-mono-dark-on-light",      775,  900,  665,  790, False),

    ("mini-resting",                     985, 1078,  670,  770, True),
    ("mini-content",                    1105, 1222,  670,  770, True),
    ("mini-working",                    1252, 1372,  670,  770, True),
    ("mini-sorry",                      1404, 1490,  670,  770, True),
]

# Which slices the app imports, and what it calls them. Native crop resolution
# is kept: the largest is 382px wide and the biggest on-screen use is ~130px,
# so these are already the 2x asset and downscaling further would cost detail
# on a high-DPI display for a few tens of kilobytes.
APP_ASSETS = {
    "pose-resting": "resting",
    "pose-content": "content",
    "pose-working": "working",
    "pose-sorry": "sorry",
    "app-mark": "mark",
    "mini-working": "mini-working",
    "mini-sorry": "mini-sorry",
}
APP_DIR = Path("src/lib/mascot")
# 512 to match what the icon pipeline was already fed. The mark is only 250px
# on the sheet, so this upscales roughly 2x -- soft, and the reason to redraw
# the head larger if the icon ever needs to be sharper than "fine at 128".
ICON_SOURCE = Path("src-tauri/icon-source.png")
ICON_SIZE = 512

PAD = 6
# How far (sum of per-channel deltas) a pixel may sit from the paper colour and
# still be flooded away as paper. Deliberately tight: the cream markings are
# only ~10-25 away from the paper, and several of the lines penning them in are
# pale grey, so a loose flood walks straight through the muzzle.
FLOOD_TOLERANCE = 26
# The decorative halo behind the seated poses and the floor shadow are too far
# from the paper for that flood, so they get a second pass that grows out of the
# paper into neutral greys only. Cream fur is warm (red minus blue >= 8) and
# bright (>= 236); the greys are neutral and darker, which is the whole basis
# for telling them apart.
GREY_WARMTH = 6      # max red-minus-blue to count as neutral
GREY_MIN = 185       # darker than this is artwork, not halo or shadow
GREY_MAX = 240       # brighter than this is cream, not halo
# Anti-aliased pixels on the outside of an outline are too dark to flood but too
# light to keep, and left opaque they ring the dog in a pale halo that only
# shows once he is on the app's near-black background. The two rings just
# outside the flooded region fade across this range.
FEATHER_DEPTH = 2
FEATHER_LO = 30
FEATHER_HI = 130
# Threshold for finding the artwork's bounding box -- generous, so the halo and
# shadow are treated as background here too.
BBOX_THRESHOLD = 50
SENTINEL = (255, 0, 255)

# --- Eyes -------------------------------------------------------------------
#
# mascot.md §6 says a raster pose has no named parts, so eyes cannot move
# without redrawing it as vector. That is true of the *drawn* file and not of
# what we can cut out of it: the pupil is a closed dark oval on a pale socket,
# so lifting it onto its own layer costs no redrawing at all. The socket is
# filled with sclera and the pupil rides inside it, clipped to the eye opening
# -- the same construction a Lottie rig uses, minus the runtime.
#
# Geometry is in the *padded crop's* coordinates, i.e. the pixels of
# `pose-resting.png`, measured off a luminance map of that file rather than
# eyeballed. It is stable while the artwork's bounding box on the sheet is;
# redraw the head and these four ellipses have to be measured again. Nothing
# checks that for you -- a stale opening shows up as sclera leaking onto fur.
#
#          name    opening (cx, cy, rx, ry)      pupil (cx, cy, rx, ry)
EYES = {
    "pose-resting": [
        ("near", (155.0, 96.5, 8.8, 7.9), (157.3, 96.5, 6.9, 8.6)),
        ("far",  (196.5, 84.8, 6.4, 6.0), (197.6, 84.8, 5.2, 6.4)),
    ],
}
# Sampled out of the crescent the artwork already draws beside each pupil, top
# and bottom of it, so the fill carries the same slight shading. Flat cream
# reads as a doll's eye at 150px.
SCLERA_TOP = (250, 238, 221)
SCLERA_BOTTOM = (234, 224, 210)
# Softness of each ellipse's rim, as a fraction of its radius. The socket fill
# wants a wider fade than the pupil: it meets hand-drawn fur, where a hard edge
# is obvious, while the pupil meets the sclera it was cut from.
SOCKET_FEATHER = 0.22
PUPIL_FEATHER = 0.16
# How far past the opening the pupil may travel before it stops looking like an
# eye and starts looking like a fault, in crop pixels. The pupil nearly fills
# the opening in this drawing, so the honest range is small; see mascot.md §6.1.
TRAVEL_SLACK = 1.2
EYES_GEOMETRY = Path("src/lib/mascot/eyes.ts")


def tight_bbox(dist, x0, x1, y0, y1, threshold):
    """Bounding box of pixels further than `threshold` from the paper colour."""
    sub = dist[y0:y1 + 1, x0:x1 + 1] > threshold
    ys = np.where(sub.any(axis=1))[0]
    xs = np.where(sub.any(axis=0))[0]
    if not len(ys) or not len(xs):
        raise SystemExit(f"empty cell at x={x0}..{x1} y={y0}..{y1}")
    return x0 + xs[0], x0 + xs[-1], y0 + ys[0], y0 + ys[-1]


def background_mask(crop):
    """Pixels reachable from the crop's border without crossing an outline."""
    scratch = Image.fromarray(crop)
    h, w, _ = crop.shape
    seeds = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1),
             (w // 2, 0), (w // 2, h - 1), (0, h // 2), (w - 1, h // 2)]
    for seed in seeds:
        ImageDraw.floodfill(scratch, seed, SENTINEL, thresh=FLOOD_TOLERANCE)
    return (np.asarray(scratch) == np.array(SENTINEL, dtype=np.uint8)).all(axis=2)


def grow_into_greys(bg, crop):
    """Extend the paper region through neutral greys -- halo and floor shadow."""
    rgb = crop.astype(np.int16)
    lum = rgb.mean(axis=2)
    warmth = rgb[:, :, 0] - rgb[:, :, 2]
    grey = (warmth <= GREY_WARMTH) & (lum >= GREY_MIN) & (lum <= GREY_MAX)

    out = bg.copy()
    while True:
        grown = out.copy()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            grown |= np.roll(out, (dy, dx), (0, 1)) & grey
        if grown.sum() == out.sum():
            return out
        out = grown


def knock_out(crop, dist):
    """RGBA copy of `crop` with the paper background made transparent."""
    bg = grow_into_greys(background_mask(crop), crop)
    alpha = np.where(bg, 0.0, 1.0)

    # Soften the kept pixels that touch the flooded region, so the cut edge
    # does not keep a bright fringe of half-paper pixels.
    edge = bg.copy()
    for _ in range(FEATHER_DEPTH):
        grown = edge.copy()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            grown |= np.roll(edge, (dy, dx), (0, 1))
        edge = grown
    edge &= ~bg
    fade = np.clip((dist - FEATHER_LO) / (FEATHER_HI - FEATHER_LO), 0.0, 1.0)
    alpha = np.where(edge, np.minimum(alpha, fade), alpha)

    return np.dstack([crop, (alpha * 255).round().astype(np.uint8)])


def _rim(shape, ellipse, feather):
    """Coverage of an ellipse over a pixel grid, 1 inside, fading at the rim."""
    cx, cy, rx, ry = ellipse
    ys, xs = np.mgrid[0:shape[0], 0:shape[1]]
    t = np.hypot((xs + 0.5 - cx) / rx, (ys + 0.5 - cy) / ry)
    return np.clip((1.0 - t) / feather, 0.0, 1.0)


def split_eyes(cut, eyes):
    """Lift each pupil onto its own layer, leaving a filled socket behind.

    Returns the eyeless pose and one sprite per eye, both RGBA, plus the
    geometry the frontend needs to put them back.
    """
    h, w, _ = cut.shape
    base = cut.astype(np.float32).copy()
    sprites, geometry = [], []

    for name, opening, pupil in eyes:
        socket = np.maximum(_rim((h, w), opening, SOCKET_FEATHER),
                            _rim((h, w), pupil, PUPIL_FEATHER))
        # The pupil overhangs the opening at the top in this pose. Filling the
        # union rather than the opening alone is what stops the overhang being
        # left behind as a black eyelash once the pupil moves off it.
        _, cy, _, ry = opening
        ramp = np.clip((np.arange(h) - (cy - ry)) / (2 * ry), 0.0, 1.0)[:, None]
        fill = np.dstack([np.full((h, w), lo, np.float32) +
                          (hi - lo) * ramp for lo, hi in zip(SCLERA_TOP, SCLERA_BOTTOM)])
        m = socket[:, :, None]
        base[:, :, :3] = base[:, :, :3] * (1 - m) + fill * m
        base[:, :, 3] = np.maximum(base[:, :, 3], socket * 255)

        pcx, pcy, prx, pry = pupil
        x0, y0 = int(pcx - prx) - 2, int(pcy - pry) - 2
        x1, y1 = int(np.ceil(pcx + prx)) + 3, int(np.ceil(pcy + pry)) + 3
        sprite = cut[y0:y1, x0:x1].astype(np.float32).copy()
        alpha = _rim(sprite.shape[:2], (pcx - x0, pcy - y0, prx, pry), PUPIL_FEATHER)
        sprite[:, :, 3] *= alpha
        sprites.append((name, sprite.round().astype(np.uint8)))

        ocx, ocy, orx, ory = opening
        sw, sh = x1 - x0, y1 - y0
        geometry.append({
            "name": name,
            # Percentages of the pose's own box, so one set of numbers holds at
            # every rendered height.
            "left": 100 * x0 / w, "top": 100 * y0 / h,
            "width": 100 * sw / w, "height": 100 * sh / h,
            "clipCx": 100 * ocx / w, "clipCy": 100 * ocy / h,
            "clipRx": 100 * orx / w, "clipRy": 100 * ory / h,
            # Travel is a percentage of the *sprite*, which is what a CSS
            # translate() of a percentage is measured against.
            "travelX": 100 * max(0.0, orx - prx + TRAVEL_SLACK) / sw,
            "travelY": 100 * max(0.0, ory - pry + TRAVEL_SLACK * 0.5) / sh,
        })

    return base.round().astype(np.uint8), sprites, geometry


def write_eye_geometry(path, poses):
    """Emit the eye geometry as a module the frontend imports."""
    lines = [
        "// Generated by scripts/extract-mascot.py -- do not hand-edit.",
        "//",
        "// Where each pupil sprite sits on its pose and how far it may travel,",
        "// all as percentages so they survive any rendered height (mascot.md §6.1).",
        "",
        "export interface Eye {",
        "  /** Sprite box, as a percentage of the pose's box. */",
        "  left: number;",
        "  top: number;",
        "  width: number;",
        "  height: number;",
        "  /** The eye opening the pupil is clipped to, same units. */",
        "  clipCx: number;",
        "  clipCy: number;",
        "  clipRx: number;",
        "  clipRy: number;",
        "  /** Travel before the pupil touches the lid, as a percentage of the sprite. */",
        "  travelX: number;",
        "  travelY: number;",
        "}",
        "",
    ]
    for pose, geometry in poses.items():
        const = pose.replace("pose-", "").upper() + "_EYES"
        lines.append(f"export const {const}: Eye[] = [")
        for eye in geometry:
            fields = ", ".join(f"{k}: {v:.3f}" for k, v in eye.items() if k != "name")
            lines.append(f"  /* {eye['name']} */ {{ {fields} }},")
        lines.append("];")
        lines.append("")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{path}")


def write_icon_source(mark, path):
    """The app mark on a square transparent canvas, for `npm run icon`."""
    scale = ICON_SIZE / max(mark.width, mark.height)
    size = (round(mark.width * scale), round(mark.height * scale))
    canvas = Image.new("RGBA", (ICON_SIZE, ICON_SIZE), (0, 0, 0, 0))
    canvas.alpha_composite(
        mark.resize(size, Image.LANCZOS),
        ((ICON_SIZE - size[0]) // 2, (ICON_SIZE - size[1]) // 2),
    )
    canvas.save(path)
    print(f"{path}  {ICON_SIZE}x{ICON_SIZE}")


def cut_eyes(root):
    """Split the poses in EYES into an eyeless base plus pupil sprites.

    Deliberately reads the pose out of `src/lib/mascot/` rather than re-cutting
    it from the sheet, and is a separate run (`--eyes`) for the same reason: the
    sprites have to line up with the exact file they will be layered over, to
    the pixel. Deriving both from one sheet pass would look equivalent and would
    quietly stop being true the moment the two runs disagree.
    """
    app_dir = root / APP_DIR
    poses = {}
    for pose, eyes in EYES.items():
        stem = APP_ASSETS[pose]
        cut = np.asarray(Image.open(app_dir / f"{stem}.png").convert("RGBA"))
        eyeless, sprites, geometry = split_eyes(cut, eyes)
        Image.fromarray(eyeless, "RGBA").save(app_dir / f"{stem}-eyeless.png")
        print(f"{stem}-eyeless.png  {eyeless.shape[1]}x{eyeless.shape[0]}")
        for eye, sprite in sprites:
            Image.fromarray(sprite, "RGBA").save(app_dir / f"{stem}-pupil-{eye}.png")
            print(f"{stem}-pupil-{eye}.png  {sprite.shape[1]}x{sprite.shape[0]}")
        poses[pose] = geometry
    write_eye_geometry(root / EYES_GEOMETRY, poses)


def main():
    root = Path(__file__).resolve().parent.parent
    sheet = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "docs/mascot/corgit.png"
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "docs/mascot"
    out.mkdir(parents=True, exist_ok=True)
    app_dir = root / APP_DIR
    app_dir.mkdir(parents=True, exist_ok=True)

    rgb = np.asarray(Image.open(sheet).convert("RGB")).astype(np.int16)
    h, w, _ = rgb.shape
    paper = rgb[2, 2]
    dist = np.abs(rgb - paper).sum(axis=2)

    for name, cx0, cx1, cy0, cy1, alpha in CELLS:
        x0, x1, y0, y1 = tight_bbox(dist, cx0, cx1, cy0, cy1, BBOX_THRESHOLD)
        x0, y0 = max(0, x0 - PAD), max(0, y0 - PAD)
        x1, y1 = min(w - 1, x1 + PAD), min(h - 1, y1 + PAD)

        crop = rgb[y0:y1 + 1, x0:x1 + 1].astype(np.uint8)
        Image.fromarray(crop).save(out / f"{name}.png")
        print(f"{name}.png  {x1-x0+1}x{y1-y0+1}  from ({x0},{y0})")

        if not alpha:
            continue
        d = dist[y0:y1 + 1, x0:x1 + 1].astype(np.float32)
        cut = Image.fromarray(knock_out(crop, d), "RGBA")
        cut.save(out / f"{name}-alpha.png")

        if name in APP_ASSETS:
            cut.save(app_dir / f"{APP_ASSETS[name]}.png")
        if name == "app-mark":
            write_icon_source(cut, root / ICON_SOURCE)


if __name__ == "__main__":
    if "--eyes" in sys.argv[1:]:
        cut_eyes(Path(__file__).resolve().parent.parent)
    else:
        main()
