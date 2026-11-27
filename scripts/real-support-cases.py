#!/usr/bin/env python3
"""Real-structure support cases: cuts the fixtures of backend/tests/support_real out of real pre-supported models.

A case is one support structure (or a self-contained section of one) of a real model whose Lychee project is known:
the original support shells of the chosen Lychee supports, the raft under them, small closed cuts of the model where
the supports meet it (never the whole model), and the expected result taken from the Lychee scene. The case list is
data: backend/tests/support_real/cases.json.

usage:
  real-support-cases.py build --truth DIR --split DIR [--meta FILE] [--cases ID,ID,...] [--no-images]
  real-support-cases.py images [--cases ID,ID,...]

  --truth DIR   Lychee truth files, DIR/<model id>.json (written by the Lychee truth tooling kept with the support
                work files: supports with their tips, segments, joints, roots and braces in the frame of the split STL)
  --split DIR   the supported file of each model split as the app splits it, DIR/<model id>.model.stl and
                DIR/<model id>.support.stl (support-reader-tool split SUPPORTED.stl DIR/<model id> RAFT_MM)
  --meta FILE   model metadata by id (creator, name, relativePath, raftHeightMm) for models the case list's own
                "models" map does not hold yet

build writes, per case, backend/tests/support_real/<case>/model.stl, supports.stl, truth.json and expected.png.
images redraws expected.png from the fixtures alone.

How a case is cut:
- Shells. The support mesh is split into shells (triangles joined through shared vertices). Each shell goes to the
  Lychee support whose primitives (shaft and brace rods, tip cones, the root under a trunk) hold most of its
  vertices; a wide shell on the plate that no support explains is raft.
- Supports. The case list names Lychee supports. A structure takes the named trunk with everything hanging from
  it; a section takes the named supports and the hosts under them down to the plate. Braces are kept only when
  both their hosts are kept, so no kept piece dangles. A shell welded of several supports brings them all into the
  case (a case that would grow past 40 supports this way is refused); a shell that only braces of the case share
  with braces outside it is left out with those braces.
- Braces in welded shells. A brace holding less than the owner's share of a welded shell has no shell of its own: it
  is in the case when its free span (the stretch outside its hosts) lies in the kept mesh, and left out when no kept
  mesh shows it. A brace to a support outside the case whose mesh is in a kept shell all the same would dangle: its
  far hosts come into the case, unless that takes more than a few supports (the case then says how many dangle).
- Stubs. A welded shell can hold the end of a brace or branch that leads to a support outside the case. Where
  more than 1 mm of a kept shell is no kept support's mesh but is that of a support outside the case, it is cut
  off: its triangles are removed and the hole is closed where the stub left the kept support (the case's truth
  says which supports were cut off). Mesh that no support explains is left alone, for the suite's validate
  stage to report.
- Bare lines. Where the kept shells leave a stretch of a kept support's line bare (its shell went to a neighbour
  outside the case that explains it as well), the shells along that stretch come into the case, the one holding
  most of the stretch first. As in the suite's validate stage, a shell that is another support's alone does not
  cover a line, however close that support's rod runs to it: where over 1 mm in a row of a line lies in such
  shells only (and not buried in another kept support), its own shell is looked for the same way.
- Doubles. A support placed twice at one spot (same line, same tips) is one support, and Lychee's braces between
  the two are left out: no mesh can show them.
- Widths. Lychee stores width settings, not what it drew (a brace's width is a guess from the tip setting): the
  mesh's own width along a brace (one width for the braces of one setting) or a shaft's long stretch replaces the
  setting where it is under 0.8 or over 1.15 of it (`d_lychee` keeps the setting), and a kickstand's run that
  tapers towards its host gets `d_run_end`.
- Kickstands. Lychee does not store how a kickstand is drawn: its line is fitted to the mesh (see
  `fit_kickstands`), and the case's truth says so.
- Model. Boxes around every contact (and wherever the kept supports come close to the model) are merged until
  they are disjoint; the model is clipped to each box and the cut faces are capped, so each cut is a closed solid
  with the real surface where the supports meet it. The boxes reach 1.5 mm from a contact, less (down to 0.5 mm)
  where a case's meshes would pass its budget (150 KB and 50 KB per tip): dense sculpts hold thousands of triangles
  in a few square millimetres.
- Feet. A flat shell centred on a trunk's or kickstand's foot with a regular outline at two heights (a disc, a
  square or octagon plate, or the cone on top of one) is that support's foot: the truth keeps it as measured
  (`foot_pieces`: heights, widths across the corners, sides and turn), and the pictures draw it so. Any other flat
  shell on the plate (the tiles of a pixelated raft, the links between neighbouring feet) is raft.
- Raft. The raft shells are clipped to a box round the kept supports' feet, capped the same way.
- Truth. The Lychee supports kept, in the STL's frame: tips, routes, shaft lines and braces.

Needs numpy and Pillow only. Other sources: the collection's CTB files are sliced plates, which hold layers and no
support structure, so every case comes from a Lychee project. A source that does give supports (tips, routes, lines,
braces in the split STL's frame) only has to be loaded into the same shape: `load_truth` is the hook.
"""
import argparse, importlib.util, json, math, os, struct, sys
from collections import Counter, OrderedDict, defaultdict

import numpy as np

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SUITE = os.path.join(REPO, "backend", "tests", "support_real")
CASES = os.path.join(SUITE, "cases.json")

CUT_SIZES_MM = (1.5, 1.25, 1.0, 0.8, 0.65, 0.5)  # half size of the model cut round a contact: the largest that fits the budget
BUDGET_KB = (150, 50, 700)  # mesh budget of a case (model cut and supports): a base, more per tip, and a cap
NEAR_BOX_MM = 1.0  # half size of the cut round a place where a support comes close to the model
NEAR_MM = 0.35  # "close to the model"
RAFT_MARGIN_MM = 3.0  # the raft is kept this far round the kept feet
EXPLAINED_MM = 0.25  # a shell vertex this near a Lychee primitive is explained by it
ROOT_ZONE_MM = 3.2  # shells wholly under this height are roots or raft
FOOT_CENTRE_MM = 0.3  # a foot piece's outline is centred on its support's foot this closely
FOOT_RING_MM = 0.03  # the corners of a foot piece's outline at one height lie this close to one circle round the foot
FOOT_BASE_MM = 1.2  # a foot piece starts under this height (a disc on the plate, or the cone on top of one)
FLAT_MM = 1.1  # a shell on the plate no taller than this that is no foot piece and no support's own mesh is raft
OWNER_SHARE = 0.12  # a support holding this share of a shell's vertices owns it (a welded shell has several owners)
BRACE_IN_MESH = 0.8  # a brace with no shell of its own is in the case when this share of its free span lies in the kept mesh
DANGLING_SHARE, DANGLING_MM = 0.6, 1.0  # a brace to a support outside the case dangles when this much of its free span is in the kept mesh
GROW_DANGLING = 6  # at most this many supports are added to give dangling braces their far hosts
SAME_SUPPORT_MM = 0.05  # two supports whose lines and tips agree this closely are one support placed twice
STUB_MM = 1.0  # a stretch of a kept shell larger than this that only a support outside the case explains is cut off
STUB_EXPLAINED_MM = 0.3  # "explains" for stubs: the suite's validate stage uses the same distance
WIDTH_SHAFT_MM = 4.0  # a shaft's width is measured on a stretch at least this long (shorter ones lie inside their joints' shells)
WIDTH_KEEP = (0.8, 1.15)  # a measured width within this ratio of Lychee's keeps Lychee's (a six-sided rod measures 0.87 to 1 of its nominal width)


def log(msg):
    print(msg, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- STL


def read_stl(path):
    b = open(path, "rb").read()
    n = struct.unpack_from("<I", b, 80)[0]
    a = np.frombuffer(b, dtype=np.dtype([("n", "<f4", 3), ("v", "<f4", (3, 3)), ("a", "<u2")]), count=n, offset=84)
    return a["v"].copy()


def write_stl(path, tris):
    tris = np.asarray(tris, np.float32).reshape(-1, 3, 3)
    a = np.zeros(len(tris), dtype=np.dtype([("n", "<f4", 3), ("v", "<f4", (3, 3)), ("a", "<u2")]))
    n = np.cross(tris[:, 1] - tris[:, 0], tris[:, 2] - tris[:, 0]).astype(np.float64)
    n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-30)
    a["n"] = n
    a["v"] = tris
    with open(path, "wb") as f:
        f.write(b"real support case".ljust(80, b" "))
        f.write(struct.pack("<I", len(tris)))
        f.write(a.tobytes())


def components(tris):
    """Shell of each triangle: triangles joined through shared vertices (exact match)."""
    v = tris.reshape(-1, 3)
    u, inv = np.unique(v, axis=0, return_inverse=True)
    f = inv.reshape(-1, 3)
    lab = np.arange(len(u))
    while True:
        m = lab[f].min(1)
        new = lab.copy()
        for k in range(3):
            np.minimum.at(new, f[:, k], m)
        new = new[new]
        if (new == lab).all():
            break
        lab = new
    _, tl = np.unique(lab[f[:, 0]], return_inverse=True)
    return tl


# ---------------------------------------------------------------- Lychee truth


def load_truth(path):
    """The Lychee truth of a model (the hook for other sources: anything giving the same shape works)."""
    with open(path) as f:
        return json.load(f)


def A3(p):
    return np.array(p, float)


def host_of(s):
    """The support a support hangs from (None for trunks, struts and braces)."""
    if s["kind"] == "brace":
        return None
    return s.get("host_tip") if s["kind"] in ("reverse_branch", "kickstand") else s.get("host_base")


def poly_of(s):
    """Bottom-to-top centre line of a support's shaft (cones left out)."""
    seg = s["segments"]
    if not seg:
        return [s["tips"][0]["socket"], s["tips"][0]["socket"]] if s["tips"] else []
    return [seg[0]["a"]] + [g["b"] for g in seg]


def project_down(poly, point):
    """The part of a bottom-to-top line below the projection of `point` onto it, top first."""
    best = (np.inf, 0, A3(poly[0]))
    for i, (a, b) in enumerate(zip(poly[:-1], poly[1:])):
        a, b = A3(a), A3(b)
        ab = b - a
        t = np.clip((A3(point) - a) @ ab / max(ab @ ab, 1e-12), 0, 1)
        q = a + t * ab
        d = np.linalg.norm(A3(point) - q)
        if d < best[0]:
            best = (d, i, q)
    _, i, q = best
    return [q] + [A3(p) for p in poly[: i + 1]][::-1]


