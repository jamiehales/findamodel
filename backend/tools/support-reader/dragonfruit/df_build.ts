/**
 * DragonFruit side of the support reader round trip: opens findamodel VOXLs the way the app does and
 * builds the support mesh it would slice, with DragonFruit's own code. Copied from the prototype's
 * build drivers (df_build.ts for the offset 0 build, df_build_lean.ts for the tip offset build),
 * merged into one file; the build itself is unchanged.
 *
 *   - the VOXL is parsed with the app's codec (`parseVoxlAuto`);
 *   - each model's MESH is placed as the app/CLI place it (bbox centre, then the transform);
 *   - the supports are opened with `mergeFromImportFormat` (the app's VOXL open path: id
 *     isolation, import defaults, knot normalisation);
 *   - the print mesh is `buildSupportAndRaftWorldTriangles` (what `buildSolidSliceMeshForWasm`
 *     appends after the models), raft off.
 *
 * Tip offset: without TIP_OFFSET_MM no printer or material profile is active and the builder runs
 * at offset 0 (the prototype's df_build.ts). With TIP_OFFSET_MM set (DragonFruit's own default is
 * 0.05) a generic printer and material profile are made active with that manual tip offset, as the
 * app applies it (the prototype's df_build_lean.ts).
 *
 * Every triangle is labelled with the support entity (or root) that emitted it, by watching
 * which entity the builder reads next. Per entity the script records how DragonFruit
 * resolved it (contacts, sockets, knots, drawn diameters).
 *
 * Usage (via dfnode.sh): df_build.ts <dir> <id>...
 * Reads <dir>/<id>.voxl, writes <dir>/<id>.df_support.stl, .df_model.stl, .df_labels.bin
 * (int32 per support triangle, entity index), .df.json.
 */
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import * as THREE from 'three';
import { parseVoxlAuto } from '@/features/scene/voxl/codec';
import { mergeFromImportFormat, getSnapshot, resetStore } from '@/supports/state';
import { setRaftSettings } from '@/supports/Rafts/Crenelated/RaftState';
import { DEFAULT_RAFT_SETTINGS } from '@/supports/Rafts/Crenelated/RaftDefaults';
import { SUPPORT_TYPES } from '@/supports/supportTypeRegistry';
import { buildSupportAndRaftWorldTriangles } from '@/features/slicing/rasterLayerZipExport';
import { getFinalSocketPosition } from '@/supports/SupportPrimitives/ContactCone/contactConeUtils';
import { resolveSegmentEndpoints } from '@/supports/SupportPrimitives/Knot/segmentEndpoints';
import { quaternionFromGlobalEuler } from '@/utils/rotation';
import * as Profiles from '@/features/profiles/profileStore';
import { globalPenetrationMm } from '@/supports/exportGeometry/helpers';

/** The slicer's tip offset as the app applies it. Without an active printer and material profile the
 * builder runs at 0, which the app never does. */
function setTipOffset(mm: number): void {
  const presets: any[] = Profiles.getAvailablePrinterPresets();
  const pid = Profiles.addPrinterProfileFromPreset(presets.find((p) => p.presetId === 'generic-machine')?.presetId ?? presets[0].presetId);
  Profiles.setActivePrinterProfile(pid);
  let m: any = Profiles.getActiveMaterialProfile();
  if (!m) {
    const mid = (Profiles as any).addMaterialProfile(pid);
    Profiles.setActiveMaterialProfile(mid);
    m = Profiles.getActiveMaterialProfile();
  }
  Profiles.updateMaterialProfile(m.id, { antiAliasingSettings: { ...m.antiAliasingSettings, enableCustomSettings: true, enableOverride: true, tipOffsetMode: 'manual', tipOffsetMm: mm } } as any);
  const got = globalPenetrationMm();
  if (Math.abs(got - mm) > 1e-6) throw new Error(`tip offset ${got}, wanted ${mm}`);
  console.error(`tip offset ${got} mm`);
}

type V = { x: number; y: number; z: number };
const arr = (p: V | undefined | null) => (p ? [p.x, p.y, p.z] : null);

