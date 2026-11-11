#!/usr/bin/env python3
"""Synthetic support suite catalogue: renders one image per case and rewrites docs/support-reader-tests.md.

usage:
  synthetic-catalogue.py [--dump DIR] [--roundtrip FILE] [--keep-dump DIR] [--cases ID,ID,...]

  --dump DIR       read an existing dump (DIR/<variant>.json, written by the suite with SYNTHETIC_DUMP=DIR)
                   instead of running the suite
  --keep-dump DIR  run the suite into DIR and keep it (default: a temporary directory, removed afterwards)
  --roundtrip FILE DragonFruit round-trip results for the stages that need DragonFruit, as JSON:
                   {"<variant id>": {"3 build": true, "4a clips": true, "4b loops": false}, ...}
                   Variants missing from it show as not run.
  --cases          only render these cases' images (the page is still written for every case)
  --no-images      write the page only

Without --dump it runs, from backend/:
  SYNTHETIC_DUMP=<dir> cargo test -p findamodel --test support_synthetic every_synthetic_variant_reads_as_before

Each dump file holds the case's model and support triangles, the nominal build's triangles, the truth, the reader's
reading and layout, the stages and the stage 1, 2 and 4 results. Images go to docs/images/support-reader/<case>.png:
the base variant (8 sides, no rotation, separate shells; the first variant where a case has no 8-sided one) seen
from the front and from the side, the original supports beside the reader's nominal build coloured by part, with
the model cut away (only its far faces drawn) so supports under or inside it stay visible.

Needs numpy and Pillow only.
"""
import argparse, json, math, os, re, shutil, subprocess, sys, tempfile
from collections import OrderedDict

import numpy as np
from PIL import Image, ImageDraw, ImageFont

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BACKEND = os.path.join(REPO, "backend")
SUITE = os.path.join(BACKEND, "tests", "support_synthetic")
DOCS = os.path.join(REPO, "docs")
PAGE = os.path.join(DOCS, "support-reader-tests.md")
IMG_DIR = os.path.join(DOCS, "images", "support-reader")
IMG_REL = "images/support-reader"

CARGO_STAGES = ["validate", "1 reader labels", "2 layout", "4a clips (nominal)", "4b loops (nominal)"]
CARGO_SHORT = {"validate": "V", "1 reader labels": "1", "2 layout": "2", "4a clips (nominal)": "4a", "4b loops (nominal)": "4b"}
DF_STAGES = ["3 build", "4a clips", "4b loops"]
DF_SHORT = {"3 build": "3", "4a clips": "4a", "4b loops": "4b"}

# Image: four panels (original front, rebuilt front, original side, rebuilt side).
PANEL_W, PANEL_H = 240, 300
VIEWS = (("front", -25.0), ("side", 65.0))
ELEVATION = 15.0
BG = (24, 24, 28)
COLOURS = OrderedDict(
    [
        ("model", (120, 120, 128)),
        ("original", (214, 178, 120)),
        ("tip", (70, 200, 90)),
        ("trunk", (70, 130, 235)),
        ("branch", (130, 180, 255)),
        ("strut", (170, 120, 240)),
        ("post", (40, 200, 210)),
        ("leg", (230, 70, 60)),
        ("brace", (245, 140, 40)),
        ("root", (60, 90, 150)),
        ("knot", (235, 90, 150)),
    ]
)
LEGEND = [
    ("model", "model (cut away: only its far faces are drawn, under the supports)"),
    ("original", "original supports"),
    ("tip", "rebuilt tip cones"),
    ("trunk", "trunks (shaft and joints of a support standing on the plate)"),
    ("branch", "branches (a support hanging from another)"),
    ("strut", "struts and micro supports (model to model)"),
    ("post", "posts"),
    ("leg", "legs (DragonFruit kickstands)"),
    ("brace", "braces and their end knots"),
    ("root", "roots"),
    ("knot", "knots where a branch or leg meets its host"),
]


