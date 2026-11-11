# support-reader (dev tool)

Runs the backend's support reader (`backend/src/geometry/support_reader`) and VOXL writer
(`backend/src/geometry/voxl.rs`) on STL files, and holds the measuring tools and the DragonFruit
build driver of the support reader round trip. It is not part of `cargo test` and not a member
of the repository's workspace.

## Build

```sh
CARGO_TARGET_DIR=target-tool cargo build --release --manifest-path backend/tools/support-reader/Cargo.toml
# binary: target-tool/release/support-reader-tool
```

It depends on the backend crate by path, so it builds whatever the backend currently is.

## Commands

All STL files are binary and Z-up. Reader commands print their run time on stderr.

| Command | What it does |
| --- | --- |
| `contacts MODEL.stl SUPPORT.stl OUT.json` | Touch-rule contacts of the supports on the model (raft band 3.5 mm). |
| `pieces MODEL.stl SUPPORT.stl CONTACTS.json OUT.json` | The support mesh split into pieces, contacts taken from `{"contacts": [{at, d}]}`. |
| `read MODEL.stl SUPPORT.stl OUT_PREFIX [RAFT_MM]` | The whole read: writes `OUT_PREFIX.layout.json` and `OUT_PREFIX.class.json`. |
| `classify MODEL.stl SUPPORT.stl CONTACTS.json OUT_PREFIX` | As `read`, with the contacts taken from a JSON file. |
| `voxl MODEL.stl SUPPORT.stl OUT.voxl [RAFT_MM] [CONTACTS.json] [--class OUT_PREFIX]` | The export as the app writes it: the model without its supports, with the read supports as DragonFruit supports. With `CONTACTS.json` the contacts come from that file. `--class` also writes the layout and classification JSON. Prints one JSON line: `read_s` (contacts, pieces and classification), `write_s` (VOXL write) and `sphere_tips`. `SPHERE_TIPS=1` writes ball tips as DragonFruit sphere tips (they need a DragonFruit with `sphereDepthMm`); the backend default is off. `LAYOUT_DEBUG=PATH` also writes the layout as the writer takes it (Rust debug format). |
| `export MODEL.stl SUPPORT.stl [RAFT_MM]` | A plate VOXL export's per-model work on the model and its supports joined into one mesh, as the app loads it: `export_s` (support split and read), `write_s` (writing its export entry of `entry_bytes`), then `cached_s` (the export read back from the entry, the body split off by its mask), and `same` (the cached export equals the read one). One JSON line. |
| `overlap A.stl B.stl M.stl [--raft Z] [--pitch 0.025] [--labels A.u8 --names a,b] [--diff OUT.json] [--axis all\|x\|y\|z]` | Volume overlap of the original support mesh A and the built one B, split by inside or outside the model M and by the raft band `z < Z`. One JSON object on stdout. |
| `touch orig WORK_DIR ID` | The original's own touch-rule contacts (truth): reads `WORK_DIR/ID.orig_model.stl` and `.orig_support.stl`, prints contacts, pass-throughs and buried shells as one JSON line. |
| `touch built WORK_DIR ID TRUTH.json` | The built mesh's tips and inside-the-model patches: reads `ID.orig_model.stl`, `ID.df_support.stl`, `ID.df_labels.bin` and `ID.df.json` (a DragonFruit build). `TOUCH_PEN` (default 0.05 mm) is how far each built cone is run on into the model before its loop is taken; use `TOUCH_PEN=0` on a build made at DragonFruit's own 0.05 mm tip offset. A sphere tip's ball near the contact counts as part of the tip, and the tip is sized by the larger of the ball's and the cone's loop. A twig's disk is sized by the largest loop centred within it among its disk, the disk's ball and the shaft's end joint (shells of their own that each cross the surface when the disk is short). |
| `touch selftest` | The touch rule's own scene checks. |

`overlap` and `touch` are the prototype's measuring tools copied unchanged apart from their entry
points (the sources say where they came from) and, in `touch`, ordered maps where the prototype
iterated hash maps: its winding-number cells and the loop chaining are now walked in key order, as
the backend's contact reader does, so `touch` gives the same answer on every run. Their numbers equal
the prototype's wherever the prototype's own runs agreed.

## DragonFruit setup

The build driver `dragonfruit/df_build.ts` runs DragonFruit's own support builder through
`dragonfruit/dfnode.sh`, which needs `DF_REPO` set to a DragonFruit checkout. The checkout is only
read. It needs:

- its node dependencies installed (`yarn install` or `npm install` in the checkout, so that
  `node_modules` holds `tsx` and `three`);
- its generated registries in place (in the checkout: `node scripts/generate-plugin-registry.mjs`,
  `node scripts/generate-builtin-simple-plugins.mjs`, `node scripts/generate-support-registrations.mjs`,
  `node scripts/generate-support-renderers.mjs`, which a normal dev setup has already run);