function parseBinaryStl(bytes: Uint8Array): Float32Array {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = view.getUint32(80, true);
  const out = new Float32Array(count * 9);
  for (let i = 0; i < count; i++) {
    const base = 84 + i * 50 + 12;
    for (let k = 0; k < 9; k++) out[i * 9 + k] = view.getFloat32(base + k * 4, true);
  }
  return out;
}

function writeBinaryStl(path: string, positions: Float32Array | number[]): void {
  const count = positions.length / 9;
  const buf = Buffer.alloc(84 + count * 50);
  buf.writeUInt32LE(count, 80);
  for (let i = 0; i < count; i++) {
    const base = 84 + i * 50 + 12;
    for (let k = 0; k < 9; k++) buf.writeFloatLE(positions[i * 9 + k], base + k * 4);
  }
  writeFileSync(path, buf);
}

/** As the CLI's applyVoxlTransform / the GUI: centre on the bbox, then compose the transform. */
function placeModel(positions: Float32Array, t: { position: V; rotation: V; scale: V }): void {
  let min = [Infinity, Infinity, Infinity];
  let max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < positions.length; i += 3) {
    for (let k = 0; k < 3; k++) {
      min[k] = Math.min(min[k], positions[i + k]);
      max[k] = Math.max(max[k], positions[i + k]);
    }
  }
  const c = [0, 1, 2].map((k) => (min[k] + max[k]) / 2);
  const m = new THREE.Matrix4().compose(
    new THREE.Vector3(t.position.x, t.position.y, t.position.z),
    quaternionFromGlobalEuler(t.rotation),
    new THREE.Vector3(t.scale.x, t.scale.y, t.scale.z),
  );
  const v = new THREE.Vector3();
  for (let i = 0; i < positions.length; i += 3) {
    v.set(positions[i] - c[0], positions[i + 1] - c[1], positions[i + 2] - c[2]).applyMatrix4(m);
    positions[i] = v.x;
    positions[i + 1] = v.y;
    positions[i + 2] = v.z;
  }
}

// Counts world triangles as the builder pushes them into its output array.
let pushed = 0;
const origPush = Array.prototype.push;
function watchPushes(on: boolean): void {
  if (!on) {
    Array.prototype.push = origPush;
    return;
  }
  Array.prototype.push = function (this: unknown[], ...items: unknown[]) {
    if (items.length === 1) {
      const t = items[0] as { zMin?: number; ax?: number } | null;
      if (t && typeof t === 'object' && t.zMin !== undefined && t.ax !== undefined) pushed++;
    }
    return origPush.apply(this, items);
  } as typeof Array.prototype.push;
}

