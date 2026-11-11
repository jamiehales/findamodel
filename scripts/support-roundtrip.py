#!/usr/bin/env python3
"""Support reader round trip: the backend's VOXL export of pre-supported models, built by DragonFruit, measured
against the original supports. Not part of `cargo test`; see backend/tools/support-reader/README.md.

usage:
  support-roundtrip.py run LIST OUT [--jobs N] [--truth-dir DIR] [--no-contacts]
      export (support-reader voxl) -> DragonFruit build at offset 0 and at its own 0.05 mm tip offset ->
      measures -> OUT/per_model.tsv, OUT/per/<id>.json, OUT/summary.json
  support-roundtrip.py measure LIST OUT --build0 DIR --build-off DIR --class-dir DIR [--jobs N] [--truth-dir DIR]
      the measures and tables only, on builds made elsewhere (for example the prototype's): DIR holds
      <id>.df_support.stl, .df_labels.bin, .df.json (and .layout.json); --class-dir holds <id>.class.json
  support-roundtrip.py table OUT
      the tables again from the measures already in OUT

LIST is a tab-separated file, one model per line: id, model STL, support STL, raft height (mm), and optionally a
touch-rule contacts JSON ({"contacts": [...]}) that replaces the reader's own contacts (prototype comparisons;
--no-contacts ignores that column). Paths are relative to the list file. STLs are binary and Z-up. Lines starting
with # are skipped.

Environment:
  DF_REPO          a DragonFruit checkout with node_modules (required for run)
  SUPPORT_READER   the support-reader-tool binary (default: target-tool/release/ or the tool's own target/release/)
  SPHERE_TIPS      passed to the export: 1 writes ball tips as DragonFruit sphere tips (the backend default is off)
  TIP_OFFSET_MM    the offset build's tip offset (default 0.05, DragonFruit's own)
Existing outputs are kept (delete OUT, or the step's directory, to redo a step).
"""
import importlib.util, json, os, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOOL_DIR = os.path.join(REPO, "backend", "tools", "support-reader")
DFNODE = os.path.join(TOOL_DIR, "dragonfruit", "dfnode.sh")
DF_BUILD = os.path.join(TOOL_DIR, "dragonfruit", "df_build.ts")
_spec = importlib.util.spec_from_file_location("roundtrip_score", os.path.join(os.path.dirname(os.path.abspath(__file__)), "support-roundtrip-score.py"))
SC = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(SC)
ENV1 = {**os.environ, "RAYON_NUM_THREADS": "1"}


def tool():
    if os.environ.get("SUPPORT_READER"):
        return os.environ["SUPPORT_READER"]
    for p in (os.path.join(REPO, "target-tool", "release", "support-reader-tool"), os.path.join(TOOL_DIR, "target", "release", "support-reader-tool")):
        if os.path.exists(p):
            return p
    sys.exit("support-reader-tool not built (see backend/tools/support-reader/README.md) and SUPPORT_READER not set")


def read_list(path, use_contacts=True):
    base = os.path.dirname(os.path.abspath(path))
    rows = []
    for line in open(path):
        if not line.strip() or line.startswith("#"):
            continue
        f = line.rstrip("\n").split("\t")
        ab = lambda p: p if os.path.isabs(p) else os.path.join(base, p)
        rows.append({"id": f[0], "model": ab(f[1]), "support": ab(f[2]), "raft": float(f[3]) if len(f) > 3 and f[3] else 3.0,
                     "contacts": ab(f[4]) if use_contacts and len(f) > 4 and f[4] else None})
    return rows


def link(src, dst):
    if os.path.lexists(dst):
        os.remove(dst)
    os.symlink(os.path.abspath(src), dst)


def done(p):
    return os.path.exists(p) and os.path.getsize(p) > 0


def pmap(fn, items, jobs):
    with ThreadPoolExecutor(jobs) as ex:
        return list(ex.map(fn, items))