def winding(p, tris):
    """The winding number of a mesh round a point (1 inside a closed solid)."""
    a, b, c = tris[:, 0] - p, tris[:, 1] - p, tris[:, 2] - p
    la, lb, lc = np.linalg.norm(a, axis=1), np.linalg.norm(b, axis=1), np.linalg.norm(c, axis=1)
    det = (a * np.cross(b, c)).sum(1)
    den = la * lb * lc + (a * b).sum(1) * lc + (b * c).sum(1) * la + (c * a).sum(1) * lb
    return float((2 * np.arctan2(det, den)).sum() / (4 * np.pi))


def line_share(poly, mesh, boxes=None):
    """Share of a line (above the root zone) that lies inside a mesh (`boxes`: its triangles' bounds, if at hand)."""
    pts = []
    for a, b in zip(poly[:-1], poly[1:]):
        a, b = A3(a), A3(b)
        n = max(2, int(np.linalg.norm(b - a) / 0.5))
        pts += [a + (b - a) * (i + 0.5) / n for i in range(n)]
    pts = [q for q in pts if q[2] > 2.0]
    if not pts:
        return 1.0
    lo, hi = np.min(pts, 0) - 3.0, np.max(pts, 0) + 3.0
    mlo, mhi = boxes if boxes is not None else (mesh.min(1), mesh.max(1))
    local = mesh[((mhi >= lo) & (mlo <= hi)).all(1)].astype(np.float64)
    return float(np.mean([winding(q, local) > 0.5 for q in pts]))


def seg_dist(q, a, b):
    """Distance from a point to a segment."""
    a, b = A3(a), A3(b)
    u = b - a
    t = np.clip((q - a) @ u / max(u @ u, 1e-12), 0, 1)
    return float(np.linalg.norm(q - (a + t * u)))


def free_span(ends, hosts, step=0.2):
    """Points along a brace outside its hosts (the stretch a mesh can show), and the length they stand for."""
    a, b = A3(ends[0]), A3(ends[1])
    L = float(np.linalg.norm(b - a))
    n = max(4, int(L / step))
    pts = [a + (b - a) * (i + 0.5) / n for i in range(n)]
    free = []
    for q in pts:
        if q[2] <= 2.0:
            continue
        inside = False
        for h in hosts:
            poly = poly_of(h)
            d = (h["segments"][-1]["d"] if h["segments"] else 1.0) or 1.0
            if any(seg_dist(q, x, y) < 0.35 * d for x, y in zip(poly[:-1], poly[1:])):
                inside = True
                break
        if not inside:
            free.append(q)
    return free, len(free) * L / n


def share_inside(pts, mesh, boxes):
    """Share of some points that lie inside a mesh."""
    if not len(pts):
        return 0.0
    lo, hi = np.min(pts, 0) - 3.0, np.max(pts, 0) + 3.0
    local = mesh[((boxes[1] >= lo) & (boxes[0] <= hi)).all(1)].astype(np.float64)
    return float(np.mean([winding(q, local) > 0.5 for q in pts]))


def measure_width(a, b, mesh, boxes, stations=(0.3, 0.4, 0.5, 0.6, 0.7), n=16):
    """The width of the mesh round a line: at stations along it, rays perpendicular to the line from the line to the
    first face; each station gives twice its median ray. A station inside another shell that overlaps this one sees
    that shell's faces first and comes out narrow, so the result is the mean of the two widest stations. None when
    the line is short or not inside the mesh."""
    a, b = A3(a), A3(b)
    L = float(np.linalg.norm(b - a))
    if L < 0.8:
        return None
    u = (b - a) / L
    h = np.array([0, 0, 1.0]) if abs(u[2]) < 0.9 else np.array([1.0, 0, 0])
    e1 = np.cross(u, h)
    e1 /= np.linalg.norm(e1)
    e2 = np.cross(u, e1)
    ang = np.arange(n) * 2 * math.pi / n
    dirs = np.cos(ang)[:, None] * e1 + np.sin(ang)[:, None] * e2
    out = []
    for s in stations:
        o = a + (b - a) * s
        if o[2] < 2.5:
            continue
        local = mesh[((boxes[1] >= o - 4.0) & (boxes[0] <= o + 4.0)).all(1)].astype(np.float64)
        if not len(local) or winding(o, local) < 0.5:
            continue
        v0, e_1, e_2 = local[:, 0], local[:, 1] - local[:, 0], local[:, 2] - local[:, 0]
        t0 = o - v0
        q = np.cross(t0, e_1)
        near = np.full(n, np.inf)
        for k in range(n):
            pv = np.cross(dirs[k], e_2)
            det = (e_1 * pv).sum(1)
            ok = np.abs(det) > 1e-12
            inv = np.where(ok, 1.0 / np.where(ok, det, 1.0), 0.0)
            uu = (t0 * pv).sum(1) * inv
            vv = (q @ dirs[k]) * inv
            tt = (e_2 * q).sum(1) * inv
            hit = ok & (uu >= -1e-9) & (vv >= -1e-9) & (uu + vv <= 1 + 1e-9) & (tt > 1e-6)
            if hit.any():
                near[k] = tt[hit].min()
        if np.isfinite(near).all():
            out.append(2 * float(np.median(near)))
    if not out:
        return None
    out.sort()
    return float(np.mean(out[-2:]))


def fit_kickstands(truth, mesh):
    """Lychee does not store how a kickstand is drawn. Its truth line (an upright pillar to the height
    `joinLength`, then straight to the host) is kept where the mesh agrees; where the mesh fits better, the line is
    an upright pillar to the height from which a 45 degree run reaches the host (what Lychee draws by default)."""
    boxes = None
    for s in truth["supports"]:
        if s["kind"] != "kickstand" or not s.get("root") or not s.get("attach") or not s["segments"]:
            continue
        if boxes is None:
            boxes = (mesh.min(1), mesh.max(1))
        a = A3(s["attach"])
        p = np.array([s["root"]["plate"][0], s["root"]["plate"][1], 0.0])
        kz = max(a[2] - float(np.linalg.norm(a[:2] - p[:2])), 0.0)
        knee = np.array([p[0], p[1], kz])
        if line_share([p, knee, a], mesh, boxes) > line_share(poly_of(s), mesh, boxes) + 0.05:
            d = s["segments"][0]["d"]
            s["segments"] = [{"a": p.tolist(), "b": knee.tolist(), "d": d, "role": "pillar"}, {"a": knee.tolist(), "b": a.tolist(), "d": d, "role": "kick"}]
            s["segments"] = [g for g in s["segments"] if np.linalg.norm(A3(g["a"]) - A3(g["b"])) > 1e-6]
            s["joints"] = [knee.tolist()] if kz > 0.3 and np.linalg.norm(knee - a) > 0.3 else []
            s["line_fit"] = "kickstand drawn as an upright pillar and a 45 degree run (fitted to the mesh)"


def primitives(truth):
    """Round cones (a, b, ra, rb) of every support, with the support's index and what each is."""
    A, B, RA, RB, K, SI = [], [], [], [], [], []
    for i, s in enumerate(truth["supports"]):
        for g in s["segments"]:
            r = (g["d"] or 1.0) / 2
            rb = (g.get("d_b") or g["d"] or 1.0) / 2
            A.append(g["a"]); B.append(g["b"]); RA.append(r); RB.append(rb); K.append(g["role"]); SI.append(i)
        for t in s["tips"]:
            if (t["length"] or 0) <= 1e-6:
                continue
            A.append(t["socket"]); B.append(t["contact"]); RA.append((t["body_d"] or 1.0) / 2); RB.append((t["contact_d"] or 0.3) / 2)
            K.append("tip"); SI.append(i)
    return np.array(A, float).reshape(-1, 3), np.array(B, float).reshape(-1, 3), np.array(RA), np.array(RB), K, np.array(SI, dtype=int)


def sdf_cones(q, A, B, RA, RB):
    """Signed distance of each point to each round cone: (len(q), len(A))."""
    AB = B - A
    L2 = np.maximum((AB * AB).sum(1), 1e-12)
    pa = q[:, None, :] - A[None]
    t = np.clip((pa * AB[None]).sum(2) / L2[None], 0, 1)
    return np.linalg.norm(pa - t[..., None] * AB[None], axis=2) - (RA[None] + t * (RB - RA)[None])


def foot_piece(p, trunks, txy):
    """A shell as a piece of a foot: (support index, piece), or None.

    A foot piece is a flat solid with a regular outline at two heights, centred on the foot of a trunk or kickstand:
    the disc (or square or octagon plate) on the plate, or the cone standing on it. The piece keeps what the mesh
    has: its two heights (`z`), its width across the corners at each (`d`), the number of sides and the turn of
    the first corner (`turn_deg`, from the x axis)."""
    if not len(txy):
        return None
    lo, hi = p.min(0), p.max(0)
    if lo[2] > FOOT_BASE_MM or hi[2] > ROOT_ZONE_MM or hi[2] - lo[2] < 0.05:
        return None
    low, high = p[p[:, 2] <= lo[2] + 0.02], p[p[:, 2] >= hi[2] - 0.02]
    if len(low) + len(high) != len(p):
        return None
    dd = np.linalg.norm(txy - (lo[:2] + hi[:2]) / 2, axis=1)
    k = int(dd.argmin())
    if dd[k] > FOOT_CENTRE_MM:
        return None
    f = txy[k]
    rings = []
    for ring in (low, high):
        r = np.linalg.norm(ring[:, :2] - f, axis=1)
        rim = ring[r > 0.05]
        if len(rim) < 3:
            return None
        rr = np.linalg.norm(rim[:, :2] - f, axis=1)
        ang = np.arctan2(rim[:, 1] - f[1], rim[:, 0] - f[0])
        # The outline is a regular polygon round the foot: its corners are the points furthest out, evenly
        # spaced, and every other point of the rim lies on its sides (a square plate has points along its sides).
        corner = rr >= rr.max() - FOOT_RING_MM
        n = int(corner.sum())
        if n < 3:
            return None
        ca = np.sort(ang[corner])
        if np.abs(np.diff(np.append(ca, ca[0] + 2 * math.pi)) - 2 * math.pi / n).max() > math.radians(2.0):
            return None
        side = (ang - ca[0]) % (2 * math.pi / n) - math.pi / n
        if np.abs(rr - rr.max() * math.cos(math.pi / n) / np.cos(side)).max() > 2 * FOOT_RING_MM:
            return None
        rings.append((float(rr.max()), n, math.degrees(ca[0]) % (360.0 / n)))
    if rings[0][1] != rings[1][1]:
        return None
    sides = rings[0][1]
    return trunks[k][0], {"z": [round(float(lo[2]), 3), round(float(hi[2]), 3)], "d": [round(2 * rings[0][0], 3), round(2 * rings[1][0], 3)],
                          "sides": sides, "turn_deg": round(rings[0][2], 2)}


