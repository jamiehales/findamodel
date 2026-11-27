#!/usr/bin/env python3
"""Real-structure support suite catalogue: renders one image per case and rewrites docs/support-reader-real-tests.md.

usage:
  real-support-catalogue.py [--dump DIR] [--keep-dump DIR] [--cases ID,ID,...] [--no-images]

  --dump DIR       read an existing dump (DIR/<case>.json, written by the suite with REAL_DUMP=DIR) instead of
                   running the suite
  --keep-dump DIR  run the suite into DIR and keep it (default: a temporary directory, removed afterwards)
  --cases          only render these cases' images (the page is still written for every case)
  --no-images      write the page only

Without --dump it runs, from backend/:
  REAL_DUMP=<dir> cargo test -p findamodel --test support_real every_real_case_reads_as_before

Images go to docs/images/support-reader-real/<case>.png: the case's expected.png (the original supports and the
expected structure, front and side) with the reader's nominal build, coloured by part, beside it.

Needs numpy and Pillow only.
"""
import argparse, importlib.util, json, os, re, shutil, subprocess, sys, tempfile
from collections import Counter, OrderedDict

import numpy as np
from PIL import Image

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BACKEND = os.path.join(REPO, "backend")
SUITE = os.path.join(BACKEND, "tests", "support_real")
DOCS = os.path.join(REPO, "docs")
PAGE = os.path.join(DOCS, "support-reader-real-tests.md")
IMG_DIR = os.path.join(DOCS, "images", "support-reader-real")
IMG_REL = "images/support-reader-real"

STAGES = ["validate", "1 tips", "1 tip shape", "2 routes", "2 shafts", "2 braces", "2 joints", "4a clips (nominal)", "4b loops (nominal)"]
SHORT = {"validate": "V", "1 tips": "tips", "1 tip shape": "shape", "2 routes": "routes", "2 shafts": "shafts", "2 braces": "braces", "2 joints": "joints",
         "4a clips (nominal)": "4a", "4b loops (nominal)": "4b"}
# Where each stage's errors are in a dump.
ERRORS = {"validate": ("validate",), "1 tips": ("layout_stages", "tips"), "1 tip shape": ("layout_stages", "shape"), "2 routes": ("layout_stages", "routes"),
          "2 shafts": ("layout_stages", "shafts"), "2 braces": ("layout_stages", "braces"), "2 joints": ("layout_stages", "joints")}
GROUPS = OrderedDict([("t", "Trunks"), ("p", "Tips"), ("b", "Branches and kickstands"), ("l", "Mini supports on a host (Lychee leaves)"),
                      ("m", "Supports between two parts of the model"), ("x", "Braces"), ("h", "Supports inside hollows and deep recesses"), ("w", "Shells"), ("e", "Nerikson's Elf Mage"), ("n", "Other creators")])


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def load(name, file):
    spec = importlib.util.spec_from_file_location(name, os.path.join(REPO, "scripts", file))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def run_suite(dump_dir):
    env = dict(os.environ, REAL_DUMP=os.path.abspath(dump_dir))
    cmd = ["cargo", "test", "-p", "findamodel", "--test", "support_real", "every_real_case_reads_as_before"]
    log(f"running {' '.join(cmd)} (REAL_DUMP={dump_dir})")
    r = subprocess.run(cmd, cwd=BACKEND, env=env)
    if r.returncode != 0:
        log("the suite failed (a stage changed against expected.json); the catalogue shows the current results")


