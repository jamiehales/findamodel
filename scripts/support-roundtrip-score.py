"""Touch-rule scorer of the support reader round trip: built tips against the original's touch-rule contacts.

Copied from the prototype's scorer (scratchpad touch/score.py, merge r6 with owner decisions 4 and 9); the
matching, sizing and tolerances are unchanged. Only the inputs are passed in (the truth directory) instead of
being fixed paths, and the prototype's benchmark subsets and command line are left out.

- Pairing: a truth contact pairs one-to-one with a built tip whose own loop centre at the 0.05 mm offset (its
  written position when it has no loop) lies within POS_CAP (0.5 mm) and within half the two contact diameters.
- Decision 4: a built tip left over after the one-to-one pairing whose loop centre lands inside an original
  contact's loop matches that contact too (it is not extra).
- Decision 9: tip sizes come from a DragonFruit build made at its own 0.05 mm tip offset (touch built with
  TOUCH_PEN=0), paired to the built tip by written position (within 0.2 mm), instead of the 0.05 mm run-on of the
  offset 0 build (area05). A tip is within 10% (20%) when its loop's equivalent diameter is within 10% (20%) of
  the truth contact's.
- Clips: the built mesh's inside-the-model patches that are not tips (patch "clip"), summed by area (mm2).
"""
import json, math, os, sys
from collections import Counter

POS_CAP = 0.5


def eq_d(a):
    return 2 * math.sqrt(max(a, 0) / math.pi)


def key_of(t):
    """Where a built tip is paired from: its own loop's centre at the 0.05 mm offset; the written position when the
    tip has no loop (counted as a fallback)."""
    lp = t.get("loop05")
    return (lp["centre"], False) if lp else (t["pos"], True)


OLD = bool(os.environ.get("TOUCH_OLD"))


def inside_extra(truth, tips, m):
    """merge (decision 4): built tips not in the one-to-one pairing whose loop centre lies inside an original
    contact's loop: {tip: contact}."""
    if OLD:
        return {}
    used = set(m.values())
    out = {}
    for j, t in enumerate(tips):
        if j in used:
            continue
        k = key_of(t)[0]
        best = min(((math.dist(c["at"], k), i) for i, c in enumerate(truth) if math.dist(c["at"], k) <= c["d"] / 2), default=None)
        if best:
            out[j] = best[1]
    return out


def off_sizes(tips, off_tips):
    """merge (decision 9): each built tip's size from the offset build: the offset build's tip at the same written
    position (within 0.2 mm); None where there is none."""
    out = []
    for t in tips:
        best = min(((math.dist(t["pos"], o["pos"]), o) for o in off_tips), key=lambda x: x[0], default=None)
        out.append(best[1].get("area05") if best and best[0] <= 0.2 else None)
    return out


def match(truth, tips):
    keys = [key_of(t)[0] for t in tips]
    pairs = []
    for i, c in enumerate(truth):
        for j, t in enumerate(tips):
            d = math.dist(c["at"], keys[j])
            if d <= POS_CAP and d <= max(c["d"] / 2 + t["contact_d"] / 2, 0.05):
                pairs.append((d, i, j))
    pairs.sort()
    ua, ub, out = set(), set(), {}
    for d, i, j in pairs:
        if i not in ua and j not in ub:
            ua.add(i); ub.add(j); out[i] = j
    return out


def line_angle(a, b):
    """Angle between two axes as lines (0 to 90 degrees)."""
    na, nb = math.sqrt(sum(x * x for x in a)), math.sqrt(sum(x * x for x in b))
    c = abs(sum(x * y for x, y in zip(a, b))) / max(na * nb, 1e-12)
    return math.degrees(math.acos(min(1.0, c)))


DEEP_MM = 0.1  # a truth tip whose own penetration differs from the 0.05 mm offset by more than this is "deep" (or shallow)
ELONG_MIN = 1.2  # direction is compared only where the truth loop is at least this elongated