def log(msg):
    print(msg, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- suite run and dump


def run_suite(dump_dir):
    env = dict(os.environ, SYNTHETIC_DUMP=os.path.abspath(dump_dir))
    cmd = ["cargo", "test", "-p", "findamodel", "--test", "support_synthetic", "every_synthetic_variant_reads_as_before"]
    log(f"running {' '.join(cmd)} (SYNTHETIC_DUMP={dump_dir})")
    r = subprocess.run(cmd, cwd=BACKEND, env=env)
    if r.returncode != 0:
        log("the suite failed (a stage changed against expected.json); the catalogue shows the current results")


def variant_ids():
    with open(os.path.join(SUITE, "variants.txt")) as f:
        return [l.strip() for l in f if l.strip()]


def split_id(vid):
    case, rest = vid.split("__", 1)
    return case, rest


def base_variant(ids):
    """The harness's base variant: n8_r0_sep, else the first listed."""
    base = OrderedDict()
    for vid in ids:
        case, rest = split_id(vid)
        if case not in base or rest == "n8_r0_sep":
            base[case] = vid
    return base


def case_defs():
    """(id -> styles) from cases.rs: the styles a case is generated in."""
    src = open(os.path.join(SUITE, "cases.rs")).read()
    out = {}
    for m in re.finditer(r'add\(\s*"([a-z0-9_]+)",\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)",\s*(ALL|SEP|SJ)', src):
        out[m.group(1)] = {"ALL": ["sep", "joined", "welded"], "SEP": ["sep"], "SJ": ["sep", "joined"]}[m.group(4)]
    return out


# ---------------------------------------------------------------- nominal build labels


def frustum_tris(a, b):
    return 0 if np.linalg.norm(np.asarray(b, float) - np.asarray(a, float)) < 1e-9 else 128


BALL_TRIS, ROOT_TRIS = 960, 192


def nominal_labels(lay, n_tris):
    """The part of each nominal build triangle, by replaying the order nominal.rs meshes a layout in (32 sides: a
    frustum 128 triangles, a ball 960, a root 192). None when the count does not come out to the build's."""
    labels = []

    def add(k, part):
        labels.extend([part] * k)

    sups, posts = lay.get("supports", []), lay.get("posts", [])
    lines = []
    for s in sups:
        b = s["base"]
        line = []
        if isinstance(b, dict) and "plate" in b:
            line.append([b["plate"][0], b["plate"][1], 2.0])
        elif isinstance(b, dict) and "at" in b:
            line.append(b["at"])
        elif s.get("lower"):
            line.append(s["lower"]["socket"])
        line += s.get("joints", [])
        line.append(s["tip"]["socket"])
        lines.append(line)
    for p in posts:
        lines.append([p["foot"]] + p.get("joints", []) + [p["top"]])

    def pillar(line, part):
        for w in zip(line, line[1:]):
            add(frustum_tris(*w), part)
        add(BALL_TRIS * max(len(line) - 2, 0), part)

    for i, s in enumerate(sups):
        b = s["base"]
        if isinstance(b, dict) and "plate" in b:
            role = "trunk"
        elif isinstance(b, dict) and "at" in b:
            role = "branch"
        else:
            role = "strut"
        pillar(lines[i], role)
        if role == "trunk":
            add(ROOT_TRIS, "root")
        elif role == "branch":
            add(BALL_TRIS, "knot")
        if s.get("written") == "twig":
            # A twig's ends: per end a disk (a frustum and its capping ball) and the joint off it.
            add(2 * (128 + 2 * BALL_TRIS), "tip")
            continue
        for tip in [s["tip"]] + ([s["lower"]] if s.get("lower") else []):
            sock, contact = np.asarray(tip["socket"], float), np.asarray(tip["contact"], float)
            u = contact - sock
            u = u / max(np.linalg.norm(u), 1e-300)
            add(frustum_tris(sock, contact + u * 0.05), "tip")
    for j, p in enumerate(posts):
        pillar(lines[len(sups) + j], "post")
        add(ROOT_TRIS, "root")
    for b in lay.get("braces", []):
        add(frustum_tris(b["lower"][2], b["upper"][2]), "brace")
        add(2 * BALL_TRIS, "brace")
    for k in lay.get("kickstands", []):
        pillar([k["foot"]] + k.get("joints", []) + [k["knot"]], "leg")
        add(ROOT_TRIS, "root")
        add(BALL_TRIS, "knot")
    return labels if len(labels) == n_tris else None


# ---------------------------------------------------------------- rendering


def camera(az_deg, el_deg):
    az, el = math.radians(az_deg), math.radians(el_deg)
    f = np.array([math.sin(az) * math.cos(el), math.cos(az) * math.cos(el), -math.sin(el)])
    r = np.cross(f, [0.0, 0.0, 1.0])
    r /= np.linalg.norm(r)
    u = np.cross(r, f)
    return np.array([r, u, f])


def rasterise(tris, cols, w, h):
    """Orthographic z-buffer: tris (N, 3, 3) in screen pixels (x right, y down, z depth, larger is farther), cols
    (N, 3) uint8. Returns the image array (h, w, 3) and a mask of covered pixels."""
    img = np.zeros((h, w, 3), np.uint8)
    img[:] = BG
    mask = np.zeros((h, w), bool)
    if len(tris) == 0:
        return img, mask
    tris = np.asarray(tris, float)
    x, y, z = tris[..., 0], tris[..., 1], tris[..., 2]
    x0 = np.floor(x.min(1)).astype(np.int64)
    y0 = np.floor(y.min(1)).astype(np.int64)
    x1 = np.ceil(x.max(1)).astype(np.int64)
    y1 = np.ceil(y.max(1)).astype(np.int64)
    keep = (x1 >= 0) & (y1 >= 0) & (x0 < w) & (y0 < h)
    x0, y0 = np.maximum(x0, 0), np.maximum(y0, 0)
    x1, y1 = np.minimum(x1, w - 1), np.minimum(y1, h - 1)
    size = np.maximum(x1 - x0, y1 - y0) + 1
    ax, ay, bx, by, cx, cy = x[:, 0], y[:, 0], x[:, 1], y[:, 1], x[:, 2], y[:, 2]
    area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
    keep &= np.abs(area) > 1e-12
    pix_all, dep_all, tri_all = [], [], []
    k = 1
    while True:
        sel = np.nonzero(keep & (size <= k) & (size > k // 2 if k > 1 else True))[0]
        if len(sel):
            oy, ox = np.mgrid[0:k, 0:k]
            ox, oy = ox.ravel(), oy.ravel()
            step = max(1, 4_000_000 // (k * k))
            for s0 in range(0, len(sel), step):
                t = sel[s0 : s0 + step]
                px = x0[t][:, None] + ox[None, :]
                py = y0[t][:, None] + oy[None, :]
                sx, sy = px + 0.5, py + 0.5
                w0 = (bx[t, None] - sx) * (cy[t, None] - sy) - (by[t, None] - sy) * (cx[t, None] - sx)
                w1 = (cx[t, None] - sx) * (ay[t, None] - sy) - (cy[t, None] - sy) * (ax[t, None] - sx)
                w2 = (ax[t, None] - sx) * (by[t, None] - sy) - (ay[t, None] - sy) * (bx[t, None] - sx)
                sg = np.sign(area[t])[:, None]
                inside = (w0 * sg >= 0) & (w1 * sg >= 0) & (w2 * sg >= 0) & (px <= x1[t][:, None]) & (py <= y1[t][:, None])
                if not inside.any():
                    continue
                a_ = area[t][:, None]
                d = (w0 * z[t, 0, None] + w1 * z[t, 1, None] + w2 * z[t, 2, None]) / a_
                ii, jj = np.nonzero(inside)
                pix_all.append(py[ii, jj] * w + px[ii, jj])
                dep_all.append(d[ii, jj])
                tri_all.append(t[ii])
        if k >= size[keep].max(initial=1):
            break
        k *= 2
    if not pix_all:
        return img, mask
    pix, dep, tri = np.concatenate(pix_all), np.concatenate(dep_all), np.concatenate(tri_all)
    order = np.lexsort((dep, pix))
    pix, tri = pix[order], tri[order]
    first = np.ones(len(pix), bool)
    first[1:] = pix[1:] != pix[:-1]
    flat = img.reshape(-1, 3)
    flat[pix[first]] = cols[tri[first]]
    mask.reshape(-1)[pix[first]] = True
    return img, mask


def shade(tris, base, m):
    """Per triangle colour: the base colour (one, or one per triangle) lit by a light over the viewer's left shoulder (two-sided)."""
    n = np.cross(tris[:, 1] - tris[:, 0], tris[:, 2] - tris[:, 0])
    n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-300)
    light = -m[2] + 0.6 * m[1] - 0.4 * m[0]
    light /= np.linalg.norm(light)
    lam = np.abs(n @ light)
    lam = np.round(lam * 5) / 5  # few shades: small palette, small PNG
    base = np.broadcast_to(np.asarray(base, float).reshape(-1, 3), (len(tris), 3))
    return np.clip(base * (0.35 + 0.65 * lam[:, None]), 0, 255).astype(np.uint8)


def render_case(d, out_path):
    model = np.asarray(d["model"], float).reshape(-1, 3, 3)
    orig = np.asarray(d["support"], float).reshape(-1, 3, 3)
    nom = np.asarray(d["nominal"], float).reshape(-1, 3, 3)
    labels = nominal_labels(d["layout"], len(nom)) if len(nom) else []
    if labels is None:
        log(f"{d['id']}: nominal build order not recognised, build drawn in one colour")
        labels = ["trunk"] * len(nom)
    nom_base = np.array([COLOURS[l] for l in labels], float).reshape(-1, 3)
    pts = np.concatenate([orig.reshape(-1, 3), nom.reshape(-1, 3)]) if len(nom) else orig.reshape(-1, 3)
    font = ImageFont.load_default(size=12)
    img = Image.new("RGB", (PANEL_W * 4, PANEL_H), BG)
    draw = ImageDraw.Draw(img)
    for vi, (vname, az) in enumerate(VIEWS):
        m = camera(az, ELEVATION)
        sp = pts @ m.T
        lo, hi = sp.min(0), sp.max(0)
        pad = 0.08 * (hi - lo) + 1.5
        lo, hi = lo - pad, hi + pad
        centre = (lo + hi) / 2
        margin = 14
        top = 18
        scale = min((PANEL_W - 2 * margin) / max(hi[0] - lo[0], 1e-6), (PANEL_H - top - 2 * margin) / max(hi[1] - lo[1], 1e-6))
        scale = min(scale, 60.0)
        # The model's far faces only (a cut-away), drawn under the supports so they always show.
        n = np.cross(model[:, 1] - model[:, 0], model[:, 2] - model[:, 0])
        mshow = model[(n @ m[2]) > 0]

        def to_screen(t):
            s = t @ m.T
            out = np.empty_like(s)
            out[..., 0] = PANEL_W / 2 + (s[..., 0] - centre[0]) * scale
            out[..., 1] = top + (PANEL_H - top) / 2 - (s[..., 1] - centre[1]) * scale
            out[..., 2] = s[..., 2]
            return out

        marr, mmask = rasterise(to_screen(mshow), shade(mshow, COLOURS["model"], m), PANEL_W, PANEL_H) if len(mshow) else rasterise([], None, PANEL_W, PANEL_H)
        for pi, (sup, cols, title) in enumerate(
            (
                (orig, shade(orig, COLOURS["original"], m), f"original, {vname}"),
                (nom, shade(nom, nom_base, m) if len(nom) else None, f"nominal build, {vname}"),
            )
        ):
            sarr, smask = rasterise(to_screen(sup) if len(sup) else [], cols, PANEL_W, PANEL_H)
            arr = np.where(smask[..., None], sarr, marr)
            x = (vi * 2 + pi) * PANEL_W
            img.paste(Image.fromarray(arr), (x, 0))
            draw.text((x + 6, 3), title, fill=(220, 220, 225), font=font)
            draw.line([(x, 0), (x, PANEL_H)], fill=(60, 60, 66))
    img.quantize(colors=48, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(out_path, optimize=True)


# ---------------------------------------------------------------- page text


def fmt_mm(x):
    return f"{x:g}"


def describe_shapes(shapes):
    out = []
    for s in shapes:
        k = s["kind"]
        if k == "box":
            lo, hi = s["lo"], s["hi"]
            size = " x ".join(fmt_mm(round(hi[i] - lo[i], 3)) for i in range(3))
            out.append(f"a {size} mm block from z {fmt_mm(round(lo[2], 3))} mm")
        elif k == "hollow":
            lo, hi = s["lo"], s["hi"]
            size = " x ".join(fmt_mm(round(hi[i] - lo[i], 3)) for i in range(3))
            out.append(f"a hollow {size} mm shell (cavity from z {fmt_mm(s['ilo'][2])} to {fmt_mm(s['ihi'][2])} mm)")
        elif k == "obox":
            half = s["half"]
            size = " x ".join(fmt_mm(round(2 * h, 3)) for h in half)
            out.append(f"a turned {size} mm block centred at z {fmt_mm(round(s['c'][2], 3))} mm")
        elif k == "sphere":
            out.append(f"a {fmt_mm(2 * s['r'])} mm ball centred at z {fmt_mm(s['c'][2])} mm")
        else:
            out.append(f"a {k} shape")
    return "; ".join(out)


COUNT_NAMES = [("trunks", "trunk"), ("branches", "branch"), ("struts", "strut"), ("posts", "post"), ("kickstands", "leg"), ("braces", "brace")]


def describe_counts(c):
    parts = []
    for key, one in COUNT_NAMES:
        n = c.get(key, 0)
        if n:
            many = {"branch": "branches"}.get(one, one + "s")
            parts.append(f"{n} {one if n == 1 else many}")
    return ", ".join(parts) or "no supports"


TIP_KINDS = {
    "bare": "blunt rod end (no cone)",
    "dome": "rounded rod end",
    "ball": "cone ending in a separate ball",
    "weldball": "cone with a ball welded on",
}
STYLE_TEXT = {"sep": "separate shells", "joined": "joined (shells welded at shared cap centres)", "welded": "welded (one unioned shell)"}


def generates(t, styles):
    lines = [f"Model: {describe_shapes(t['shapes'])}."]
    tips = t["tips"]
    kinds = {}
    for x in tips:
        kinds[x["kind"]] = kinds.get(x["kind"], 0) + 1
    tip_text = f"{len(tips)} contact{'s' if len(tips) != 1 else ''}"
    extra = [f"{n} {TIP_KINDS.get(k, k)}" for k, n in kinds.items() if k != "flat"]
    if extra:
        tip_text += " (" + ", ".join(extra) + ")"
    raft = any(p["type"] == "raft" for p in t["parts"])
    lines.append(f"Supports: {describe_counts(t['counts'])}; {tip_text}{'; on a raft' if raft else ''}.")
    stepped = [s["name"] for s in t["layout"]["supports"] if s.get("seg_d")] + [p["name"] for p in t["layout"]["posts"] if p.get("seg_d")] + [k["name"] for k in t["layout"]["kickstands"] if k.get("seg_d")]
    lines.append("Styles: " + ", ".join(STYLE_TEXT[s] for s in styles) + "; side counts 8, 12, 16, 32 and the scene turned 17, 45 and 30 degrees.")
    return lines, stepped


def checks(t, stepped):
    out = []
    labelled = {}
    for p in t["parts"]:
        for e in p.get("expect") or []:
            labelled[e] = labelled.get(e, 0) + 1
    if labelled:
        out.append("1 reader labels: " + ", ".join(f"{n} {k.replace('_', ' ')}" for k, n in sorted(labelled.items())) + " part(s) read with the right axis, width and label.")
    want = describe_counts(t["counts"])
    s2 = f"2 layout: {want}, with each tip's contact, socket and width, each shaft's width, foot or parent and line, and each brace's ends."
    out.append(s2)
    if t.get("counts_alt"):
        out.append("Also accepted counts: " + "; ".join(describe_counts(c) for c in t["counts_alt"]) + " (then widths and lines are not checked).")
    if t.get("flex"):
        out.append(f"Flex: up to {t['flex']} more posts, legs or braces than the truth are accepted.")
    if t.get("tree_free"):
        out.append("Tree free: the counts are checked but not which support stands where (widths and lines are not checked).")
    if stepped:
        out.append("Stepped widths: " + ", ".join(stepped) + " must be written in the truth's number of constant-width steps, each within the width tolerance.")
    if t.get("passes_through"):
        out.append("Passes through: the original itself runs through the model, so the truth's own clip is allowed in 4a.")
    if t.get("model_open"):
        out.append("Open model: the model mesh is not closed.")
    out.append(f"4a clips and 4b loops on the nominal build: nothing but tips inside the model, and {len(t['tips'])} contact loop(s) at the truth's place and size.")
    return out


def first_errors(d, limit=3):
    errs = []
    if not d["stages"]["validate"]:
        errs += [f"validate: {e}" for e in d.get("gen_errors", [])]
    for key, res in (("1", d.get("s1")), ("2", d.get("s2")), ("4", d.get("s4"))):
        if res and not res.get("pass", True):
            errs += [f"{key}: {e}" for e in res.get("errors", [])]
    return errs[:limit]


def mark(v):
    return "-" if v is None else ("pass" if v else "**fail**")


def md_escape(s):
    return s.replace("|", "\\|")


def write_page(cases, df):
    n_var = sum(len(c["variants"]) for c in cases)
    tot = {s: sum(1 for c in cases for v in c["variants"].values() if v.get(s)) for s in CARGO_STAGES}
    all_var = sum(1 for c in cases for v in c["variants"].values() if all(v.get(s) for s in CARGO_STAGES))
    base_all = sum(1 for c in cases if all(c["base_stages"].get(s) for s in CARGO_STAGES))
    case_all = sum(1 for c in cases if c["all_pass"] == len(c["variants"]))
    df_note = "filled from the round-trip results given" if df else "empty (no round-trip results were given)"
    L = []
    L += [
        "---",
        "layout: default",
        "title: Support reader test cases",
        "nav_exclude: true",
        "---",
        "",
        "# Support reader test cases",
        "",
        "<!-- Written by scripts/synthetic-catalogue.py; edit the script, not this page. -->",
        "",
        "Every case of the synthetic support suite (`backend/tests/support_synthetic`), with a picture of its base "
        "variant, what it generates, what it checks and how the [support reader](support-reader.md) does on it. Each "
        "case is a small generated scene with one thing under test; its truth (the parts, tips and layout it was built "
        "from) is known exactly.",
        "",
        "Regenerate this page and its images with `python3 scripts/synthetic-catalogue.py` (it runs the suite with "
        "`SYNTHETIC_DUMP`, renders the images and rewrites this page). `--roundtrip FILE` fills the DragonFruit columns "
        "from a JSON file of round-trip results, `{\"<variant id>\": {\"3 build\": true, \"4a clips\": true, \"4b loops\": true}}`.",
        "",
        "## Reading the results",
        "",
        "- **Variants.** A case runs in up to 11 variants: 8, 12, 16 and 32 sides; the scene turned 17, 45 and 30 "
        "degrees about z; and, where the case allows, joined shells (rods split where another part's end lands on them, "
        "so end caps weld) and welded shells (every support but raft and roots unioned into one shell), at 8 and 16 "
        "sides. The base variant is `n8_r0_sep` (8 sides, no rotation, separate shells).",
        "- **Cargo stages** (`cargo test -p findamodel --test support_synthetic`): V validate (the truth against its own "
        "mesh), 1 reader labels, 2 layout, 4a clips and 4b loops on the nominal build. The nominal build "
        "(`support_synthetic/nominal.rs`) meshes the layout roughly as DragonFruit builds what the VOXL writer writes "
        "(tips placed by the writer's own placement, twigs with their contact disks), clips at tip offset 0 and loops "
        "at DragonFruit's 0.05 mm as on DragonFruit's builds; it is an approximation (no socket joints, rounder "
        "knots), but its 4a and 4b agree with DragonFruit's own build on nearly every variant.",
        f"- **DragonFruit stages** (3 build recall and precision, 4a clips and 4b loops on DragonFruit's own build) come "
        f"from the round trip in `backend/tools/support-reader`; on this page they are {df_note}.",
        "- **Images.** Left to right: the original supports from the front, the reader's nominal build from the front, "
        "then both from the side. The model is cut away: only its far faces are drawn, and always under the supports.",
        "",
        "| Colour | Part |",
        "| --- | --- |",
    ]
    for key, text in LEGEND:
        r, g, b = COLOURS[key]
        L.append(f"| <span style=\"display:inline-block;width:1.4em;height:0.9em;background:rgb({r},{g},{b})\"></span> | {text} |")
    L += [
        "",
        "## Summary",
        "",
        f"{len(cases)} cases, {n_var} variants. All cargo stages pass on {all_var} of {n_var} variants, on {base_all} of "
        f"{len(cases)} base variants, and on every variant of {case_all} cases.",
        "",
        "| Stage | Variants passing |",
        "| --- | --- |",
    ]
    for s in CARGO_STAGES:
        L.append(f"| {s} | {tot[s]} of {n_var} |")
    if df:
        for s in DF_STAGES:
            got = [df[v].get(s) for c in cases for v in c["variants"] if v in df]
            L.append(f"| {s} (DragonFruit) | {sum(1 for x in got if x)} of {len(got)} run |")
    L += [
        "",
        "Base variant per stage (V, 1, 2, 4a, 4b on the nominal build; DragonFruit 3, 4a, 4b), and how many of the case's "
        "variants pass every cargo stage.",
        "",
        "| Case | Title | V | 1 | 2 | 4a | 4b | DF 3 | DF 4a | DF 4b | Variants |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for c in cases:
        bs = c["base_stages"]
        dfb = df.get(c["base"], {}) if df else {}
        row = [f"[{c['id']}](#{c['id']})", md_escape(c["title"])] + [mark(bs.get(s)) for s in CARGO_STAGES] + [mark(dfb.get(s)) for s in DF_STAGES]
        row.append(f"{c['all_pass']} of {len(c['variants'])}")
        L.append("| " + " | ".join(row) + " |")
    L += ["", "## Cases", ""]
    for c in cases:
        t = c["truth"]
        L += [f"<a id=\"{c['id']}\"></a>", "", f"### {c['id']}", "", f"{md_escape(c['title'])}.", ""]
        if c["image"]:
            L += [f"![{c['id']}, base variant {split_id(c['base'])[1]}]({IMG_REL}/{c['id']}.png)", ""]
        if t.get("real"):
            L += [f"Seen in real model {t['real']}.", ""]
        gen, stepped = generates(t, c["styles"])
        L += ["**Generates.** " + " ".join(gen), ""]
        for note in t.get("notes") or []:
            L += [f"Note: {note}", ""]
        L += ["**Checks.**", ""] + [f"- {x}" for x in checks(t, stepped)] + [""]
        bs = c["base_stages"]
        res = ", ".join(f"{s} {mark(bs.get(s))}" for s in CARGO_STAGES)
        L += [f"**Result.** Base variant `{c['base']}`: {res}."]
        if df:
            dfb = df.get(c["base"])
            L[-1] += " DragonFruit: " + (", ".join(f"{s} {mark(dfb.get(s))}" for s in DF_STAGES) if dfb else "not run") + "."
        L[-1] += f" Every cargo stage passes on {c['all_pass']} of {len(c['variants'])} variants."
        L.append("")
        for e in c["base_errors"]:
            L.append(f"- `{md_escape(e)}`")
        if c["base_errors"]:
            L.append("")
        failing = [(v, [CARGO_SHORT[s] for s in CARGO_STAGES if not st.get(s)]) for v, st in c["variants"].items() if not all(st.get(s) for s in CARGO_STAGES)]
        if failing:
            L += ["Failing variants: " + "; ".join(f"`{split_id(v)[1]}` ({', '.join(f)})" for v, f in failing) + ".", ""]
    text = "\n".join(L).rstrip() + "\n"
    assert "\u2014" not in text, "no emdash in the docs"
    with open(PAGE, "w") as f:
        f.write(text)
    log(f"wrote {PAGE}")


# ---------------------------------------------------------------- main


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dump")
    ap.add_argument("--keep-dump")
    ap.add_argument("--roundtrip")
    ap.add_argument("--cases")
    ap.add_argument("--no-images", action="store_true")
    a = ap.parse_args()
    tmp = None
    if a.dump:
        dump = a.dump
    else:
        dump = a.keep_dump or tempfile.mkdtemp(prefix="synthetic-dump-")
        tmp = None if a.keep_dump else dump
        run_suite(dump)
    df = {}
    if a.roundtrip:
        with open(a.roundtrip) as f:
            df = json.load(f)
    only = set(a.cases.split(",")) if a.cases else None
    styles = case_defs()
    ids = variant_ids()
    bases = base_variant(ids)
    os.makedirs(IMG_DIR, exist_ok=True)
    cases = []
    try:
        by_case = OrderedDict()
        for vid in ids:
            by_case.setdefault(split_id(vid)[0], []).append(vid)
        for cid, vids in by_case.items():
            stages = OrderedDict()
            base = None
            for vid in vids:
                p = os.path.join(dump, vid + ".json")
                if not os.path.exists(p):
                    log(f"{vid}: no dump file")
                    stages[vid] = {}
                    continue
                with open(p) as f:
                    d = json.load(f)
                stages[vid] = d["stages"]
                if vid == bases[cid]:
                    base = d
            if base is None:
                log(f"{cid}: base variant missing, skipped")
                continue
            img = os.path.join(IMG_DIR, cid + ".png")
            if not a.no_images and (only is None or cid in only):
                render_case(base, img)
            cases.append(
                {
                    "id": cid,
                    "title": base["title"],
                    "base": bases[cid],
                    "truth": base["truth"],
                    "styles": styles.get(cid) or sorted({split_id(v)[1].rsplit("_", 1)[1] for v in vids}, key=["sep", "joined", "welded"].index),
                    "variants": stages,
                    "base_stages": base["stages"],
                    "base_errors": first_errors(base),
                    "all_pass": sum(1 for s in stages.values() if s and all(s.get(k) for k in CARGO_STAGES)),
                    "image": os.path.exists(img),
                }
            )
            log(f"{cid}: {cases[-1]['all_pass']}/{len(vids)}")
        write_page(cases, df)
    finally:
        if tmp:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