def render(cat, cases, cid, dump):
    """The docs image of a case: expected.png with the reader's nominal build beside it."""
    d = os.path.join(SUITE, cid)
    model = cases.read_stl(os.path.join(d, "model.stl")).astype(np.float64)
    sup = cases.read_stl(os.path.join(d, "supports.stl")).astype(np.float64)
    with open(os.path.join(d, "truth.json")) as f:
        truth = json.load(f)
    tm, _ = cases.truth_mesh(truth)
    pts = cases.view_points(model, sup, tm)
    nom = np.asarray(dump["nominal"], float).reshape(-1, 3, 3)
    labels = cat.nominal_labels(dump["layout"], len(nom)) if len(nom) else []
    if labels is None:
        log(f"{cid}: nominal build order not recognised, build drawn in one colour")
        labels = ["trunk"] * len(nom)
    cols = np.array([cat.COLOURS[l] for l in labels], float).reshape(-1, 3)
    tmp = os.path.join(IMG_DIR, cid + ".reader.tmp.png")
    cases.render_panels(cat, model, [("reader", nom, cols)], tmp, pts)
    exp = Image.open(os.path.join(d, "expected.png")).convert("RGB")
    rd = Image.open(tmp).convert("RGB")
    img = Image.new("RGB", (exp.width + rd.width, max(exp.height, rd.height)), cat.BG)
    img.paste(exp, (0, 0))
    img.paste(rd, (exp.width, 0))
    img.quantize(colors=64, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(os.path.join(IMG_DIR, cid + ".png"), optimize=True)
    os.remove(tmp)


def mark(v):
    return "-" if v is None else ("pass" if v else "**fail**")


def md(s):
    return str(s).replace("|", "\\|")


def errors_of(dump, stage):
    if stage in ERRORS:
        x = dump
        for k in ERRORS[stage]:
            x = x.get(k, {})
        return x.get("errors", [])
    errs = dump.get("s4", {}).get("errors", [])
    clip = [e for e in errs if e.startswith("clips")]
    return clip if stage.startswith("4a") else [e for e in errs if not e.startswith("clips")]


def accepted(dump):
    """The accepted readings a case's result used (see the scorer)."""
    ls = dump.get("layout_stages", {})
    b, ro = ls.get("braces", {}), ls.get("routes", {})
    one = lambda n, a, many: f"{n} {a if n == 1 else many}"
    out = []
    if b.get("as_legs"):
        out.append(one(b["as_legs"], "brace into the root zone read as a leg", "braces into the root zone read as legs"))
    if ro.get("own_foot"):
        out.append(one(ro["own_foot"], "branch from inside a trunk's foot read with a foot of its own", "branches from inside a trunk's foot read with feet of their own"))
    if b.get("split"):
        out.append(one(b["split"], "brace through a pillar read in stretches", "braces through pillars read in stretches"))
    if b.get("under_root"):
        out.append(one(b["under_root"], "brace wholly under 2 mm left out", "braces wholly under 2 mm left out"))
    if ls.get("joints", {}).get("on_kickstands"):
        out.append(one(ls["joints"]["on_kickstands"], "branch on a kickstand read as a support up the kickstand's pillar",
                       "branches on kickstands read as supports up the kickstands' pillars"))
    if ro.get("socket_swaps"):
        out.append(one(ro["socket_swaps"], "shaft and brace swapped at a socket", "shafts and braces swapped at their sockets"))
    if b.get("posts_of_missed_tips"):
        out.append(one(b["posts_of_missed_tips"], "post under a missed tip", "posts under missed tips"))
    return out


def counts_text(truth):
    c = truth.get("counts", {})
    names = [("trunk", "trunk", "trunks"), ("branch", "branch", "branches"), ("strut", "support between two parts of the model", "supports between two parts of the model"),
             ("leg", "kickstand", "kickstands"), ("brace", "brace", "braces")]
    parts = [f"{c[k]} {one if c[k] == 1 else many}" for k, one, many in names if c.get(k)]
    minis = sum(1 for s in truth["supports"] if s.get("mini"))
    text = ", ".join(parts) or "no supports"
    text += f"; {len(truth['tips'])} tip{'s' if len(truth['tips']) != 1 else ''}"
    if minis:
        text += f" ({minis} mini support{'s' if minis != 1 else ''} with no cone)"
    return text


def write_page(rows, cat):
    n = len(rows)
    tot = {s: sum(1 for r in rows if r["stages"].get(s)) for s in STAGES}
    reader_stages = STAGES[1:]
    valid = [r for r in rows if r["stages"].get("validate")]
    all_pass = sum(1 for r in valid if all(r["stages"].get(s) for s in reader_stages))
    creators = Counter(r["truth"]["source"]["creator"] for r in rows)
    mesh = sum(r["mesh_bytes"] for r in rows)
    L = [
        "---",
        "layout: default",
        "title: Support reader real-structure test cases",
        "nav_exclude: true",
        "---",
        "",
        "# Support reader real-structure test cases",
        "",
        "<!-- Written by scripts/real-support-catalogue.py; edit the script, not this page. -->",
        "",
        "Every case of the real-structure support suite (`backend/tests/support_real`): support structures cut out of real "
        "pre-supported models whose Lychee project is known, with a picture of what is expected and what the "
        "[support reader](support-reader.md) reads. The [synthetic cases](support-reader-tests.md) generate one thing under "
        "test; these cases hold the structures real creators build.",
        "",
        "A case is one support structure (one connected Lychee tree with its braces) or a self-contained section of one: the "
        "named supports with the hosts under them down to the plate, and only the braces whose both hosts are kept. Its "
        "`supports.stl` holds the original support shells of those supports and the raft under them; its `model.stl` holds "
        "small closed cuts of the model where the supports meet it (boxes up to 1.5 mm round each contact, the cut faces "
        "capped), never the whole model; its `truth.json` holds the Lychee supports in the STL's frame; its `expected.png` "
        "shows the expected result.",
        "",
        "Regenerate this page and its images with `python3 scripts/real-support-catalogue.py` (it runs the suite with "
        "`REAL_DUMP`, renders the images and rewrites this page). The cases themselves are cut by "
        "`scripts/real-support-cases.py` from the case list `backend/tests/support_real/cases.json`, the Lychee truth files "
        "and the split meshes of the source models (see the script's header). Sources are Lychee projects (`.lys`) only: the "
        "few `.ctb` files in the collection are sliced plates, which hold no support structure to take a truth from.",
        "",
        "## Reading the results",
        "",
        "- **validate** checks the truth against its own meshes before any failure is trusted. Every truth contact lies on "
        "the cut model (within 0.15 mm). At least 90% of every truth line (shafts, tip cones, braces) lies inside the support "
        "mesh, and no stretch of a line longer than 1 mm is bare of it. The mesh must be the line's own: a point held "
        "only by shells that are another element's alone (a shell at least 90% explained by a primitive of another "
        "support or brace) does not count, unless the line's whole width there lies inside another primitive (a rod's "
        "end buried in its host). The truth's primitives (shafts, tip cones, braces and "
        "the feet as measured on the mesh) explain the support mesh: at least 90% of it above the root zone, at least 90% of "
        "every connected piece of it down to the plate, and no stretch of it larger than 1 mm is left over (a corner of "
        "the mesh is explained within 0.3 mm of a primitive; the raft kept under a case is the raft's, and stays under "
        "6 mm). The original supports make a contact loop at every truth tip. A case that fails validate is a fixture "
        "fault, to be fixed in `scripts/real-support-cases.py` or the case list, not a reader problem.",
        "- **1 tips**: every truth tip has a reader tip with its contact within 0.3 mm (one to one, nearest first); no reader "
        "tip is left over. Two Lychee supports ending at one point (within 0.05 mm) are one contact, answered by one reader tip.",
        "- **1 tip shape**: the reader's contact width is 0.7 to 1.5 times Lychee's point diameter (the reader sizes the loop "
        "the cone really makes, which is wider than the nominal point on a slope), its direction is within 15 degrees of "
        "Lychee's axis (taken from the contact to the socket, or along the support to 0.5 mm from the contact where the "
        "socket is nearer: a hair support's sockets sit a few hundredths of a millimetre from its contacts), and on tips with a cone the body width is within 0.75 to 1.33 times Lychee's and the socket within 1 mm.",
        "- **2 routes**: each tip's line from the contact down to the plate (or to the model) is covered: 90% of each "
        "side's line within 0.5 mm of the other's, above 2 mm (roots and rafts are drawn differently by design). Which of "
        "two supports sharing a shaft is the trunk and which the branch is the author's choice in Lychee, so the same line "
        "under another base passes; where the lines are too short to compare, the base (plate, another support, or the "
        "model) must be the same. A support between two parts of the model is followed on both sides to its other contact. "
        "A reader support on the plate stands upright in its root: its line is drawn from its foot straight up to 2 mm and "
        "on from there, as DragonFruit builds it.",
        "- **2 shafts**: at least 90% of Lychee's shaft length has a reader shaft within 0.3 mm, and at least 90% of the "
        "reader's shaft length (supports, posts and legs) lies on a Lychee shaft.",
        "- **2 braces**: every Lychee brace has a reader brace with both ends within 1 mm, no reader brace is left over, the "
        "reader has no posts, every Lychee kickstand is read as one leg with the same knee and no other leg is left over "
        "(see the vocabulary and the accepted readings below).",
        "- **2 joints**: no support has more bends than Lychee's (the owner prefers fewer joints), and no leg has more joints "
        "than its kickstand has bends (one, its knee).",
        "- **4a clips** and **4b loops** run on the nominal build (`support_synthetic/nominal.rs`, the layout meshed roughly "
        "as DragonFruit builds what the VOXL writer writes): 4a, nothing but tips inside the cut model beyond what the "
        "original supports clip themselves; 4b, a contact loop at every truth tip within 0.15 mm of the loop the original "
        "support makes and within 20% of its area, and no contact the originals do not have.",
        "- Results are held to `backend/tests/support_real/expected.json`; a stage that starts or stops passing fails "
        "`cargo test -p findamodel --test support_real` (`REAL_UPDATE=1` rewrites the file).",
        "",
        "### Lychee's vocabulary and the reader's",
        "",
        "Only what means the same on both sides is compared.",
        "",
        "| Lychee | Reader (DragonFruit) | Compared as |",
        "| --- | --- | --- |",
        "| trunk (base on the plate) | trunk | a support standing on the plate |",
        "| branch (base on another support) | branch | a support standing on another support; its route is its own line and then the parent's line down |",
        "| leaf (a mini support on a host: one tapered rod, no cone) | branch with a blunt tip | a support standing on another support; only the contact, its width and the rod's direction are compared at the tip |",
        "| mesh to mesh (both ends on the model, mini or cone-ended) | strut, stick, twig or micro support | a support standing on the model, with a tip at each end |",
        "| kickstand (base on the plate, an upright pillar, then a lean onto a support) | one leg with a knee (DragonFruit kickstand) | "
        "one leg per kickstand (owner, 2026-11-25): the leg's knot within 1 mm of where the kickstand meets its host, at least 90% of "
        "the leg's line above 2 mm within 0.3 mm of the kickstand's line and at least 90% of the kickstand's line within 0.3 mm of the "
        "leg's, and no more bends than the kickstand has. The pillar read as a post and the lean as a brace is not accepted, nor is a "
        "leg that stops at the knee or cuts its corner. The leg's line counts as a shaft |",
        "| brace (two hosts) | brace | both ends within 1 mm |",
        "| (none) | post | Lychee has no posts: a reader post counts against the braces stage |",
        "| foot (disc or plate, with or without a cone), raft, foundation webs | root, raft | not compared: lines under 2 mm are left out |",
        "| ball joints at knees and sockets | joints and knots | not stored by Lychee; only the number of bends per support is compared |",
        "",
        "### Accepted readings",
        "",
        "Seven readings differ from Lychee's in name only. The scorer accepts each by a geometric rule "
        "(`backend/tests/support_real/score.rs`); a case's result says where one was used.",
        "",
        "| Lychee | Accepted reading | Rule |",
        "| --- | --- | --- |",
        "| a brace whose lower end is under 2 mm (inside its host's root) | a leg | the leg's knot is within 1 mm of the "
        "brace's upper end, and at least 90% of the leg's line above 2 mm lies within 0.3 mm of the brace. The brace then "
        "counts as found, the leg is not counted against Lychee's kickstands, and the leg's line counts as lying on a Lychee "
        "line in the shafts stage |",
        "| a brace lying wholly under 2 mm (both ends inside the root zone) | nothing | both ends of the Lychee brace are under 2 mm, "
        "where the reader has roots and routes and shafts are not compared either: the braces stage leaves it out (owner). A brace "
        "with one end at or above 2 mm is compared as any other |",
        "| a branch hanging from a kickstand | a support standing in the kickstand's foot, beside the leg | DragonFruit's kickstand "
        "holds up a support and carries no branch, so the branch's support stands on the plate and runs up the kickstand's pillar "
        "to where the branch leaves it. Accepted when it bends within 0.5 mm of that point and at least 90% of its line below the "
        "bend, above 2 mm, lies within 0.3 mm of the kickstand's line: that bend is not counted against Lychee's joints. The "
        "kickstand itself must still be read as one leg |",
        "| a branch that joins a trunk under 2 mm (inside the trunk's foot) | a support with a foot of its own on the plate | "
        "the tip's line above 2 mm is compared as any other (90% of each side within 0.5 mm of the other); where it "
        "passes, or is too short to compare, standing on the plate instead of on the trunk is accepted |",
        "| one brace that passes through other supports between its ends | one brace per stretch | two or more reader braces, "
        "each with both ends within 0.3 mm of the Lychee brace's line, run end to end (each starts within 0.3 mm of where "
        "the last one stopped), start and stop within 1 mm of the Lychee brace's ends, and meet on Lychee supports (each "
        "meeting point within 0.3 mm of a support's centre line). All of them then count as that one brace |",
        "| a branch's shaft and a brace ending at its socket that the mesh cannot tell apart | the tip hanging from the "
        "brace's other host, with the shaft as the brace | the branch hangs from its parent by one straight shaft and "
        "nothing hangs from the branch; the brace ends within 0.3 mm of the socket; the two rods have the same width "
        "(within 10%), both slope down from the socket (neither within 25 degrees of straight down nor level), turn off "
        "the tip cone's line within 5 degrees of each other, and no other brace ends on either between its ends. The "
        "tip's line down the brace and on down the brace's other host is then compared as any other; where it passes, "
        "the Lychee brace counts as found, a reader brace along the Lychee shaft counts as that shaft, and both count "
        "as lying on a line in the shafts stage |",
        "| a trunk whose own tip the reader misses | a post | at least 90% of the post's line above 2 mm lies within 0.3 mm "
        "of that trunk's line (one post per missed tip). The missed tip fails the tips stage; the post is not counted "
        "against the braces stage as well |",
        "",
        "Nothing else is waived: a leg off the brace's line, a branch that joins its trunk above 2 mm and is stood on the "
        "plate with a different line, braces that meet in mid air or stop short, a post with every tip found, and a "
        "shaft and brace swapped where one leaves the socket more steeply or carries another brace all fail as before.",
        "",
        "### Images",
        "",
        "Left to right: the original supports and the expected structure from the front, the same from the side, then the "
        "reader's nominal build from the front and from the side. The model cuts are drawn under the supports. The view is "
        "scaled to the supports (a small support on a large cut would be a speck), and a tall structure is drawn in parts "
        "side by side, the part on the plate left, so its braces stay readable. Shafts and braces are drawn as wide as the "
        "mesh has them where that differs from Lychee's setting (`d_lychee` in `truth.json` keeps the setting). Each "
        "foot is drawn as the mesh has it (`foot_pieces` in `truth.json`, measured by the builder): a flat disc or plate "
        "of the real width, height and number of sides, with the cone on it where the mesh has one; a support that stands "
        "on the raft alone has none.",
        "",
        "| Colour | Part |",
        "| --- | --- |",
    ]
    legend = [("model", "model cuts"), ("original", "original supports"), ("tip", "tips (a mini support's rod end is a short stub)"),
              ("trunk", "trunks"), ("branch", "branches and mini supports on a host"), ("strut", "supports between two parts of the model"),
              ("post", "posts (reader only)"), ("leg", "kickstands (reader: legs)"), ("brace", "braces and their end knots"), ("root", "feet (expected: as measured on the mesh; reader: its roots)"),
              ("knot", "knots where a branch or leg meets its host (reader only)")]
    for key, text in legend:
        r, g, b = cat.COLOURS[key]
        L.append(f"| <span style=\"display:inline-block;width:1.4em;height:0.9em;background:rgb({r},{g},{b})\"></span> | {text} |")
    r, g, b = RAFT
    L.append(f"| <span style=\"display:inline-block;width:1.4em;height:0.9em;background:rgb({r},{g},{b})\"></span> | raft (the part kept under the case's feet) |")
    L += [
        "",
        "## Summary",
        "",
        f"{n} cases from {len(creators)} creators ({', '.join(f'{c} {k}' for c, k in sorted(creators.items()))}), {mesh / 1e6:.1f} MB of mesh. "
        f"The truth validates on {len(valid)} cases; the reader passes every stage on {all_pass} of those.",
        "",
        "| Stage | Cases passing |",
        "| --- | --- |",
    ]
    for s in STAGES:
        L.append(f"| {s} | {tot[s]} of {n} |")
    L += ["", "| Case | Title | Creator | " + " | ".join(SHORT[s] for s in STAGES) + " |", "| --- | --- | --- | " + " | ".join("---" for _ in STAGES) + " |"]
    for r in rows:
        L.append(f"| [{r['id']}](#{r['id']}) | {md(r['title'])} | {md(r['truth']['source']['creator'])} | " + " | ".join(mark(r["stages"].get(s)) for s in STAGES) + " |")
    L += ["", "### What fails", ""]
    for s in STAGES:
        bad = [r["id"] for r in rows if not r["stages"].get(s)]
        if bad:
            L.append(f"- **{s}** ({len(bad)}): " + ", ".join(f"[{c}](#{c})" for c in bad) + ".")
    L += ["", "## Cases"]
    last = None
    for r in rows:
        g = GROUPS.get(r["id"][0], "Cases")
        if g != last:
            L += ["", f"### {g}"]
            last = g
        t = r["truth"]
        src = t["source"]
        L += ["", f"<a id=\"{r['id']}\"></a>", "", f"#### {r['id']}", "", f"{md(t['title'])}.", ""]
        if r["image"]:
            L += [f"![{r['id']}: original, expected and read]({IMG_REL}/{r['id']}.png)", ""]
        scope = "a whole structure" if t.get("scope") == "structure" else "a section"
        L += [f"**Situation.** {md(t['situation'])} ({scope}).", ""]
        ids = src["lychee_ids"]
        id_text = ", ".join(ids[:12]) + (f" and {len(ids) - 12} more" if len(ids) > 12 else "")
        L += [f"**Source.** {md(src['creator'])}, model `{src['model_id']}` (`{md(src.get('path') or src.get('name') or '')}`), Lychee {src.get('lychee_version')}; "
              f"Lychee supports {id_text}.", ""]
        raft = "with the raft under it" if t["raft"]["kept"] else "no raft"
        L += [f"**Holds.** {counts_text(t)}; {raft}; model cut to {len(t['model_cuts'])} box{'es' if len(t['model_cuts']) != 1 else ''} "
              f"({t.get('model_cut_mm')} mm round each contact); {r['mesh_bytes'] // 1024} KB of mesh.", ""]
        for note in t.get("notes") or []:
            L += [f"Note: {md(note)}.", ""]
        L += ["**Result.** " + ", ".join(f"{s} {mark(r['stages'].get(s))}" for s in STAGES) + ".", ""]
        acc = accepted(r["dump"])
        if acc:
            L += ["Accepted readings: " + ", ".join(acc) + ".", ""]
        shown = 0
        for s in STAGES:
            if r["stages"].get(s):
                continue
            for e in errors_of(r["dump"], s)[:4]:
                L.append(f"- {s}: `{md(e)}`")
                shown += 1
            extra = len(errors_of(r["dump"], s)) - 4
            if extra > 0:
                L.append(f"- {s}: and {extra} more")
        if shown:
            L.append("")
    text = re.sub(r"\n{3,}", "\n\n", "\n".join(L)).rstrip() + "\n"
    assert "\u2014" not in text, "no emdash in the docs"
    with open(PAGE, "w") as f:
        f.write(text)
    log(f"wrote {PAGE}")


RAFT = (96, 104, 120)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dump")
    ap.add_argument("--keep-dump")
    ap.add_argument("--cases")
    ap.add_argument("--no-images", action="store_true")
    a = ap.parse_args()
    tmp = None
    if a.dump:
        dump = a.dump
    else:
        dump = a.keep_dump or tempfile.mkdtemp(prefix="real-dump-")
        tmp = None if a.keep_dump else dump
        run_suite(dump)
    cat = load("synthetic_catalogue", "synthetic-catalogue.py")
    cases = load("real_support_cases", "real-support-cases.py")
    only = set(a.cases.split(",")) if a.cases else None
    os.makedirs(IMG_DIR, exist_ok=True)
    with open(os.path.join(SUITE, "cases.json")) as f:
        spec = json.load(f)
    rows = []
    try:
        for c in spec["cases"]:
            cid = c["case"]
            p = os.path.join(dump, cid + ".json")
            if not os.path.exists(p):
                log(f"{cid}: no dump file, skipped")
                continue
            with open(p) as f:
                d = json.load(f)
            with open(os.path.join(SUITE, cid, "truth.json")) as f:
                truth = json.load(f)
            img = os.path.join(IMG_DIR, cid + ".png")
            if not a.no_images and (only is None or cid in only):
                render(cat, cases, cid, d)
            rows.append({"id": cid, "title": truth["title"], "truth": truth, "stages": d["stages"], "dump": d, "image": os.path.exists(img),
                         "mesh_bytes": sum(os.path.getsize(os.path.join(SUITE, cid, x)) for x in ("model.stl", "supports.stl"))})
        keep = {r["id"] + ".png" for r in rows}
        for f in os.listdir(IMG_DIR):
            if f not in keep:
                os.remove(os.path.join(IMG_DIR, f))
        write_page(rows, cat)
    finally:
        if tmp:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
