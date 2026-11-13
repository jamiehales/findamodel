import * as THREE from 'three';
import { createPreviewMaterial } from '../../lib/previewShading';
import type { Bounds, SpinMesh } from '../../lib/spinMesh';
import styles from './SpinPreview.module.css';

// Turns a model tile's spin mesh in 3D while the tile is hovered. One WebGL renderer and canvas
// are shared by every tile (browsers limit WebGL contexts): the canvas moves into the hovered
// tile's image box and leaves it on pointer leave.
//
// The first frame matches the tile's PNG preview: the camera, framing and shading of the
// preview renderer (backend/src/preview/camera.rs, ported in ModelViewer.tsx and
// previewShading.ts). The camera then eases to a framing of the model's bounding cylinder about
// the vertical axis, which holds the model at one size as it turns.

/** One full turn about the vertical axis, in milliseconds. */
export const ROTATION_PERIOD_MS = 2000;
/** How long the camera takes to move from the preview's framing to the turning framing. */
const FRAMING_EASE_MS = 400;

const BODY_COLOR = '#a8404f';
const FOV_Y_DEGREES = 45;
const FRAMING_PADDING = 1.15;
const CAMERA_DIRECTION = new THREE.Vector3(1, 0.8, -1).normalize();
const CYLINDER_SAMPLES = 64;

interface Framing {
  target: THREE.Vector3;
  distance: number;
}

/**
 * Port of camera_distance_for_box (backend/src/preview/camera.rs) for any points relative to
 * the target: the distance along the preview direction at which every point fits in a square
 * frustum, with the preview's padding.
 */
function distanceForPoints(points: THREE.Vector3[]): number {
  const tan = Math.tan(THREE.MathUtils.degToRad(FOV_Y_DEGREES) / 2);
  const toCamera = CAMERA_DIRECTION;
  const right = new THREE.Vector3(0, 1, 0).cross(toCamera).normalize();
  const up = toCamera.clone().cross(right).normalize();
  let required = 0;
  for (const p of points) {
    const cx = Math.abs(p.dot(right));
    const cy = Math.abs(p.dot(up));
    const cz = p.dot(toCamera);
    required = Math.max(required, cz + cx / tan, cz + cy / tan);
  }
  return Math.max(required, 0.001) * FRAMING_PADDING;
}

/** The preview renderer's framing of a bounding box. */
function boxFraming({ min, max }: Bounds): Framing {
  const lo = new THREE.Vector3(...min);
  const hi = new THREE.Vector3(...max);
  const target = lo.clone().add(hi).multiplyScalar(0.5);
  const half = hi.clone().sub(lo).multiplyScalar(0.5);
  const corners: THREE.Vector3[] = [];
  for (const sx of [-1, 1])
    for (const sy of [-1, 1])
      for (const sz of [-1, 1])
        corners.push(new THREE.Vector3(sx * half.x, sy * half.y, sz * half.z));
  return { target, distance: distanceForPoints(corners) };
}

/** A framing of the cylinder the body sweeps as it turns about `axis` (x, z). */
function cylinderFraming(positions: Float32Array, axis: [number, number], bounds: Bounds): Framing {
  let radiusSq = 0;
  for (let i = 0; i < positions.length; i += 3) {
    const dx = positions[i] - axis[0];
    const dz = positions[i + 2] - axis[1];
    radiusSq = Math.max(radiusSq, dx * dx + dz * dz);
  }
  const radius = Math.sqrt(radiusSq);
  const halfHeight = (bounds.max[1] - bounds.min[1]) / 2;
  const points: THREE.Vector3[] = [];
  for (let i = 0; i < CYLINDER_SAMPLES; i++) {
    const a = (i / CYLINDER_SAMPLES) * Math.PI * 2;
    for (const y of [-halfHeight, halfHeight]) {
      points.push(new THREE.Vector3(Math.cos(a) * radius, y, Math.sin(a) * radius));
    }
  }
  const target = new THREE.Vector3(axis[0], (bounds.min[1] + bounds.max[1]) / 2, axis[1]);
  return { target, distance: distanceForPoints(points) };
}

function smoothstep(t: number): number {
  const x = Math.min(Math.max(t, 0), 1);
  return x * x * (3 - 2 * x);
}

interface Session {
  container: HTMLElement;
  image: HTMLImageElement | null;
  frame: number | null;
  geometry: THREE.BufferGeometry | null;
}

function prefersReducedMotion(): boolean {
  return window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
}

class SpinPlayer {
  private renderer: THREE.WebGLRenderer | null = null;
  private unavailable = false;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(FOV_Y_DEGREES, 1, 0.1, 1000);
  private readonly material = createPreviewMaterial(BODY_COLOR);
  /** Turns about the vertical axis through the body's centre. */
  private readonly pivot = new THREE.Group();
  private readonly mesh = new THREE.Mesh(undefined, this.material);
  private session: Session | null = null;

  constructor() {
    this.pivot.add(this.mesh);
    this.scene.add(this.pivot);
  }