def score(built_dir, work, ids, off_dir=None, truth_dir="."):
    rows = []
    for i in ids:
        tp = os.path.join(truth_dir, f"{i}.json"); bp = os.path.join(built_dir, f"{i}.json")
        if not (os.path.exists(tp) and os.path.exists(bp)) or os.path.getsize(bp) == 0:
            continue
        t = json.load(open(tp)); b = json.load(open(bp))
        lay = {}
        lp = os.path.join(work, f"{i}.layout.json")
        if os.path.exists(lp):
            lay = json.load(open(lp))
        truth = t["contacts"]; tips = b["tips"]
        m = match(truth, tips)
        extra_in = inside_extra(truth, tips, m)
        osz = None
        if off_dir and not OLD:
            op = os.path.join(off_dir, f"{i}.json")
            if os.path.exists(op) and os.path.getsize(op) > 0:
                osz = off_sizes(tips, json.load(open(op))["tips"])
        sizes = []
        ext = Counter()
        ext["extra_inside"] = len(extra_in)
        offsets, elong_err, dir_err = [], [], []
        for ci, tj in m.items():
            fb = key_of(tips[tj])[1]
            ext["fallback_pairs"] += fb
            # A flush tip has no loop to size it by: it counts for missing, not for size.
            if truth[ci].get("flush"):
                continue
            deep = abs(truth[ci].get("depth", 0.05) - 0.05) > DEEP_MM
            a = tips[tj].get("area05")
            if osz is not None:
                ext["sized_offset_build"] += osz[tj] is not None
                a = osz[tj] if osz[tj] is not None else a
            if a:
                s = abs(eq_d(a) - truth[ci]["d"]) / truth[ci]["d"]
                sizes.append(s)
                k = "deep" if deep else "near"
                ext[k + "_sized"] += 1; ext[k + "_w10"] += s <= 0.10; ext[k + "_w20"] += s <= 0.20
            lp, sh = tips[tj].get("loop05"), truth[ci].get("shape")
            if lp:
                offsets.append(math.dist(truth[ci]["at"], lp["centre"]))
            if lp and sh:
                elong_err.append(abs(lp["elong"] / sh["elong"] - 1))
                if sh["elong"] > ELONG_MIN:
                    dir_err.append(line_angle(sh["major"], lp["major"]))
        for k, c in enumerate(truth):
            if not c.get("flush") and abs(c.get("depth", 0.05) - 0.05) > DEEP_MM:
                ext["deep_truth"] += 1
                ext["deep_missing"] += k not in m
            elif not c.get("flush"):
                ext["near_truth"] += 1
        ext["tips_no_loop"] = sum(1 for t in tips if not t.get("loop05"))
        clips = [p for p in b["patches"] if p["clip"]]
        # Where the original itself passes through the model, near enough to be the same place.
        orig_through = [l["at"] for th in t["throughs"] for l in th["loops"]]
        same = [p for p in clips if any(math.dist(p["at"], q) <= 1.0 for q in orig_through)]
        c = b.get("counts", {})
        rows.append({
            "id": i, "fb": bool(lay.get("fell_back")), "truth": len(truth), "tips": len(tips), "matched": len(m),
            "missing": len(truth) - len(m), "extra": len(tips) - len(m) - len(extra_in),
            "w10": sum(1 for s in sizes if s <= 0.10), "w20": sum(1 for s in sizes if s <= 0.20),
            "clip_area": sum(p["area"] for p in clips), "clip_n": len(clips),
            "clip_orig_too": sum(p["area"] for p in same),
            "clip_through": sum(1 for p in clips if p["n"] >= 2), "joints": c.get("joint", 0),
            "missing_list": [truth[k]["at"] for k in range(len(truth)) if k not in m],
            "ext": dict(ext), "offsets": offsets, "elong_err": elong_err, "dir_err": dir_err,
        })
    return rows


def summary(rows):
    if not rows:
        return None
    s = Counter()
    for r in rows:
        for k in ("truth", "tips", "matched", "missing", "extra", "w10", "w20", "clip_n", "clip_through", "joints"):
            s[k] += r[k]
        s["clip_area"] += r["clip_area"]
        s["clip_orig_too"] += r["clip_orig_too"]
    shares = sorted((r["missing"] / r["truth"] if r["truth"] else 0) for r in rows)
    dec = shares[-max(1, len(shares) // 10):]
    T = max(1, s["truth"])
    return {
        "models": len(rows), "truth": s["truth"], "missing": s["missing"], "missing_pct": 100 * s["missing"] / T,
        "worst_decile": sum(dec) / len(dec), "w10": s["w10"] / T, "w20": s["w20"] / T,
        "clip_mm2": round(s["clip_area"], 1), "clip_orig_too": round(s["clip_orig_too"], 1), "clip_patches": s["clip_n"], "clip_through": s["clip_through"],
        "extra": s["extra"], "extra_pct": 100 * s["extra"] / T, "jps": s["joints"] / max(1, s["tips"]),
        "recall": s["matched"] / T, "precision": s["matched"] / max(1, s["tips"]),
        **shape_summary(rows),
    }


def shape_summary(rows):
    """measure: loop shape, centre offset and deep-tip metrics, apart from w10/w20."""
    e = Counter()
    off, el, di = [], [], []
    for r in rows:
        for k, v in (r.get("ext") or {}).items():
            e[k] += v
        off += r.get("offsets", []); el += r.get("elong_err", []); di += r.get("dir_err", [])
    med = lambda x: sorted(x)[len(x) // 2] if x else float("nan")
    share = lambda x, f: sum(1 for v in x if f(v)) / len(x) if x else float("nan")
    return {
        "fallback_pairs": e["fallback_pairs"], "tips_no_loop": e["tips_no_loop"],
        "centre_offset_median": med(off), "centre_within_015": share(off, lambda v: v <= 0.15), "centre_n": len(off),
        "elong_w10": share(el, lambda v: v <= 0.10), "elong_w20": share(el, lambda v: v <= 0.20), "elong_n": len(el),
        "dir_w15": share(di, lambda v: v <= 15.0), "dir_n": len(di),
        "deep_truth": e["deep_truth"], "deep_missing": e["deep_missing"],
        "deep_w10": e["deep_w10"] / max(1, e["deep_truth"]), "deep_w20": e["deep_w20"] / max(1, e["deep_truth"]),
        "near_truth": e["near_truth"], "near_w10": e["near_w10"] / max(1, e["near_truth"]), "near_w20": e["near_w20"] / max(1, e["near_truth"]),
    }