def edge_parts(tris):
    """Part of each triangle of a shell: triangles joined through shared edges (a shared corner alone joins nothing)."""
    u, inv = np.unique(tris.reshape(-1, 3), axis=0, return_inverse=True)
    f = inv.reshape(-1, 3)
    e = np.concatenate([np.sort(f[:, [0, 1]], 1), np.sort(f[:, [1, 2]], 1), np.sort(f[:, [2, 0]], 1)])
    _, ek = np.unique(e[:, 0] * len(u) + e[:, 1], return_inverse=True)
    t = np.tile(np.arange(len(f)), 3)
    lab = np.arange(len(f))
    while True:
        m = np.full(ek.max() + 1, len(f))
        np.minimum.at(m, ek, lab[t])
        new = lab.copy()
        np.minimum.at(new, t, m[ek])
        new = new[new]
        if (new == lab).all():
            break
        lab = new
    return np.unique(lab, return_inverse=True)[1]


def foot_pieces(tri, trunks, txy):
    """The foot pieces in a shell, and whether the shell is nothing but one: ([(support index, piece)], whole).

    Mostly a foot piece is a shell of its own. Where a foot plate and the shaft standing on it share a corner (one
    shell to `components`), the piece is a part of the shell joined through shared edges."""
    if not len(txy) or tri[:, :, 2].min() > FOOT_BASE_MM:
        return [], False
    whole = foot_piece(np.unique(tri.reshape(-1, 3), axis=0).astype(np.float64), trunks, txy)
    if whole:
        return [whole], True
    part = edge_parts(tri)
    if part.max() == 0:
        return [], False
    out = [foot_piece(np.unique(tri[part == k].reshape(-1, 3), axis=0).astype(np.float64), trunks, txy) for k in range(part.max() + 1)]
    return [x for x in out if x], False