def export(out, rows, jobs):
    """The backend's reader and VOXL writer: <id>.voxl, .layout.json, .class.json, .export.json (timings)."""
    work = os.path.join(out, "work")

    def one(r):
        i = r["id"]
        link(r["model"], os.path.join(work, f"{i}.orig_model.stl"))
        link(r["support"], os.path.join(work, f"{i}.orig_support.stl"))
        if done(os.path.join(work, f"{i}.export.json")):
            return
        cmd = [tool(), "voxl", r["model"], r["support"], os.path.join(work, f"{i}.voxl"), str(r["raft"])]
        if r["contacts"]:
            cmd.append(r["contacts"])
        cmd += ["--class", os.path.join(work, i)]
        t0 = time.time()
        p = subprocess.run(cmd, capture_output=True, text=True)
        if p.returncode != 0:
            open(os.path.join(work, f"{i}.export.err"), "w").write(p.stderr)
            print(i, "export failed", flush=True)
            return
        rec = json.loads(p.stdout.strip().splitlines()[-1])
        rec["wall_s"] = time.time() - t0
        rec["contacts_given"] = bool(r["contacts"])
        json.dump(rec, open(os.path.join(work, f"{i}.export.json"), "w"))

    os.makedirs(work, exist_ok=True)
    pmap(one, rows, jobs)


def build(out, rows, jobs):
    """DragonFruit builds of every VOXL: build0 at offset 0, build_off at DragonFruit's own tip offset."""
    work = os.path.join(out, "work")
    for name, offset in (("build0", None), ("build_off", os.environ.get("TIP_OFFSET_MM", "0.05"))):
        d = os.path.join(out, name)
        os.makedirs(d, exist_ok=True)
        todo = []
        for r in rows:
            i = r["id"]
            if not done(os.path.join(work, f"{i}.voxl")) or done(os.path.join(d, f"{i}.df.json")):
                continue
            link(os.path.join(work, f"{i}.voxl"), os.path.join(d, f"{i}.voxl"))
            todo.append(i)
        parts = [todo[k::jobs] for k in range(jobs) if todo[k::jobs]]
        env = {k: v for k, v in os.environ.items() if k != "TIP_OFFSET_MM"}
        if offset is not None:
            env["TIP_OFFSET_MM"] = offset
        if not os.environ.get("DF_REPO") and parts:
            sys.exit("DF_REPO is not set (see backend/tools/support-reader/README.md)")

        def one(part):
            with open(os.path.join(d, f"build.{part[0]}.log"), "w") as log:
                subprocess.run(["bash", DFNODE, DF_BUILD, os.path.abspath(d), *part], stdout=log, stderr=subprocess.STDOUT, env=env)

        pmap(one, parts, jobs)
        for i in todo:
            m = os.path.join(d, f"{i}.df_model.stl")
            if os.path.exists(m):
                os.remove(m)
        print(f"{name}: built {sum(done(os.path.join(d, r['id'] + '.df_support.stl')) for r in rows)} of {len(rows)}", flush=True)


def adopt(out, rows, build0, build_off, class_dir):
    """measure mode: builds and classifications made elsewhere, linked into OUT."""
    for name, src in (("build0", build0), ("build_off", build_off)):
        d = os.path.join(out, name)
        os.makedirs(d, exist_ok=True)
        for r in rows:
            for ext in ("df_support.stl", "df_labels.bin", "df.json", "layout.json"):
                s = os.path.join(src, f"{r['id']}.{ext}")
                if os.path.exists(s):
                    link(s, os.path.join(d, f"{r['id']}.{ext}"))
    work = os.path.join(out, "work")
    os.makedirs(work, exist_ok=True)
    for r in rows:
        link(r["model"], os.path.join(work, f"{r['id']}.orig_model.stl"))
        link(r["support"], os.path.join(work, f"{r['id']}.orig_support.stl"))
        for ext in ("class.json", "layout.json"):
            s = os.path.join(class_dir, f"{r['id']}.{ext}")
            if os.path.exists(s):
                link(s, os.path.join(work, f"{r['id']}.{ext}"))