async function buildOne(dir: string, id: string): Promise<void> {
  const t0 = performance.now();
  const voxlPath = join(dir, `${id}.voxl`);
  const parsed = await parseVoxlAuto(new Uint8Array(readFileSync(voxlPath)));
  const doc = parsed.document;
  const authored = doc.supports as any;

  // Models, placed.
  const modelPositions: Float32Array[] = [];
  for (const model of doc.models) {
    const bytes = parsed.meshBytes.get(model.id);
    if (!bytes) throw new Error(`no mesh bytes for ${model.id}`);
    const positions = parseBinaryStl(bytes);
    placeModel(positions, model.transform as any);
    modelPositions.push(positions);
  }
  const total = modelPositions.reduce((s, p) => s + p.length, 0);
  const allModel = new Float32Array(total);
  let off = 0;
  for (const p of modelPositions) {
    allModel.set(p, off);
    off += p.length;
  }
  writeBinaryStl(join(dir, `${id}.df_model.stl`), allModel);

  // Supports, opened as the app opens a VOXL.
  const warnings: string[] = [];
  const origWarn = console.warn;
  const origLog = console.log;
  console.warn = (...a: unknown[]) => warnings.push(a.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))).join(' '));
  console.log = () => {};
  resetStore();
  setRaftSettings({ ...DEFAULT_RAFT_SETTINGS, bottomMode: 'off' });
  try {
    mergeFromImportFormat(authored);
  } finally {
    console.log = origLog;
  }
  const state = getSnapshot() as any;

  // Label entities: wrap each in a proxy that notes when the builder starts reading it.
  type Entity = { type: string; id: string; start: number; tris: number; info: any };
  const entities: Entity[] = [];
  const marks: Array<[number, number]> = []; // [triangle count, entity index]
  const wrap = (type: string, collection: Record<string, any>) => {
    for (const [key, entity] of Object.entries(collection)) {
      const index = entities.length;
      entities.push({ type, id: key, start: -1, tris: 0, info: null });
      collection[key] = new Proxy(entity, {
        get(target, prop, receiver) {
          if (prop === 'modelId') marks.push([pushed, index]);
          return Reflect.get(target, prop, receiver);
        },
      });
    }
  };
  // Keep the unwrapped objects for reporting.
  const raw: Record<string, Record<string, any>> = {};
  raw.roots = { ...state.roots };
  wrap('root', state.roots);
  for (const d of SUPPORT_TYPES) {
    raw[d.location.key] = { ...(state[d.location.key] ?? {}) };
    wrap(d.location.key, state[d.location.key] ?? {});
  }

  const visible = new Set<string>(doc.models.map((m: any) => m.id));
  pushed = 0;
  watchPushes(true);
  let tris: any[];
  try {
    tris = buildSupportAndRaftWorldTriangles(visible, undefined, 0, []);
  } finally {
    watchPushes(false);
    console.warn = origWarn;
  }
  // Restore the plain objects in the store.
  for (const [key, coll] of Object.entries(raw)) Object.assign(state[key], coll);

  // Triangle labels: the last entity read before a triangle was pushed emitted it.
  const labels = new Int32Array(tris.length).fill(-1);
  marks.sort((a, b) => a[0] - b[0]);
  // Several reads at one count: the last one wins (reads with no triangles between).
  for (let i = 0; i < marks.length; i++) {
    const [at, entity] = marks[i];
    const next = i + 1 < marks.length ? marks[i + 1][0] : tris.length;
    for (let t = at; t < next; t++) labels[t] = entity;
  }
  for (let t = 0; t < labels.length; t++) {
    const e = labels[t];
    if (e >= 0) {
      if (entities[e].start < 0) entities[e].start = t;
      entities[e].tris++;
    }
  }
  const supportPositions = new Float32Array(tris.length * 9);
  tris.forEach((t, i) => {
    supportPositions.set([t.ax, t.ay, t.az, t.bx, t.by, t.bz, t.cx, t.cy, t.cz], i * 9);
  });
  writeBinaryStl(join(dir, `${id}.df_support.stl`), supportPositions);
  writeFileSync(join(dir, `${id}.df_labels.bin`), Buffer.from(labels.buffer));

  // How DragonFruit resolved each entity.
  const cone = (c: any) =>
    c && {
      pos: arr(c.pos),
      surfaceNormal: arr(c.surfaceNormal ?? c.normal),
      socket: arr(getFinalSocketPosition(c)),
      contact_d: c.profile?.contactDiameterMm,
      body_d: c.profile?.bodyDiameterMm,
    };
  const disk = (c: any) =>
    c && {
      pos: arr(c.pos),
      surfaceNormal: arr(c.surfaceNormal),
      contact_d: c.contactDiameterMm,
      standoff: c.diskLengthOverride,
    };
  const knots = state.knots as Record<string, any>;
  const knotInfo = (kid: string) => {
    const k = knots[kid];
    return k ? { pos: arr(k.pos), d: k.diameter, t: k.t, shaft: k.parentShaftId } : null;
  };
  const segEnds = (entity: any, hosts: { root?: any; hostKnot?: any }) =>
    (entity.segments ?? []).map((s: any, i: number) => {
      const e = resolveSegmentEndpoints(entity, s, i, hosts);
      return e ? { a: arr(e.start), b: arr(e.end), d: s.diameter } : null;
    });
  for (const e of entities) {
    const x = raw[e.type === 'root' ? 'roots' : e.type][e.id];
    switch (e.type) {
      case 'root':
        e.info = { pos: arr(x.transform?.pos), d: x.diameter, disk: x.diskHeight, cone: x.coneHeight };
        break;
      case 'trunks':
        e.info = { cone: cone(x.contactCone), segs: segEnds(x, { root: state.roots[x.rootId] }), rootOk: !!state.roots[x.rootId] };
        break;
      case 'branches':
        e.info = {
          cone: cone(x.contactCone),
          knot: knotInfo(x.parentKnotId),
          segs: segEnds(x, { hostKnot: knots[x.parentKnotId] }),
        };
        break;
      case 'twigs':
        e.info = { a: disk(x.contactDiskA), b: disk(x.contactDiskB), segs: segEnds(x, {}) };
        break;
      case 'sticks':
        e.info = { a: cone(x.contactConeA), b: cone(x.contactConeB), segs: segEnds(x, {}) };
        break;
      case 'braces': {
        const s = knots[x.startKnotId];
        const t = knots[x.endKnotId];
        e.info = {
          start: knotInfo(x.startKnotId),
          end: knotInfo(x.endKnotId),
          profile_d: x.profile?.diameter,
          drawn_d: s && t ? (Math.max(0.05, s.diameter - 0.1) + Math.max(0.05, t.diameter - 0.1)) / 2 : null,
        };
        break;
      }
      case 'kickstands':
        e.info = {
          root: state.roots[x.rootId] ? arr(state.roots[x.rootId].transform.pos) : null,
          knot: knotInfo(x.hostKnotId),
          segs: segEnds(x, { root: state.roots[x.rootId], hostKnot: knots[x.hostKnotId] }),
        };
        break;
      default:
        e.info = {};
    }
  }

  // Authored (VOXL) knots against where DragonFruit put them, matched by order (the open
  // path re-ids them but keeps their order).
  const authoredKnots: any[] = [...(authored.knots ?? []), ...(authored.kickstands ?? []).map((b: any) => b.hostKnot)];
  const loadedKnots = Object.values(knots);
  const role = new Map<string, string>();
  for (const b of authored.branches ?? []) role.set(b.parentKnotId, 'branch');
  for (const b of authored.braces ?? []) {
    role.set(b.startKnotId, 'brace');
    role.set(b.endKnotId, 'brace');
  }
  for (const b of authored.kickstands ?? []) role.set(b.hostKnot.id, 'kickstand');
  const knotMoves = authoredKnots.map((k, i) => {
    const l = loadedKnots[i] as any;
    if (!l) return null;
    const d = Math.hypot(l.pos.x - k.pos.x, l.pos.y - k.pos.y, l.pos.z - k.pos.z);
    return { moved: d, d_from: k.diameter, d_to: l.diameter, role: role.get(k.id) ?? '?', at: arr(k.pos), rebound: l.parentShaftId !== k.parentShaftId };
  });

  const counts = (o: any) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, Array.isArray(v) ? v.length : 0]));
  const result = {
    id,
    voxl: counts(authored),
    loaded: Object.fromEntries(['roots', 'knots', ...SUPPORT_TYPES.map((d) => d.location.key)].map((k) => [k, Object.keys(state[k] ?? {}).length])),
    triangles: tris.length,
    unlabelled: labels.reduce((n, l) => n + (l < 0 ? 1 : 0), 0),
    warnings: warnings.filter((w) => !w.startsWith('[SupportAA]')).slice(0, 50),
    warningCount: warnings.length,
    knotMoves,
    entities,
    ms: Math.round(performance.now() - t0),
  };
  writeFileSync(join(dir, `${id}.df.json`), JSON.stringify(result));
}

async function main() {
  const [dir, ...ids] = process.argv.slice(2);
  if (process.env.TIP_OFFSET_MM !== undefined && process.env.TIP_OFFSET_MM !== '') setTipOffset(Number(process.env.TIP_OFFSET_MM));
  for (const id of ids) {
    if (!existsSync(join(dir, `${id}.voxl`))) continue;
    try {
      await buildOne(dir, id);
    } catch (err) {
      writeFileSync(join(dir, `${id}.df.json`), JSON.stringify({ id, error: String((err as Error)?.stack ?? err) }));
    }
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