def assign_shells(sup, lab, truth):
    """Which Lychee supports each shell belongs to.

    Returns (owners, raft, feet): per shell the indices of the supports that explain it (each holding at least
    `OWNER_SHARE` of its vertices, most first; empty when none does), whether the shell is raft, and the foot
    pieces in each shell that holds any (see `foot_pieces`)."""
    A, B, RA, RB, K, SI = primitives(truth)
    n = lab.max() + 1
    order = np.argsort(lab, kind="stable")
    starts = np.searchsorted(lab[order], np.arange(n + 1))
    plo = np.minimum(A, B) - np.maximum(RA, RB)[:, None]
    phi = np.maximum(A, B) + np.maximum(RA, RB)[:, None]
    trunks = [(i, A3(s["root"]["plate"])) for i, s in enumerate(truth["supports"]) if s.get("root")]
    txy = np.array([p for _, p in trunks]).reshape(-1, 2)
    owners = [[] for _ in range(n)]
    raft = np.zeros(n, bool)
    feet = {}
    kinds = [s["kind"] for s in truth["supports"]]
    rank = {"brace": 3, "branch": 2, "leaf": 2, "reverse_branch": 2, "kickstand": 2, "mesh_to_mesh": 1, "trunk": 0}
    for c in range(n):
        tri = sup[order[starts[c] : starts[c + 1]]]
        p = np.unique(tri.reshape(-1, 3), axis=0).astype(np.float64)
        lo, hi = p.min(0), p.max(0)
        if lo[2] < 0.3 and hi[2] <= 6.0 and max(hi[0] - lo[0], hi[1] - lo[1]) >= 8.0:
            raft[c] = True
            continue
        pieces, whole = foot_pieces(tri, trunks, txy)
        if pieces:
            feet[c] = pieces
        if whole:
            owners[c] = [pieces[0][0]]
            continue
        cand = np.nonzero(((plo <= hi + EXPLAINED_MM) & (phi >= lo - EXPLAINED_MM)).all(1))[0]
        found = []
        whole_owner = False
        if len(cand):
            if len(p) > 600:
                p = p[:: len(p) // 600 + 1]
            d = sdf_cones(p, A[cand], B[cand], RA[cand], RB[cand])
            near = d <= EXPLAINED_MM
            sups = SI[cand]
            # One support explaining the whole shell owns it. Where several do (the ball where a brace or a
            # branch ends on its host lies in both), it is the one ending there: a brace before a branch or
            # leaf, those before the trunk.
            whole = [(rank[kinds[s]], -float(d[:, sups == s].min(1).mean()), int(s)) for s in np.unique(sups) if near[:, sups == s].any(1).mean() >= 0.9]
            if whole:
                found = [(1.0, max(whole)[2])]
                whole_owner = True
            else:
                # A shell welded of several supports: each vertex goes to the support whose primitive is nearest.
                first = np.where(near.any(1), sups[d.argmin(1)], -1)
                for s in np.unique(first[first >= 0]):
                    sh = (first == s).mean()
                    if sh >= OWNER_SHARE:
                        found.append((sh, int(s)))
                if sum(x[0] for x in found) < 0.5:
                    found = []
        on_plate = lo[2] < 0.3 and hi[2] <= FLAT_MM
        if on_plate and not whole_owner:
            # A flat shell on the plate that is no foot piece and no support's own mesh: a tile of a pixelated raft,
            # a link between neighbouring feet.
            raft[c] = True
            continue
        if not found and hi[2] <= ROOT_ZONE_MM and len(txy):
            # A root piece off the plate: the root of the trunk standing over it.
            c2 = (lo[:2] + hi[:2]) / 2
            dd = np.linalg.norm(txy - c2, axis=1)
            k = int(dd.argmin())
            if dd[k] <= 0.5 * max(hi[0] - lo[0], hi[1] - lo[1]) + 0.3:
                found = [(1.0, trunks[k][0])]
        if len(found) > 1 and lo[2] < 0.3 and hi[2] <= ROOT_ZONE_MM:
            # A flat piece on the plate between several feet: a web of the raft.
            raft[c] = True
        elif found:
            owners[c] = [s for _, s in sorted(found, reverse=True)]
        elif lo[2] < 0.3 and hi[2] <= 6.0:
            raft[c] = True
    return owners, raft, feet


# ---------------------------------------------------------------- clipping a closed mesh to a box


def clip_half(tris, axis, value, sign, lo=None, hi=None):
    """The part of a mesh on one side of an axis plane (sign * (x[axis] - value) <= 0), with the cut face capped.

    `lo` and `hi` are the box the mesh is being cut to: cap strips wholly outside it are not made."""
    if len(tris) == 0:
        return tris
    d = sign * (tris[:, :, axis] - value)
    inside = d <= 0
    cnt = inside.sum(1)
    out = [tris[cnt == 3]]
    segs = []

    def cut(p, q, dp, dq):
        # The same operands for both triangles of an edge (inside vertex first), so the cut points are identical.
        x = p + (q - p) * (dp / (dp - dq))[:, None]
        x[:, axis] = value
        return x

    one = np.nonzero(cnt == 1)[0]
    if len(one):
        k = inside[one].argmax(1)
        r = np.arange(len(one))
        t = tris[one]
        dd = d[one]
        v0, v1, v2 = t[r, k], t[r, (k + 1) % 3], t[r, (k + 2) % 3]
        d0, d1, d2 = dd[r, k], dd[r, (k + 1) % 3], dd[r, (k + 2) % 3]
        p01, p02 = cut(v0, v1, d0, d1), cut(v0, v2, d0, d2)
        out.append(np.stack([v0, p01, p02], 1))
        segs.append(np.stack([p02, p01], 1))
    two = np.nonzero(cnt == 2)[0]
    if len(two):
        k = (~inside[two]).argmax(1)
        r = np.arange(len(two))
        t = tris[two]
        dd = d[two]
        v0, v1, v2 = t[r, k], t[r, (k + 1) % 3], t[r, (k + 2) % 3]
        d0, d1, d2 = dd[r, k], dd[r, (k + 1) % 3], dd[r, (k + 2) % 3]
        p01, p20 = cut(v1, v0, d1, d0), cut(v2, v0, d2, d0)
        out.append(np.stack([p01, v1, v2], 1))
        out.append(np.stack([p01, v2, p20], 1))
        segs.append(np.stack([p01, p20], 1))
    if segs:
        out.append(cap(np.concatenate(segs), axis, value, sign, lo, hi))
    return np.concatenate(out)


def cap(segs, axis, value, sign, lo3=None, hi3=None):
    """Triangles filling the cut face bounded by oriented segments on an axis plane (outward normal sign * axis).

    Trapezoids between the segments in horizontal strips, filled where the winding is not zero: no loops are
    assembled, so it also closes cuts through messy meshes as well as they can be closed."""
    ua, va = (axis + 1) % 3, (axis + 2) % 3
    if sign < 0:
        ua, va = va, ua
    a2 = segs[:, 0][:, [ua, va]].astype(np.float64)
    b2 = segs[:, 1][:, [ua, va]].astype(np.float64)
    keep = a2[:, 1] != b2[:, 1]
    a2, b2 = a2[keep], b2[keep]
    if len(a2) == 0:
        return np.zeros((0, 3, 3), np.float32)
    # A polygon counter-clockwise seen from outside has its inside right of a segment going down.
    w = np.where(b2[:, 1] < a2[:, 1], 1, -1)
    lo = np.where((w > 0)[:, None], b2, a2)
    hi = np.where((w > 0)[:, None], a2, b2)
    ys = np.unique(np.concatenate([lo[:, 1], hi[:, 1]]))
    (u_lo, u_hi), (v_lo, v_hi) = ((-np.inf, np.inf),) * 2 if lo3 is None else ((lo3[ua], hi3[ua]), (lo3[va], hi3[va]))
    order = np.argsort(lo[:, 1], kind="stable")
    lo, hi, w = lo[order], hi[order], w[order]
    slope = (hi[:, 0] - lo[:, 0]) / (hi[:, 1] - lo[:, 1])
    out = []

    # A side of a filled gap lying wholly beyond the box (by a margin, so the later cuts still cross the cap) is
    # replaced by a straight edge there: what is beyond the box is cut away anyway, and a cut through the inside
    # of a large solid would otherwise bring every strip of its far outline.
    far_lo, far_hi = u_lo - 0.05, u_hi + 0.05

    def emit(left, right, y0, y1):
        # The trapezoid between two segments over a run of strips neither of them ends in.
        xl0, xl1 = ((far_lo, far_lo) if left < 0 else (lo[left, 0] + slope[left] * (y - lo[left, 1]) for y in (y0, y1)))
        xr0, xr1 = ((far_hi, far_hi) if right < 0 else (lo[right, 0] + slope[right] * (y - lo[right, 1]) for y in (y0, y1)))
        if xr0 - xl0 > 1e-9:
            out.append(((xl0, y0), (xr0, y0), (xr1, y1)))
        if xr1 - xl1 > 1e-9:
            out.append(((xl0, y0), (xr1, y1), (xl1, y1)))

    running = {}  # (left segment, right segment) of a filled gap: the height its trapezoid started at
    nxt = 0
    for y0, y1 in zip(ys[:-1], ys[1:]):
        while nxt < len(lo) and lo[nxt, 1] <= y0:
            nxt += 1
        filled = []
        if y1 - y0 >= 1e-9 and y1 >= v_lo and y0 <= v_hi:
            cur = np.nonzero((lo[:nxt, 1] <= y0) & (hi[:nxt, 1] >= y1))[0]
            if len(cur) >= 2:
                x0 = lo[cur, 0] + slope[cur] * (y0 - lo[cur, 1])
                x1 = lo[cur, 0] + slope[cur] * (y1 - lo[cur, 1])
                o = np.argsort(x0 + x1, kind="stable")
                x0, x1, cur = x0[o], x1[o], cur[o]
                wind = np.cumsum(w[cur])[:-1]
                for i in np.nonzero(wind != 0)[0]:
                    if max(x0[i + 1], x1[i + 1]) >= u_lo and min(x0[i], x1[i]) <= u_hi:
                        left = int(cur[i]) if max(x0[i], x1[i]) > far_lo else -1
                        right = int(cur[i + 1]) if min(x0[i + 1], x1[i + 1]) < far_hi else -1
                        filled.append((left, right))
        for key in [k for k in running if k not in filled]:
            emit(key[0], key[1], running.pop(key), y0)
        for key in filled:
            running.setdefault(key, y0)
    for key, start in running.items():
        emit(key[0], key[1], start, ys[-1])
    if not out:
        return np.zeros((0, 3, 3), np.float32)
    t2 = np.array(out, float)
    t3 = np.empty((len(t2), 3, 3))
    t3[:, :, axis] = value
    t3[:, :, ua] = t2[:, :, 0]
    t3[:, :, va] = t2[:, :, 1]
    return t3.astype(np.float32)


def clip_box(tris, lo, hi):
    """A closed mesh cut to a box, with the cut faces capped.

    `tris` must be the whole closed mesh: a cut face is filled from the whole cross-section of the mesh in its
    plane, so a mesh already trimmed to the box's neighbourhood leaves its cut faces open."""
    lo, hi = np.asarray(lo, float), np.asarray(hi, float)
    t = tris
    for axis in range(3):
        # Triangles wholly outside this axis's slab meet neither of its planes.
        t = t[(t[:, :, axis].max(1) >= lo[axis]) & (t[:, :, axis].min(1) <= hi[axis])]
        t = clip_half(t, axis, np.float32(lo[axis]), -1, lo, hi)
        t = clip_half(t, axis, np.float32(hi[axis]), 1, lo, hi)
    # Drop slivers of no area.
    a = np.linalg.norm(np.cross(t[:, 1] - t[:, 0], t[:, 2] - t[:, 0]).astype(np.float64), axis=1)
    return t[a > 1e-12]


def merge_boxes(boxes, gap=0.05):
    """Boxes merged (into their bounding box) until none overlap or come within `gap`."""
    boxes = [(np.array(l, float), np.array(h, float)) for l, h in boxes]
    changed = True
    while changed:
        changed = False
        out = []
        for l, h in boxes:
            for k, (ol, oh) in enumerate(out):
                if (l <= oh + gap).all() and (h >= ol - gap).all():
                    out[k] = (np.minimum(l, ol), np.maximum(h, oh))
                    changed = True
                    break
            else:
                out.append((l, h))
        boxes = out
    return boxes


class PointGrid:
    """Nearest point within h of each query (inf beyond)."""

    def __init__(self, pts, h):
        self.h, self.pts = h, pts
        self.o = pts.min(0) - h if len(pts) else np.zeros(3)
        c = np.floor((pts - self.o) / h).astype(np.int64)
        self.dim = c.max(0) + 3 if len(pts) else np.ones(3, dtype=np.int64)
        key = (c[:, 0] * self.dim[1] + c[:, 1]) * self.dim[2] + c[:, 2]
        self.order = np.argsort(key, kind="stable")
        self.keys = key[self.order]

    def query(self, q):
        best = np.full(len(q), np.inf)
        if len(self.pts) == 0 or len(q) == 0:
            return best
        c = np.floor((q - self.o) / self.h).astype(np.int64)
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                for dz in (-1, 0, 1):
                    cc = c + (dx, dy, dz)
                    ok = ((cc >= 0) & (cc < self.dim)).all(1)
                    key = (cc[:, 0] * self.dim[1] + cc[:, 1]) * self.dim[2] + cc[:, 2]
                    a = np.searchsorted(self.keys, key, "left")
                    b = np.searchsorted(self.keys, key, "right")
                    cnt = np.where(ok, b - a, 0)
                    for j in range(int(cnt.max(initial=0))):
                        m = cnt > j
                        if not m.any():
                            break
                        d = np.linalg.norm(self.pts[self.order[a[m] + j]] - q[m], axis=1)
                        mi = np.nonzero(m)[0]
                        best[mi] = np.minimum(best[mi], d)
        best[best > self.h] = np.inf
        return best


def surface_points(tris, step):
    """Points on a mesh no more than about `step` apart: corners, and a lattice on the larger triangles."""
    tris = tris.astype(np.float64)
    out = [tris.reshape(-1, 3), tris.mean(1)]
    e = np.maximum.reduce([np.linalg.norm(tris[:, i] - tris[:, (i + 1) % 3], axis=1) for i in range(3)])
    for k in np.nonzero(e > step)[0]:
        n = int(min(np.ceil(e[k] / step), 60))
        i, j = np.meshgrid(np.arange(n + 1), np.arange(n + 1))
        m = i + j <= n
        u, v = i[m] / n, j[m] / n
        t = tris[k]
        out.append(t[0] + u[:, None] * (t[1] - t[0]) + v[:, None] * (t[2] - t[0]))
    return np.concatenate(out)


# ---------------------------------------------------------------- a model and its Lychee supports


class Source:
    """One real model: its split meshes, its Lychee truth and the shells of each Lychee support."""

    def __init__(self, mid, truth_dir, split_dir):
        self.id = mid
        self.truth = load_truth(os.path.join(truth_dir, f"{mid}.json"))
        self.sup = self.truth["supports"]
        self.by_id = {s["id"]: i for i, s in enumerate(self.sup)}
        self.mesh = read_stl(os.path.join(split_dir, f"{mid}.support.stl"))
        self.model_path = os.path.join(split_dir, f"{mid}.model.stl")
        self._model = None
        fit_kickstands(self.truth, self.mesh)
        self.lab = components(self.mesh)
        self.owners, self.raft, self.feet = assign_shells(self.mesh, self.lab, self.truth)
        self.children = defaultdict(list)
        for i, s in enumerate(self.sup):
            h = host_of(s)
            if h in self.by_id:
                self.children[self.by_id[h]].append(i)
        order = np.argsort(self.lab, kind="stable")
        starts = np.searchsorted(self.lab[order], np.arange(self.lab.max() + 2))
        self.shell_tris = [order[starts[c] : starts[c + 1]] for c in range(self.lab.max() + 1)]
        self.shell_box = [(self.mesh[t].reshape(-1, 3).min(0), self.mesh[t].reshape(-1, 3).max(0)) for t in self.shell_tris]

    @property
    def model(self):
        if self._model is None:
            self._model = read_stl(self.model_path)
        return self._model

    def hosts(self, i):
        """The supports a support hangs from, or a brace joins."""
        s = self.sup[i]
        hs = [s.get("host_base"), s.get("host_tip")] if s["kind"] == "brace" else [host_of(s)]
        return [self.by_id[h] for h in hs if h in self.by_id]

    def ancestors(self, i):
        out, seen = [], {i}
        cur = i
        while True:
            h = host_of(self.sup[cur])
            if h not in self.by_id or self.by_id[h] in seen:
                return out
            cur = self.by_id[h]
            seen.add(cur)
            out.append(cur)

    def descendants(self, i):
        out, todo = [], [i]
        while todo:
            for c in self.children[todo.pop()]:
                if c not in out:
                    out.append(c)
                    todo.append(c)
        return out

    def select(self, named, scope, braces=True, limit=40):
        """The supports and shells of a case: (kept support indices, kept shells, notes)."""
        keep = set()
        for i in named:
            keep.add(i)
            keep.update(self.ancestors(i))
            if scope == "structure":
                root = (self.ancestors(i) or [i])[-1]
                keep.add(root)
                keep.update(self.descendants(root))
        notes = []
        added, no_more_hosts = set(), False
        while True:
            # Braces: only those with both hosts kept.
            kept_braces = set()
            if braces:
                for i, s in enumerate(self.sup):
                    if s["kind"] == "brace":
                        h = self.hosts(i)
                        if len(h) == 2 and h[0] in keep and h[1] in keep:
                            kept_braces.add(i)
            everything = keep | kept_braces
            shells, grow, lost = [], set(), set()
            for c, own in enumerate(self.owners):
                if not own or not (set(own) & everything):
                    continue
                out = [o for o in own if o not in everything]
                if not out:
                    shells.append(c)
                    continue
                # A shell welded with supports outside the case: braces outside it are cut off with the shell when
                # only braces of the case share it; any other support comes into the case with what it hangs from.
                mine = [o for o in own if o in everything]
                if all(self.sup[o]["kind"] == "brace" for o in mine) and braces:
                    lost.update(mine)
                    continue
                for o in out:
                    if self.sup[o]["kind"] == "brace":
                        grow.update(self.hosts(o))
                        for h in self.hosts(o):
                            grow.update(self.ancestors(h))
                    else:
                        grow.add(o)
                        grow.update(self.ancestors(o))
            grow -= keep
            if grow:
                keep |= grow
                if len(keep) > limit and added:
                    # The far hosts of dangling braces brought too much with them: the case stays as it was named.
                    keep, added, no_more_hosts = named_keep, set(), True
                    notes = notes[:named_notes]
                    continue
                notes.append(f"{len(grow)} supports welded to the case's shells were added")
                if len(keep) > limit:
                    raise ValueError(f"the case grows past {limit} supports through welded shells")
                continue
            # Braces to supports outside the case whose mesh is in the kept shells all the same (a welded shell
            # holds them below the owner's share): they would dangle, so their far hosts come into the case, as
            # long as that takes only a few supports.
            if not braces or no_more_hosts:
                break
            mesh, boxes = self.kept_mesh(shells)
            near_box = (boxes[0].min(0, keepdims=True), boxes[1].max(0, keepdims=True)) if len(mesh) else boxes
            far = set()
            dangling = []
            for i, s in enumerate(self.sup):
                if s["kind"] != "brace" or i in kept_braces:
                    continue
                h = self.hosts(i)
                if len(h) != 2 or set(h) <= keep or not self.near_mesh(s["ends"], near_box):
                    continue
                pts, length = free_span(s["ends"], [self.sup[x] for x in h])
                if length >= DANGLING_MM and share_inside(pts, mesh, boxes) >= DANGLING_SHARE:
                    dangling.append(i)
                    for x in h:
                        far.add(x)
                        far.update(self.ancestors(x))
            far -= keep
            if not far:
                break
            if len(added) + len(far) > GROW_DANGLING or len(keep) + len(far) > limit:
                # Too many: the case stays as it was named, and says what dangles in it.
                if added:
                    keep = named_keep
                    notes = notes[:named_notes]
                added = set()
                no_more_hosts = True
                continue
            if not added:
                named_keep, named_notes = set(keep), len(notes)
            keep |= far
            added |= far
        kept_braces -= lost
        if lost:
            notes.append(f"{len(lost)} braces welded to shells outside the case were left out")
        if added:
            notes.append(f"{len(added)} supports were added as the far hosts of braces welded into the case's shells")
        mesh, boxes = self.kept_mesh(shells)
        if no_more_hosts and braces:
            n = 0
            for i, s in enumerate(self.sup):
                h = self.hosts(i) if s["kind"] == "brace" else []
                if len(h) == 2 and not set(h) <= keep and self.near_mesh(s["ends"], boxes):
                    pts, length = free_span(s["ends"], [self.sup[x] for x in h])
                    n += length >= DANGLING_MM and share_inside(pts, mesh, boxes) >= DANGLING_SHARE
            if n:
                notes.append(f"{n} braces to supports outside the case dangle in the case's welded shells (their far hosts would bring too many supports into the case)")
        # The lines of the kept supports that the kept shells leave bare (a shell given to a neighbour that explains
        # it as well): the shells along them come into the case when the kept supports explain them.
        A, B, RA, RB, K, SI = primitives({"supports": [self.sup[i] for i in sorted(keep)]})
        order = sorted(keep)
        taken = set(shells)
        pulled = 0

        def pull(i, pts, bare_of, enough):
            """Adds the shells holding the bare points of a line of support `i`, the one holding most first."""
            nonlocal pulled, mesh, boxes
            adopted = set()
            for _ in range(6):
                bare_pts = bare_of(adopted)
                if not enough(bare_pts):
                    return
                best = None
                for c in range(len(self.shell_tris)):
                    if c in taken or self.raft[c]:
                        continue
                    lo, hi = self.shell_box[c]
                    inside = [q for q in bare_pts if (q >= lo).all() and (q <= hi).all()]
                    if not inside:
                        continue
                    st = self.mesh[self.shell_tris[c]].astype(np.float64)
                    held = sum(winding(q, st) > 0.5 for q in inside)
                    if not held:
                        continue
                    v = np.unique(st.reshape(-1, 3), axis=0)
                    if (sdf_cones(v, A, B, RA, RB).min(1) <= EXPLAINED_MM + 0.1).mean() >= 0.9 and (best is None or held > best[0]):
                        best = (held, c)
                if best is None:
                    return
                shells.append(best[1])
                taken.add(best[1])
                adopted.add(best[1])
                pulled += 1
                mesh, boxes = self.kept_mesh(shells)

        for n_i, i in enumerate(order):
            s = self.sup[i]
            lines = [(g["a"], g["b"], (g["d"] or 1.0) / 2, (g.get("d_b") or g["d"] or 1.0) / 2) for g in s["segments"]]
            lines += [(t["socket"], t["contact"], (t["body_d"] or 1.0) / 2, (t["contact_d"] or 0.3) / 2) for t in s["tips"] if (t["length"] or 0) > 0.5]
            others = SI != n_i
            for a, b, ra, rb in lines:
                a, b = A3(a), A3(b)
                n = max(2, int(np.linalg.norm(b - a) / 0.2))
                pts = [a + (b - a) * (k + 0.5) / n for k in range(n - 1)]
                pts = [q for q in pts if q[2] > 2.0]
                # No kept mesh at all round two or more points of the line.
                pull(i, pts, lambda _: [q for q in pts if len(mesh) == 0 or share_inside([q], mesh, boxes) < 0.5], lambda bp: len(bp) >= 2)
                # Kept mesh, but none of the line's own, as the suite's validate stage takes it: over 1 mm in a
                # row lies only in shells that are another support's alone (a
                # neighbour's rod crossing or lying beside the line; b14_branch_on_strut), and not where the
                # line's whole width is inside another kept support (an end buried in its host).
                if not pts:
                    continue
                rad = [ra + (rb - ra) * float(np.linalg.norm(q - a) / max(np.linalg.norm(b - a), 1e-9)) for q in pts]
                buried = np.zeros(len(pts), bool)
                if others.any():
                    buried = (sdf_cones(np.array(pts), A[others], B[others], RA[others], RB[others]).min(1) + np.array(rad)) <= EXPLAINED_MM

                def foreign_only(adopted):
                    own, own_boxes = self.kept_mesh([c for c in shells if c in adopted or not self.owners[c] or i in self.owners[c]])
                    return [q for q, bur in zip(pts, buried) if not bur and (len(own) == 0 or share_inside([q], own, own_boxes) < 0.5)]

                def fails(bare_pts):
                    keys = {tuple(q) for q in bare_pts}
                    run = best_run = 0
                    for q in pts:
                        run = run + 1 if tuple(q) in keys else 0
                        best_run = max(best_run, run)
                    return best_run * float(np.linalg.norm(b - a)) / n > 1.0

                pull(i, pts, foreign_only, fails)
        if pulled:
            notes.append(f"{pulled} shells along the kept supports' lines were added (they were given to neighbours outside the case)")
        kept = sorted(keep | kept_braces)
        have = {o for c in shells for o in self.owners[c]}
        # A brace with no shell of its own (its mesh is part of a welded shell, below the owner's share) is in the
        # case when its free span lies in the kept mesh, and left out when no mesh shows it.
        inside, outside = [], []
        for i in kept:
            if i in have or self.sup[i]["kind"] != "brace":
                continue
            pts, _ = free_span(self.sup[i]["ends"], [self.sup[x] for x in self.hosts(i)])
            if not pts:
                e = self.sup[i]["ends"]
                pts = [(A3(e[0]) + A3(e[1])) / 2]
            (inside if len(mesh) and share_inside(pts, mesh, boxes) >= BRACE_IN_MESH else outside).append(i)
        if inside:
            notes.append(f"{len(inside)} braces welded into the case's shells are kept by their lines")
        if outside:
            kept = [i for i in kept if i not in set(outside)]
            notes.append(f"no mesh in the case for the braces {', '.join(self.sup[i]['id'] for i in outside)}")
        bare = [self.sup[i]["id"] for i in kept if i not in have and self.sup[i]["kind"] != "brace"]
        if bare:
            notes.append(f"no shell found for {', '.join(bare)}")
        return kept, shells, notes

    def kept_mesh(self, shells):
        """The triangles of some shells, and their bounds."""
        if not shells:
            return np.zeros((0, 3, 3)), (np.zeros((0, 3)), np.zeros((0, 3)))
        m = self.mesh[np.concatenate([self.shell_tris[c] for c in shells])]
        return m, (m.min(1), m.max(1))

    def near_mesh(self, ends, boxes):
        """Whether a line's bounds meet a mesh's (`boxes`: its triangles' bounds)."""
        if not len(boxes[0]):
            return False
        p = np.array(ends, float)
        return bool((p.max(0) >= boxes[0].min(0) - 0.3).all() and (p.min(0) <= boxes[1].max(0) + 0.3).all())

    def route(self, i, tip):
        """A tip's way from its socket down to the plate, or to the model (as the Lychee scorer walks it)."""
        s = self.sup[i]
        poly = [A3(p) for p in poly_of(s)]
        if s["kind"] == "mesh_to_mesh":
            return poly[::-1] if tip.get("end") == "tip" else poly
        out = poly[::-1]
        seen, cur = {i}, s
        for _ in range(40):
            h = host_of(cur)
            if h not in self.by_id or self.by_id[h] in seen:
                break
            j = self.by_id[h]
            seen.add(j)
            hp = poly_of(self.sup[j])
            if len(hp) < 2:
                break
            out += project_down(hp, out[-1])
            cur = self.sup[j]
        return out


def foot_cones(sup, feet):
    """The foot pieces of some supports as round cones (a, b, ra, rb), like `primitives`."""
    A, B, RA, RB = [], [], [], []
    for i, q in feet:
        x, y = sup[i]["root"]["plate"]
        A.append([x, y, q["z"][0]]); B.append([x, y, q["z"][1]]); RA.append(q["d"][0] / 2); RB.append(q["d"][1] / 2)
    return np.array(A, float).reshape(-1, 3), np.array(B, float).reshape(-1, 3), np.array(RA), np.array(RB)


def trim_stubs(tris, kept_cones, other_cones):
    """A case's kept shells without the stubs of supports outside the case: (triangles, the corners of each stub
    cut off).

    A stub is a group of mesh corners, joined along triangle edges, that no kept primitive explains, larger than
    `STUB_MM` and explained by the primitives of supports outside the case (`other_cones`: a function giving
    their distance). Its triangles are removed, and each hole is closed with a fan over its rim, which lies on
    the kept support."""
    if not len(tris):
        return tris, []
    u, inv = np.unique(tris.reshape(-1, 3), axis=0, return_inverse=True)
    f = inv.reshape(-1, 3)
    pts = u.astype(np.float64)
    A, B, RA, RB = kept_cones
    d = np.concatenate([sdf_cones(pts[i : i + 4000], A, B, RA, RB).min(1) for i in range(0, len(pts), 4000)]) if len(A) else np.full(len(pts), np.inf)
    stray = d > STUB_EXPLAINED_MM
    if not stray.any():
        return tris, []
    e = np.concatenate([f[:, [0, 1]], f[:, [1, 2]], f[:, [2, 0]]])
    e = e[stray[e[:, 0]] & stray[e[:, 1]]]
    lab = np.arange(len(u))
    while len(e):
        m = np.minimum(lab[e[:, 0]], lab[e[:, 1]])
        new = lab.copy()
        np.minimum.at(new, e[:, 0], m)
        np.minimum.at(new, e[:, 1], m)
        new = new[new]
        if (new == lab).all():
            break
        lab = new
    cut = np.zeros(len(u), bool)
    stubs = []
    for c in np.unique(lab[stray]):
        g = stray & (lab == c)
        q = pts[g]
        if np.linalg.norm(q.max(0) - q.min(0)) <= STUB_MM or (other_cones(q) <= STUB_EXPLAINED_MM).mean() < 0.9:
            continue
        cut |= g
        stubs.append(q)
    if not stubs:
        return tris, []
    gone = cut[f].any(1)
    # The rim of each hole: the edges of removed triangles between corners that stay, in the removed triangle's
    # order (the triangle that stays across the edge runs the other way).
    rim = []
    for t in f[gone]:
        for k in range(3):
            a, b = t[k], t[(k + 1) % 3]
            if not cut[a] and not cut[b]:
                rim.append((a, b))
    rim = np.array(rim, dtype=int).reshape(-1, 2)
    caps = []
    if len(rim):
        # Rims are told apart by the corners they share.
        rl = np.arange(len(u))
        while True:
            m = np.minimum(rl[rim[:, 0]], rl[rim[:, 1]])
            new = rl.copy()
            np.minimum.at(new, rim[:, 0], m)
            np.minimum.at(new, rim[:, 1], m)
            new = new[new]
            if (new == rl).all():
                break
            rl = new
        for c in np.unique(rl[rim[:, 0]]):
            loop = rim[rl[rim[:, 0]] == c]
            centre = pts[np.unique(loop)].mean(0)
            for a, b in loop:
                caps.append((pts[a], pts[b], centre))
    out = tris[~gone]
    if caps:
        out = np.concatenate([out, np.array(caps, np.float32)])
    return out, stubs


def same_support(a, b):
    """Two supports of one kind with the same line and the same tips."""
    pa, pb = poly_of(a), poly_of(b)
    if a["kind"] != b["kind"] or len(pa) != len(pb) or len(a["tips"]) != len(b["tips"]):
        return False
    if any(np.linalg.norm(A3(x) - A3(y)) > SAME_SUPPORT_MM for x, y in zip(pa, pb)):
        return False
    return all(np.linalg.norm(A3(x["contact"]) - A3(y["contact"])) <= SAME_SUPPORT_MM for x, y in zip(a["tips"], b["tips"]))


ROLE = {"trunk": "trunk", "branch": "branch", "leaf": "branch", "reverse_branch": "branch", "mesh_to_mesh": "strut", "kickstand": "leg", "brace": "brace"}
BASE = {"trunk": "plate", "branch": "parent", "leaf": "parent", "reverse_branch": "parent", "mesh_to_mesh": "model", "kickstand": "plate"}


def surface_normal(model, c, axis):
    """Area-weighted normal of the model faces nearest a contact (None when the cut has no face near it)."""
    if len(model) == 0:
        return None, None
    m = model.astype(np.float64)
    cen = m.mean(1)
    d = np.linalg.norm(cen - c, axis=1)
    near = d <= max(0.3, np.sort(d)[min(len(d) - 1, 3)])
    if d.min() > 1.0:
        return None, float(d.min())
    n = np.cross(m[near, 1] - m[near, 0], m[near, 2] - m[near, 0])
    # Faces turned towards the support's side (a tip through a thin wall also sees the far faces).
    f = n[(n @ axis) > 0]
    n = f.sum(0) if len(f) else n.sum(0)
    l = np.linalg.norm(n)
    return (n / l).tolist() if l > 1e-12 else None, float(d.min())


def point_tri_dist(p, tris):
    """Distance from a point to the nearest of some triangles."""
    a, b, c = tris[:, 0], tris[:, 1], tris[:, 2]
    ab, ac, ap = b - a, c - a, p - a
    d1, d2 = (ab * ap).sum(1), (ac * ap).sum(1)
    bp = p - b
    d3, d4 = (ab * bp).sum(1), (ac * bp).sum(1)
    cp = p - c
    d5, d6 = (ab * cp).sum(1), (ac * cp).sum(1)
    va, vb, vc = d3 * d6 - d5 * d4, d5 * d2 - d1 * d6, d1 * d4 - d3 * d2
    den = np.maximum(va + vb + vc, 1e-30)
    v, w = vb / den, vc / den
    q = a + ab * v[:, None] + ac * w[:, None]
    inside = (va >= 0) & (vb >= 0) & (vc >= 0)
    best = np.where(inside, np.linalg.norm(p - q, axis=1), np.inf)
    for s0, s1 in ((a, b), (b, c), (c, a)):
        e = s1 - s0
        t = np.clip(((p - s0) * e).sum(1) / np.maximum((e * e).sum(1), 1e-30), 0, 1)
        best = np.minimum(best, np.linalg.norm(p - (s0 + e * t[:, None]), axis=1))
    return float(best.min()) if len(best) else float("inf")


def build_case(case, src, meta):
    """The meshes and truth of one case."""
    named = [src.by_id[x] for x in case["supports"]]
    kept, shells, notes = src.select(named, case.get("scope", "structure"), case.get("braces", True))
    sup = src.sup
    tris = src.mesh[np.concatenate([src.shell_tris[c] for c in shells])] if shells else np.zeros((0, 3, 3), np.float32)
    # A support placed twice at one spot (same line, same tips) is one support; a brace between the two is nothing.
    double = {}
    solid = [i for i in kept if sup[i]["kind"] != "brace"]
    for n, i in enumerate(solid):
        for j in solid[n + 1 :]:
            if i not in double and j not in double and same_support(sup[i], sup[j]):
                double[j] = i
    one = lambda i: double.get(i, i)
    if double:
        gone = [i for i in kept if sup[i]["kind"] == "brace" and len({one(h) for h in src.hosts(i)}) < 2]
        kept = [i for i in kept if i not in double and i not in gone]
        notes.append(f"{len(double)} supports placed twice at one spot are one support ({', '.join(sup[i]['id'] for i in double)})"
                     + (f"; {len(gone)} braces between a support and its double were left out" if gone else ""))
    # The stubs of supports outside the case, welded into the kept shells, are cut off.
    kept_all = set(kept) | set(double)
    feet_kept = [(k, q) for c in shells for k, q in src.feet.get(c, []) if k in kept_all]
    kc = primitives({"supports": [sup[i] for i in sorted(kept_all)]})[:4]
    fc = foot_cones(sup, feet_kept)
    kept_cones = tuple(np.concatenate([x, y]) for x, y in zip(kc, fc))
    others = [i for i in range(len(sup)) if i not in kept_all]
    oc = primitives({"supports": [sup[i] for i in others]})
    olo, ohi = np.minimum(oc[0], oc[1]) - np.maximum(oc[2], oc[3])[:, None], np.maximum(oc[0], oc[1]) + np.maximum(oc[2], oc[3])[:, None]

    def other_dist(q):
        near = ((olo <= q.max(0) + 1.0) & (ohi >= q.min(0) - 1.0)).all(1)
        return sdf_cones(q, oc[0][near], oc[1][near], oc[2][near], oc[3][near]).min(1) if near.any() else np.full(len(q), np.inf)

    def stub_owners(q):
        near = np.nonzero(((olo <= q.max(0) + 1.0) & (ohi >= q.min(0) - 1.0)).all(1))[0]
        if not len(near):
            return []
        dd = sdf_cones(q, oc[0][near], oc[1][near], oc[2][near], oc[3][near])
        return sorted({sup[others[int(oc[5][near[k]])]]["id"] for k in np.unique(dd.argmin(1))})

    tris, stubs = trim_stubs(tris, kept_cones, other_dist)
    if stubs:
        ids = sorted({x for q in stubs for x in stub_owners(q)})
        notes.append(f"{len(stubs)} stub{'s' if len(stubs) != 1 else ''} of supports outside the case, welded into the kept shells, {'were' if len(stubs) != 1 else 'was'} cut off"
                     f" ({', '.join(ids[:8])}{' and more' if len(ids) > 8 else ''})")
    tri_boxes = (tris.min(1), tris.max(1)) if len(tris) else None

    def width(a, b, lychee):
        """The width the mesh has along a line, where it differs from Lychee's setting."""
        m = measure_width(a, b, tris, tri_boxes) if tri_boxes is not None else None
        if m is None or not lychee or WIDTH_KEEP[0] <= m / lychee <= WIDTH_KEEP[1]:
            return None
        return round(m, 2)
    # The raft under the kept feet.
    feet = [A3(sup[i]["root"]["plate"]) for i in kept if sup[i].get("root")]
    raft_tris = np.zeros((0, 3, 3), np.float32)
    raft_shells = np.nonzero(src.raft)[0]
    if feet and case.get("raft", True) and len(raft_shells):
        f = np.array(feet)
        lo, hi = f.min(0) - RAFT_MARGIN_MM, f.max(0) + RAFT_MARGIN_MM
        for k in range(2):
            if hi[k] - lo[k] < 10.0:
                mid = (lo[k] + hi[k]) / 2
                lo[k], hi[k] = mid - 5.0, mid + 5.0
        lo3, hi3 = np.array([lo[0], lo[1], -1.0]), np.array([hi[0], hi[1], 8.0])
        parts = []
        for c in raft_shells:
            t = src.mesh[src.shell_tris[c]]
            p = t.reshape(-1, 3)
            if (p.max(0) < lo3).any() or (p.min(0) > hi3).any():
                continue
            parts.append(t if ((p.min(0) >= lo3) & (p.max(0) <= hi3)).all() else clip_box(t, lo3, hi3))
        if parts:
            raft_tris = np.concatenate(parts)
    # The model, cut round the contacts and wherever the kept supports come close to it. The cuts shrink until
    # the case's meshes fit its budget (dense sculpts hold thousands of triangles in a few square millimetres).
    contacts = [A3(t["contact"]) for i in kept for t in sup[i]["tips"]]
    model = src.model
    near_cells = np.zeros((0, 3))
    if len(tris):
        sp = surface_points(tris, 0.25)
        slo, shi = sp.min(0) - 1.0, sp.max(0) + 1.0
        mb_lo, mb_hi = model.min(1), model.max(1)
        cand = model[((mb_hi >= slo) & (mb_lo <= shi)).all(1)]
        if len(cand):
            mp = surface_points(cand, 0.15)
            d = PointGrid(sp, NEAR_MM + 0.15).query(mp)
            near = mp[d <= NEAR_MM]
            if len(near):
                near_cells = (np.unique(np.floor(near / 0.5).astype(np.int64), axis=0) + 0.5) * 0.5
    budget_kb = case.get("budget_kb", min(BUDGET_KB[0] + BUDGET_KB[1] * len(contacts), BUDGET_KB[2]))
    budget = budget_kb * 1024 - 50 * (len(tris) + len(raft_tris))
    # Only the model near the case is clipped.
    centres = np.array(contacts + list(near_cells)).reshape(-1, 3)
    if len(centres):
        reach = CUT_SIZES_MM[0] + 0.1
        lo_all, hi_all = centres.min(0) - reach, centres.max(0) + reach
        local = model[((model.max(1) >= lo_all) & (model.min(1) <= hi_all)).all(1)]
    else:
        local = model[:0]
    best = None
    # First with the places where a support only comes close to the model, then with the contacts alone (a support
    # hugging the model for a long way would bring a large part of it).
    for with_near in (True, False):
        for half in CUT_SIZES_MM:
            boxes = [(c - half, c + half) for c in contacts]
            if with_near:
                # Places near the supports that no contact's cut already holds (a shaft grazing the model).
                nh = min(NEAR_BOX_MM, 0.7 * half)
                for c in near_cells:
                    if not any(((c >= lo + 0.25) & (c <= hi - 0.25)).all() for lo, hi in boxes[: len(contacts)]):
                        boxes.append((c - nh, c + nh))
            boxes = merge_boxes(boxes)
            # A quick bound before clipping: the triangles touching the boxes.
            touching = sum(int(((local.max(1) >= lo) & (local.min(1) <= hi)).all(1).sum()) for lo, hi in boxes)
            if 50 * touching > 1.6 * budget and not (half == CUT_SIZES_MM[-1] and not with_near):
                continue
            cuts = [clip_box(model, lo, hi) for lo, hi in boxes]
            model_tris = np.concatenate(cuts) if cuts else np.zeros((0, 3, 3), np.float32)
            best = (model_tris, boxes, half, with_near)
            if 50 * len(model_tris) <= budget:
                break
        if best and 50 * len(best[0]) <= budget:
            break
    model_tris, boxes, half, with_near = best
    if 50 * len(model_tris) > budget:
        notes.append(f"over the {budget_kb} KB mesh budget at the smallest cut")
    if not with_near and len(near_cells):
        notes.append("the model is cut round the contacts only: the supports also come within 0.35 mm of it elsewhere")
    # The truth of the kept supports.
    t_sup, t_tips, t_braces = [], [], []
    for i in kept:
        s = sup[i]
        k = s["kind"]
        if k == "brace":
            hs = [sup[one(h)]["id"] for h in src.hosts(i)]
            rec = {"id": s["id"], "a": s["ends"][0], "b": s["ends"][1], "d": s["segments"][0]["d"] if s["segments"] else None, "hosts": hs}
            t_braces.append(rec)
            continue
        line = [list(map(float, p)) for p in poly_of(s)]
        host = host_of(s)
        g = s["segments"]
        rec = {"id": s["id"], "kind": k, "role": ROLE[k], "base": BASE[k], "host": sup[one(src.by_id[host])]["id"] if host in src.by_id else None, "line": line,
               "d": g[-1]["d"] if g else None, "joints": len(s["joints"]), "mini": bool(s.get("mini"))}
        if g and g[0].get("d_b") is not None:
            rec["d_tip"] = g[0]["d_b"]
        if g and not rec["mini"]:
            # The shaft's width as the mesh has it, on its longest stretch above the root zone.
            long = max(g, key=lambda x: float(np.linalg.norm(A3(x["b"]) - A3(x["a"]))) if max(x["a"][2], x["b"][2]) > ROOT_ZONE_MM else 0.0)
            m = width(long["a"], long["b"], rec["d"]) if np.linalg.norm(A3(long["b"]) - A3(long["a"])) >= WIDTH_SHAFT_MM else None
            if m:
                rec["d_lychee"], rec["d"] = rec["d"], m
            if k == "kickstand" and len(g) > 1 and tri_boxes is not None:
                # Lychee tapers a kickstand's run towards its host.
                lo_w = measure_width(g[-1]["a"], g[-1]["b"], tris, tri_boxes, stations=(0.15, 0.25, 0.35))
                hi_w = measure_width(g[-1]["a"], g[-1]["b"], tris, tri_boxes, stations=(0.65, 0.75, 0.85))
                if lo_w and hi_w and hi_w < 0.75 * lo_w:
                    rec["d_run_end"] = round(hi_w, 2)
        if s.get("root"):
            rec["foot"] = s["root"]["plate"]
            # The foot as the mesh has it (a support placed twice has one foot; its pieces may sit with either).
            pieces = [q for c in shells for k, q in src.feet.get(c, []) if one(k) == i]
            rec["foot_pieces"] = sorted({json.dumps(q, sort_keys=True) for q in pieces}, key=lambda q: json.loads(q)["z"])
            rec["foot_pieces"] = [json.loads(q) for q in rec["foot_pieces"]]
        if s.get("attach"):
            rec["attach"] = s["attach"]
        if s.get("line_fit"):
            notes.append(f"{s['id']}: {s['line_fit']}")
        t_sup.append(rec)
        for tp in s["tips"]:
            c, ax = A3(tp["contact"]), A3(tp["axis"])
            normal, gap = surface_normal(model_tris, c, ax)
            route = [c] + src.route(i, tp)
            if k == "mesh_to_mesh":
                other = [x for x in s["tips"] if x is not tp]
                if other:
                    route.append(A3(other[0]["contact"]))
            t_tips.append({"support": s["id"], "end": tp["end"], "contact": tp["contact"], "axis": tp["axis"], "socket": tp["socket"],
                           "contact_d": tp["contact_d"], "body_d": tp["body_d"], "length": tp["length"], "base": BASE[k],
                           "normal": normal, "surface_mm": gap, "route": [list(map(float, p)) for p in route]})
    # Braces of one Lychee width are drawn alike: they take the mesh's width (the upper quartile of what the braces
    # measure: one crossing another, or short between close hosts, measures narrow) where that differs from the setting.
    by_setting = defaultdict(list)
    for rec in t_braces:
        if rec["d"] and tri_boxes is not None:
            m = measure_width(rec["a"], rec["b"], tris, tri_boxes)
            if m:
                by_setting[rec["d"]].append(m)
    for rec in t_braces:
        ms = by_setting.get(rec["d"])
        if ms:
            m = float(np.percentile(ms, 75))
            if not WIDTH_KEEP[0] <= m / rec["d"] <= WIDTH_KEEP[1]:
                rec["d_lychee"], rec["d"] = rec["d"], round(m, 2)
    raft_info = src.truth.get("raft", {}).get("object") or src.truth.get("raft", {}).get("preset") or {}
    truth = OrderedDict([
        ("case", case["case"]), ("title", case["title"]), ("situation", case["situation"]), ("scope", case.get("scope", "structure")),
        ("source", OrderedDict([("model_id", src.id), ("creator", meta.get("creator")), ("name", meta.get("name")), ("path", meta.get("relativePath")),
                                ("lys_id", src.truth.get("lys_id")), ("lys_path", src.truth.get("lys_path")), ("lychee_version", src.truth.get("app_version")),
                                ("named", case["supports"]), ("lychee_ids", [sup[i]["id"] for i in kept])])),
        ("raft_mm", float(meta.get("raftHeightMm") or 3.0)),
        ("raft", {"kept": bool(len(raft_tris)), "lychee_type": raft_info.get("type"), "lychee_enabled": raft_info.get("enabled")}),
        ("counts", dict(Counter(ROLE[sup[i]["kind"]] for i in kept))),
        ("supports", t_sup), ("tips", t_tips), ("braces", t_braces),
        ("model_cut_mm", half), ("model_cuts", [{"lo": lo.round(4).tolist(), "hi": hi.round(4).tolist()} for lo, hi in boxes]),
        ("notes", notes + ([case["note"]] if case.get("note") else [])),
    ])
    return {"model": model_tris, "supports": np.concatenate([tris, raft_tris]) if len(raft_tris) else tris, "n_raft": len(raft_tris), "truth": truth}


# ---------------------------------------------------------------- pictures


def _catalogue():
    """The synthetic catalogue's renderer and colours (scripts/synthetic-catalogue.py)."""
    spec = importlib.util.spec_from_file_location("synthetic_catalogue", os.path.join(REPO, "scripts", "synthetic-catalogue.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def frustum(a, b, ra, rb, n=20, turn_deg=None):
    """A frustum between two points; `turn_deg` turns an upright one's first corner to that angle round the axis."""
    a, b = np.asarray(a, float), np.asarray(b, float)
    ax = b - a
    L = np.linalg.norm(ax)
    if L < 1e-9:
        return np.zeros((0, 3, 3))
    ax /= L
    h = np.array([0, 0, 1.0]) if abs(ax[2]) < 0.9 else np.array([1.0, 0, 0])
    u = np.cross(ax, h)
    u /= np.linalg.norm(u)
    v = np.cross(ax, u)
    if turn_deg is not None:
        u, v = np.array([1.0, 0, 0]), np.array([0, 1.0, 0]) * (1 if ax[2] > 0 else -1)
    ang = np.arange(n) * 2 * math.pi / n + math.radians(turn_deg or 0.0)
    ring = np.cos(ang)[:, None] * u + np.sin(ang)[:, None] * v
    pa, pb = a + ring * ra, b + ring * rb
    out = []
    for i in range(n):
        j = (i + 1) % n
        out += [(pa[i], pa[j], pb[j]), (pa[i], pb[j], pb[i]), (a, pa[j], pa[i]), (b, pb[i], pb[j])]
    return np.array(out)


def truth_mesh(truth):
    """The expected structure as coloured solids: (triangles, role of each)."""
    tris, roles = [], []

    def add(t, role):
        if len(t):
            tris.append(t)
            roles.extend([role] * len(t))

    by_id = {s["id"]: s for s in truth["supports"]}
    for s in truth["supports"]:
        line = s["line"]
        d = s.get("d") or 0.5
        for k, (a, b) in enumerate(zip(line[:-1], line[1:])):
            if s.get("mini"):
                add(frustum(a, b, d / 2, (s.get("d_tip") or d) / 2), s["role"])
            elif s.get("d_run_end") and k == len(line) - 2:
                add(frustum(a, b, d / 2, s["d_run_end"] / 2), s["role"])
            else:
                add(frustum(a, b, d / 2, d / 2), s["role"])
        if s.get("foot"):
            # The foot as the mesh has it: a flat disc (or plate), and the cone on it where there is one.
            f = s["foot"]
            for q in s.get("foot_pieces") or []:
                add(frustum([f[0], f[1], q["z"][0]], [f[0], f[1], q["z"][1]], q["d"][0] / 2, q["d"][1] / 2, n=q.get("sides", 24), turn_deg=q.get("turn_deg", 0.0)), "root")
    for t in truth["tips"]:
        if (t["length"] or 0) > 1e-6:
            add(frustum(t["socket"], t["contact"], (t["body_d"] or 0.5) / 2, max((t["contact_d"] or 0.2) / 2, 0.04)), "tip")
        else:
            # A rod end with no cone: a short marker at the contact.
            s = by_id[t["support"]]
            ax = np.asarray(t["axis"], float)
            c = np.asarray(t["contact"], float)
            r = (t["contact_d"] or s.get("d") or 0.3) / 2
            add(frustum(c, c + ax * min(0.4, 2 * r + 0.1), r * 1.25, r * 1.25), "tip")
    for b in truth["braces"]:
        add(frustum(b["a"], b["b"], (b["d"] or 0.5) / 2, (b["d"] or 0.5) / 2), "brace")
    if not tris:
        return np.zeros((0, 3, 3)), []
    return np.concatenate(tris), roles


PANEL_W, PANEL_H = 250, 500
VIEW_MARGIN = (1.0, 0.15)  # the view holds the model cut this far round the supports: mm at least, and a share of their size
TALL_PARTS = 4  # a tall structure is drawn in up to this many parts side by side


def view_points(model, sup, tm):
    """The points a case's pictures are scaled to: the supports and the expected structure, and the model cut near
    them (a small support on a large cut would be a speck otherwise)."""
    core = np.concatenate([np.asarray(sup, float).reshape(-1, 3), np.asarray(tm, float).reshape(-1, 3)])
    mp = np.asarray(model, float).reshape(-1, 3)
    if not len(core):
        return mp
    lo, hi = core.min(0), core.max(0)
    m = max(VIEW_MARGIN[0], VIEW_MARGIN[1] * float(np.linalg.norm(hi - lo)))
    near = mp[((mp >= lo - m) & (mp <= hi + m)).all(1)]
    box = np.array([lo - min(m, 0.5), hi + min(m, 0.5)])
    return np.concatenate([core, near, box])


def render_panels(cat, model, panels, out_path, extent_pts):
    """A row of panels, front then side: each panel a (title, triangles, colour per triangle) drawn over the model's
    far faces."""
    from PIL import Image, ImageDraw, ImageFont

    font = ImageFont.load_default(size=12)
    img = Image.new("RGB", (PANEL_W * len(panels) * 2, PANEL_H), cat.BG)
    draw = ImageDraw.Draw(img)
    model = np.asarray(model, float).reshape(-1, 3, 3)
    for vi, (vname, az) in enumerate(cat.VIEWS):
        m = cat.camera(az, cat.ELEVATION)
        sp = extent_pts @ m.T
        lo, hi = sp.min(0), sp.max(0)
        pad = 0.04 * (hi - lo) + 0.6
        lo, hi = lo - pad, hi + pad
        margin, top = 8, 18
        # A tall structure is drawn in parts side by side, the lowest left: one column would leave it a few pixels wide.
        width, height = max(hi[0] - lo[0], 1e-6), max(hi[1] - lo[1], 1e-6)
        def fit(n):
            return min((PANEL_W // n - 2 * margin) / width, (PANEL_H - top - 2 * margin) / (height / n * (1.04 if n > 1 else 1.0)))

        parts = max(range(1, TALL_PARTS + 1), key=fit)
        if fit(parts) < 1.25 * fit(1):
            parts = 1
        pw = PANEL_W // parts
        scale = min(fit(parts), 90.0)
        mshow = model  # the cuts are small: all of them is drawn, always under the supports
        layers = [[None] * parts for _ in panels]
        for k in range(parts):
            centre = np.array([(lo[0] + hi[0]) / 2, lo[1] + height * (k + 0.5) / parts])

            def to_screen(t):
                s = t @ m.T
                out = np.empty_like(s)
                out[..., 0] = pw / 2 + (s[..., 0] - centre[0]) * scale
                out[..., 1] = top + (PANEL_H - top) / 2 - (s[..., 1] - centre[1]) * scale
                out[..., 2] = s[..., 2]
                return out

            if len(mshow):
                marr, _ = cat.rasterise(to_screen(mshow), cat.shade(mshow, cat.COLOURS["model"], m), pw, PANEL_H)
            else:
                marr, _ = cat.rasterise([], None, pw, PANEL_H)
            for pi, (title, tris, cols) in enumerate(panels):
                tris = np.asarray(tris, float).reshape(-1, 3, 3)
                if len(tris):
                    sarr, smask = cat.rasterise(to_screen(tris), cat.shade(tris, cols, m), pw, PANEL_H)
                    layers[pi][k] = np.where(smask[..., None], sarr, marr)
                else:
                    layers[pi][k] = marr
        # Each part shows its own stretch of the height only (with the small overlap), so nothing is drawn twice over.
        half = height / parts * 1.04 / 2 * scale
        mid = top + (PANEL_H - top) / 2
        for pi, (title, tris, cols) in enumerate(panels):
            x = (vi * len(panels) + pi) * PANEL_W
            for k in range(parts):
                if parts > 1:
                    layers[pi][k] = layers[pi][k].copy()
                    layers[pi][k][: max(int(mid - half), 0)] = cat.BG
                    layers[pi][k][int(mid + half) + 1 :] = cat.BG
                img.paste(Image.fromarray(layers[pi][k]), (x + k * pw, 0))
                if k:
                    draw.line([(x + k * pw, top + 4), (x + k * pw, PANEL_H)], fill=(44, 44, 50))
            draw.rectangle([x + 1, 0, x + PANEL_W - 1, top - 1], fill=cat.BG)
            draw.text((x + 6, 3), f"{title}, {vname}" + (f" ({parts} parts, plate left)" if parts > 1 else ""), fill=(220, 220, 225), font=font)
            draw.line([(x, 0), (x, PANEL_H)], fill=(60, 60, 66))
    img.quantize(colors=48, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(out_path, optimize=True)


RAFT_COLOUR = (96, 104, 120)


def expected_image(cat, case_dir):
    """expected.png: the original supports over the cut model, and the expected structure by role."""
    model = read_stl(os.path.join(case_dir, "model.stl")).astype(np.float64)
    sup = read_stl(os.path.join(case_dir, "supports.stl")).astype(np.float64)
    with open(os.path.join(case_dir, "truth.json")) as f:
        truth = json.load(f)
    n_raft = truth.get("raft_triangles", 0)
    raft = sup[len(sup) - n_raft :] if n_raft else sup[:0]
    tm, roles = truth_mesh(truth)
    cols = np.array([cat.COLOURS[r] for r in roles], float).reshape(-1, 3)
    exp = np.concatenate([tm, raft]) if len(raft) else tm
    ecols = np.concatenate([cols, np.tile(np.array(RAFT_COLOUR, float), (len(raft), 1))]) if len(raft) else cols
    ocols = np.tile(np.array(cat.COLOURS["original"], float), (len(sup), 1))
    if n_raft:
        ocols[len(sup) - n_raft :] = RAFT_COLOUR
    render_panels(cat, model, [("original", sup, ocols), ("expected", exp, ecols)], os.path.join(case_dir, "expected.png"), view_points(model, sup, tm))


# ---------------------------------------------------------------- main


def load_cases():
    with open(CASES) as f:
        return json.load(f)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", choices=["build", "images"])
    ap.add_argument("--truth")
    ap.add_argument("--split")
    ap.add_argument("--meta", help="JSON of model metadata by id (creator, name, relativePath, raftHeightMm)")
    ap.add_argument("--cases")
    ap.add_argument("--no-images", action="store_true")
    a = ap.parse_args()
    spec = load_cases()
    only = set(a.cases.split(",")) if a.cases else None
    cases = [c for c in spec["cases"] if only is None or c["case"] in only]
    cat = None if a.no_images else _catalogue()
    if a.command == "images":
        for c in cases:
            expected_image(cat, os.path.join(SUITE, c["case"]))
            log(c["case"])
        return
    if not a.truth or not a.split:
        ap.error("build needs --truth and --split")
    meta = spec.get("models", {})
    if a.meta:
        with open(a.meta) as f:
            meta = {**json.load(f), **meta}
    by_model = defaultdict(list)
    for c in cases:
        by_model[c["model"]].append(c)
    total = 0
    for mid, cs in by_model.items():
        src = Source(mid, a.truth, a.split)
        for c in cs:
            try:
                r = build_case(c, src, meta.get(mid, {}))
            except (ValueError, KeyError) as e:
                log(f"{c['case']}: FAILED {e}")
                continue
            d = os.path.join(SUITE, c["case"])
            os.makedirs(d, exist_ok=True)
            write_stl(os.path.join(d, "model.stl"), r["model"])
            write_stl(os.path.join(d, "supports.stl"), r["supports"])
            r["truth"]["raft_triangles"] = int(r["n_raft"])
            with open(os.path.join(d, "truth.json"), "w") as f:
                json.dump(r["truth"], f, indent=1)
                f.write("\n")
            if cat:
                expected_image(cat, d)
            size = sum(os.path.getsize(os.path.join(d, x)) for x in ("model.stl", "supports.stl"))
            total += size
            t = r["truth"]
            log(f"{c['case']}: {len(t['supports'])} supports, {len(t['tips'])} tips, {len(t['braces'])} braces, model {len(r['model'])} + supports {len(r['supports'])} triangles "
                f"({size // 1024} KB){'; ' + '; '.join(t['notes']) if t['notes'] else ''}")
    log(f"{len(cases)} cases, {total // 1024} KB of mesh")


if __name__ == "__main__":
    main()