  private ensureRenderer(): THREE.WebGLRenderer | null {
    if (this.renderer || this.unavailable) return this.renderer;
    try {
      const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
      renderer.setClearColor(0x000000, 0);
      renderer.setPixelRatio(window.devicePixelRatio || 1);
      renderer.domElement.className = styles.canvas;
      renderer.domElement.setAttribute('aria-hidden', 'true');
      renderer.domElement.addEventListener('webglcontextlost', () => {
        // Stop and start over with a new renderer on the next hover.
        if (this.session) this.stop(this.session.container);
        this.renderer?.dispose();
        this.renderer = null;
      });
      this.renderer = renderer;
    } catch {
      this.unavailable = true;
    }
    return this.renderer;
  }

  /**
   * Starts turning the model in `container` (a tile's image box) once `load` gives its spin
   * mesh. `withSupports`: the tile shows the with-supports preview, so the first frame uses its
   * framing. If loading fails the tile keeps its PNG.
   */
  start(container: HTMLElement, withSupports: boolean, load: () => Promise<SpinMesh>): void {
    if (prefersReducedMotion()) return;
    if (this.session) this.stop(this.session.container);
    const renderer = this.ensureRenderer();
    if (!renderer) return;

    const session: Session = {
      container,
      image: container.querySelector('img'),
      frame: null,
      geometry: null,
    };
    this.session = session;
    // In the tile from the start (hidden until the first frame), so its hover zoom runs in step
    // with the preview image's.
    const canvas = renderer.domElement;
    canvas.removeAttribute('data-visible');
    // An element inserted into a hovered card would take the zoomed state at once; start it
    // unzoomed so it transitions with the image.
    canvas.style.transform = 'none';
    container.appendChild(canvas);
    void canvas.offsetWidth;
    canvas.style.transform = '';

    load()
      .then((spinMesh) => {
        if (this.session === session) this.play(session, renderer, spinMesh, withSupports);
      })
      .catch(() => {
        // Keep the PNG.
      });
  }

  /** Stops turning the model in `container`, if it is the one turning, and shows its PNG. */
  stop(container: HTMLElement): void {
    const session = this.session;
    if (!session || session.container !== container) return;
    this.session = null;
    if (session.frame !== null) cancelAnimationFrame(session.frame);
    this.mesh.geometry = new THREE.BufferGeometry();
    session.geometry?.dispose();
    const canvas = this.renderer?.domElement;
    if (canvas) {
      canvas.removeAttribute('data-visible');
      canvas.remove();
    }
    if (session.image) session.image.style.visibility = '';
  }

  private play(
    session: Session,
    renderer: THREE.WebGLRenderer,
    spinMesh: SpinMesh,
    withSupports: boolean,
  ): void {
    const { positions, indices, bodyBounds, modelBounds } = spinMesh;
    const axis: [number, number] = [
      (bodyBounds.min[0] + bodyBounds.max[0]) / 2,
      (bodyBounds.min[2] + bodyBounds.max[2]) / 2,
    ];
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.setIndex(new THREE.BufferAttribute(indices, 1));
    session.geometry = geometry;
    this.mesh.geometry = geometry;
    // The pivot sits on the axis and the mesh is offset back, so the pivot turns the body about
    // its own vertical axis and leaves it where the preview drew it at angle 0.
    this.mesh.position.set(-axis[0], 0, -axis[1]);
    this.pivot.position.set(axis[0], 0, axis[1]);

    const from = boxFraming(withSupports ? modelBounds : bodyBounds);
    const to = cylinderFraming(positions, axis, bodyBounds);
    const target = new THREE.Vector3();
    const canvas = renderer.domElement;
    let started: number | null = null;
    let size = { width: 0, height: 0 };

    const draw = (now: number) => {
      if (this.session !== session) return;
      started ??= now;
      const elapsed = now - started;
      this.pivot.rotation.y = ((elapsed % ROTATION_PERIOD_MS) / ROTATION_PERIOD_MS) * Math.PI * 2;

      const ease = smoothstep(elapsed / FRAMING_EASE_MS);
      target.lerpVectors(from.target, to.target, ease);
      const distance = THREE.MathUtils.lerp(from.distance, to.distance, ease);
      this.camera.position.copy(target).addScaledVector(CAMERA_DIRECTION, distance);
      this.camera.lookAt(target);
      this.camera.near = distance / 100;
      this.camera.far = distance * 10;

      // The preview is square and covers the tile's image box (object-fit: cover): draw the
      // same square and show the part of it the box shows.
      const width = Math.max(1, session.container.clientWidth);
      const height = Math.max(1, session.container.clientHeight);
      if (width !== size.width || height !== size.height) {
        size = { width, height };
        renderer.setSize(width, height, false);
      }
      const side = Math.max(width, height);
      this.camera.setViewOffset(side, side, (side - width) / 2, (side - height) / 2, width, height);
      this.camera.updateProjectionMatrix();

      renderer.render(this.scene, this.camera);
      if (!canvas.hasAttribute('data-visible')) {
        // Swap in the same frame the first image is drawn.
        canvas.setAttribute('data-visible', '');
        if (session.image) session.image.style.visibility = 'hidden';
      }
      session.frame = requestAnimationFrame(draw);
    };
    session.frame = requestAnimationFrame(draw);
  }
}

let player: SpinPlayer | null = null;

/** The shared player, created on first use. */
export function spinPlayer(): SpinPlayer {
  player ??= new SpinPlayer();
  return player;
}