- sphere-tip support (`sphereDepthMm` on contact cones) if VOXLs are written with `SPHERE_TIPS=1`.

```sh
export DF_REPO=/path/to/dragonfruit
bash backend/tools/support-reader/dragonfruit/dfnode.sh backend/tools/support-reader/dragonfruit/df_build.ts /abs/dir ID1 ID2
TIP_OFFSET_MM=0.05 bash backend/tools/support-reader/dragonfruit/dfnode.sh backend/tools/support-reader/dragonfruit/df_build.ts /abs/dir ID1
```

It reads `DIR/ID.voxl` and writes `ID.df_support.stl` (the support mesh DragonFruit would slice, raft
off), `ID.df_labels.bin` (the support entity of each triangle, int32), `ID.df.json` (how DragonFruit
resolved each entity) and `ID.df_model.stl`. Without `TIP_OFFSET_MM` the build is at tip offset 0;
with it, a generic printer and material profile with that manual tip offset are made active, as the
app does (DragonFruit's own default is 0.05 mm). Pass absolute directories (the script runs from the
checkout).

## Round trip

`scripts/support-roundtrip.py` runs export, both DragonFruit builds and the measures for a list of
models, then writes the tables. The list is tab separated: id, model STL, support STL, raft height
(mm), and optionally a touch-rule contacts JSON that replaces the reader's own contacts (for
prototype comparisons; `--no-contacts` ignores the column).

```sh
export DF_REPO=/path/to/dragonfruit
python3 scripts/support-roundtrip.py run models.tsv out/ --jobs 3
SPHERE_TIPS=1 python3 scripts/support-roundtrip.py run models.tsv out_sphere/ --jobs 3
```

Steps (each keeps what is already there, so a run can be resumed):

1. `work/`: `support-reader-tool voxl ... --class` per model (`ID.voxl`, `.layout.json`, `.class.json`,
   `.export.json` with the read and write times).
2. `build0/`: DragonFruit build at tip offset 0. `build_off/`: the same VOXL at DragonFruit's own
   0.05 mm tip offset (`TIP_OFFSET_MM` changes it).
3. `truth/`: `touch orig` on the original meshes (or `--truth-dir DIR` with `ID.json` files made
   before).
4. `ovl/`: `overlap` of the original support against `build0`, raft band below the classified raft's
   top + 0.01 mm. `touch0/`: `touch built` on `build0`. `touch_off/`: `touch built` with
   `TOUCH_PEN=0` on `build_off`.
5. `per/ID.json`, `per_model.tsv` and `summary.json` (pooled). `table OUT` redoes only this step.

`measure LIST OUT --build0 DIR --build-off DIR --class-dir DIR` does steps 3 to 5 on builds made
elsewhere (for example the prototype's), so both sides of a comparison use the same measuring code.

Real models: the list points at the split model and support STLs of each model (for example the
`orig_model.stl` and `orig_support.stl` the support separation writes) with the model's raft height.

Synthetic cases: until the Rust generator dumps its cases, use the Python suite's files,
`synthetic/cases/ID.model.stl` and `ID.support.stl`, raft 3.0:

```sh
while read id; do printf '%s\t%s\t%s\t3.0\n' $id cases/$id.model.stl cases/$id.support.stl; done < ids.txt > syn.tsv
```

The synthetic suite's own stages (3 build, 4a clips, 4b loops) are scored by the suite's `score.py`
and harness, which are not in this repository; place `build0` as its `mesh/` and `build_off` as its
`mesh_tipoffset/` to score them.

## Measures

- **Overlap** (`ovl/`, headline): rays along each axis on a jittered 0.025 mm lattice, exact solid
  intervals per ray (winding number), the three axes averaged. A is the original support, B the
  DragonFruit build. Coverage = both / A (how much of the original was rebuilt), fit = both / B (how
  much of the build is where the original was), IoU = both / (A + B - both). The headline leaves out
  the raft band and counts inside and outside the model; `outside_model` counts only outside it.
  Pooled values sum the volumes over models.
- **Tips** (touch rule, `touch0/` against `truth/`): each original contact pairs one to one with a
  built tip whose loop centre at the 0.05 mm offset is within 0.5 mm and within half the two contact
  diameters. Missing = truth contacts with no built tip; extra = built tips with no contact, not
  counting a leftover tip whose loop centre lies inside a contact's loop. Percentages are of the truth
  contacts.
- **Tip size within 10% / 20%**: the matched tip's loop on the offset build (`touch_off/`, as built),
  as an equivalent diameter, within 10% (20%) of the truth contact's; shares of the truth contacts.
- **Clip mm2**: area of the build's patches inside the model that are not tips.
- **Timings**: `read_s` and `write_s` per model from the export (release build, one process).