def measure(out, rows, jobs, truth_dir=None):
    """Truth (touch orig) unless given, overlap of the offset 0 build, touch built on both builds."""
    work = os.path.join(out, "work")
    T = tool()
    tdir = truth_dir or os.path.join(out, "truth")
    for d in ("truth", "ovl", "touch0", "touch_off"):
        os.makedirs(os.path.join(out, d), exist_ok=True)

    def run_to(cmd, path, env=ENV1):
        if done(path):
            return
        with open(path + ".tmp", "w") as f:
            p = subprocess.run(cmd, stdout=f, stderr=subprocess.DEVNULL, env=env)
        if p.returncode == 0:
            os.replace(path + ".tmp", path)
        else:
            os.replace(path + ".tmp", path + ".err")

    def truth(r):
        if not truth_dir:
            run_to([T, "touch", "orig", work, r["id"]], os.path.join(tdir, f"{r['id']}.json"))

    def ovl(r):
        i = r["id"]
        b = os.path.join(out, "build0", f"{i}.df_support.stl")
        cp = os.path.join(work, f"{i}.class.json")
        if not os.path.exists(b) or not os.path.exists(cp):
            return
        # The raft band: below the classified raft's top (as the prototype's ovl.py), none without a raft.
        raft = json.load(open(cp))["raft_top"]
        run_to([T, "overlap", r["support"], b, r["model"], "--raft", str(raft + 0.01 if raft > 0 else -1)], os.path.join(out, "ovl", f"{i}.json"))

    def touch(r, name, sub, pen):
        i = r["id"]
        d = os.path.join(out, name)
        tp = os.path.join(tdir, f"{i}.json")
        if not os.path.exists(os.path.join(d, f"{i}.df_support.stl")) or not done(tp):
            return
        link(r["model"], os.path.join(d, f"{i}.orig_model.stl"))
        env = dict(ENV1)
        env.pop("TOUCH_PEN", None)
        if pen is not None:
            env["TOUCH_PEN"] = pen
        run_to([T, "touch", "built", d, i, tp], os.path.join(out, sub, f"{i}.json"), env)

    pmap(truth, rows, jobs)
    tasks = [(ovl, (r,)) for r in rows] + [(touch, (r, "build0", "touch0", None)) for r in rows] + [(touch, (r, "build_off", "touch_off", "0")) for r in rows]
    # The slowest models first.
    tasks.sort(key=lambda t: -os.path.getsize(t[1][0]["support"]))
    pmap(lambda t: t[0](*t[1]), tasks, jobs)


def table(out, truth_dir=None):
    """Per model and pooled: overlap (headline: raft band excluded; outside_model as well), touch-rule tips and clips."""
    saved = os.path.join(out, "truth_dir.txt")
    tdir = truth_dir or (open(saved).read().strip() if os.path.exists(saved) else os.path.join(out, "truth"))
    ids = [l.split("\t")[0] for l in open(os.path.join(out, "ids.txt")).read().split("\n") if l]
    os.makedirs(os.path.join(out, "per"), exist_ok=True)
    per, tsv = {}, []
    cols = ["id", "coverage", "fit", "iou", "out_coverage", "out_fit", "out_iou", "truth", "tips", "missing", "extra", "w10", "w20", "sized", "clip_mm2", "read_s", "write_s"]
    pool = {"headline": [0.0, 0.0, 0.0], "outside_model": [0.0, 0.0, 0.0]}
    rows_all = []
    for i in ids:
        rec = {"id": i}
        ex = os.path.join(out, "work", f"{i}.export.json")
        if os.path.exists(ex):
            rec["export"] = json.load(open(ex))
        op = os.path.join(out, "ovl", f"{i}.json")
        if done(op):
            o = json.load(open(op))
            rec["overlap"] = {k: o[k] for k in ("headline", "outside_model", "inside_model", "raft_band", "raft")}
            for k in pool:
                pool[k][0] += o[k]["a"]; pool[k][1] += o[k]["b"]; pool[k][2] += o[k]["both"]
        rows = SC.score(os.path.join(out, "touch0"), os.path.join(out, "build0"), [i], os.path.join(out, "touch_off"), tdir)
        if rows:
            r = rows[0]
            rows_all.append(r)
            rec["touch"] = {k: r[k] for k in ("truth", "tips", "matched", "missing", "extra", "w10", "w20", "clip_area", "clip_n", "clip_through", "missing_list")}
            rec["touch"]["sized"] = r["ext"].get("near_sized", 0) + r["ext"].get("deep_sized", 0)
            rec["touch"]["sized_offset_build"] = r["ext"].get("sized_offset_build", 0)
            rec["touch"]["extra_inside"] = r["ext"].get("extra_inside", 0)
        json.dump(rec, open(os.path.join(out, "per", f"{i}.json"), "w"), indent=1)
        per[i] = rec
        h, om, t, e = (rec.get("overlap") or {}).get("headline", {}), (rec.get("overlap") or {}).get("outside_model", {}), rec.get("touch") or {}, rec.get("export") or {}
        vals = [i, h.get("coverage"), h.get("fit"), h.get("iou"), om.get("coverage"), om.get("fit"), om.get("iou"), t.get("truth"), t.get("tips"), t.get("missing"), t.get("extra"),
                t.get("w10"), t.get("w20"), t.get("sized"), t.get("clip_area"), e.get("read_s"), e.get("write_s")]
        tsv.append("\t".join("" if v is None else (f"{v:.4f}" if isinstance(v, float) else str(v)) for v in vals))
    open(os.path.join(out, "per_model.tsv"), "w").write("\t".join(cols) + "\n" + "\n".join(tsv) + "\n")
    stat = lambda v: {"coverage": v[2] / v[0] if v[0] else None, "fit": v[2] / v[1] if v[1] else None, "iou": v[2] / (v[0] + v[1] - v[2]) if v[0] + v[1] - v[2] > 0 else None, "a": v[0], "b": v[1], "both": v[2]}
    s = SC.summary(rows_all) or {}
    summary = {"models": len(ids), "overlap_models": sum(1 for r in per.values() if "overlap" in r), "touch_models": len(rows_all), "truth_dir": tdir,
               "overlap": {k: stat(v) for k, v in pool.items()},
               "touch": {k: s.get(k) for k in ("truth", "missing", "missing_pct", "extra", "extra_pct", "w10", "w20", "clip_mm2", "clip_patches", "recall", "precision")},
               "export_s": {k: sum((r.get("export") or {}).get(k, 0) for r in per.values()) for k in ("read_s", "write_s")}}
    json.dump(summary, open(os.path.join(out, "summary.json"), "w"), indent=1)
    h, t = summary["overlap"]["headline"], summary["touch"]
    if h["coverage"] is not None and t.get("truth"):
        print(f"{out}: {summary['overlap_models']} models: coverage {h['coverage']:.4f} fit {h['fit']:.4f} IoU {h['iou']:.4f} | truth {t['truth']} missing {t['missing']} ({t['missing_pct']:.2f}%) "
              f"extra {t['extra']} ({t['extra_pct']:.2f}%) w10 {100 * t['w10']:.1f}% w20 {100 * t['w20']:.1f}% clip {t['clip_mm2']} mm2")


def main():
    a = sys.argv[1:]
    if not a or a[0] not in ("run", "measure", "table"):
        sys.exit(__doc__)
    opt = lambda k, d=None: a[a.index(k) + 1] if k in a else d
    mode, out = a[0], os.path.abspath(a[2] if a[0] != "table" else a[1])
    jobs = int(opt("--jobs", "3"))
    truth_dir = opt("--truth-dir")
    if mode == "table":
        return table(out, truth_dir)
    rows = read_list(a[1], "--no-contacts" not in a)
    os.makedirs(out, exist_ok=True)
    open(os.path.join(out, "ids.txt"), "w").write("".join(r["id"] + "\n" for r in rows))
    open(os.path.join(out, "truth_dir.txt"), "w").write(os.path.abspath(truth_dir) if truth_dir else os.path.join(out, "truth"))
    if mode == "run":
        export(out, rows, jobs)
        build(out, rows, jobs)
    else:
        adopt(out, rows, opt("--build0"), opt("--build-off"), opt("--class-dir"))
    measure(out, rows, jobs, truth_dir)
    table(out, truth_dir)


if __name__ == "__main__":
    main()
