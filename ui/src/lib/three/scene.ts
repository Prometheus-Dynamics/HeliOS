// The imperative half of the 3D view: a three.js scene with the robot, its
// camera frustums, detected tags and their trails. Everything robot-related
// lives in one group expressed in the robot frame (x forward, y left, z up);
// the group is rotated into three.js's y-up world once.
//
// Rendering is on demand: a frame is drawn only when something changed
// (data, options, controls, resize), and at most ~30 times a second.

import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { CSS2DObject, CSS2DRenderer } from "three/examples/jsm/renderers/CSS2DRenderer.js";
import { CAMERA_ASPECT, CAMERA_HFOV_DEG, TAG_SIZE, type Mount, type PlacedTag } from "./mounts";
import { buildRobot } from "./robot";

export type ViewPreset = "top" | "behind" | "free";

export interface SceneOptions {
  frustums: boolean;
  trails: boolean;
  grid: boolean;
  labels: boolean;
}

const MIN_FRAME_MS = 1000 / 30;
const TRAIL_FRAMES = 14;
const FRUSTUM_DEPTH = 0.75;

const PRESETS: Record<ViewPreset, { position: THREE.Vector3; target: THREE.Vector3 }> = {
  top: { position: new THREE.Vector3(-0.01, 9, 0), target: new THREE.Vector3(0, 0, 0) },
  behind: { position: new THREE.Vector3(-3.4, 1.85, 0), target: new THREE.Vector3(1.4, 0.4, 0) },
  free: { position: new THREE.Vector3(-3.1, 3.3, 3.4), target: new THREE.Vector3(0.3, 0.2, 0) },
};

function cssColor(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

function label(className: string, text: string): CSS2DObject {
  const el = document.createElement("div");
  el.className = className;
  el.textContent = text;
  return new CSS2DObject(el);
}

interface TagItem {
  group: THREE.Group;
  face: THREE.Mesh;
  edges: THREE.LineSegments;
  label: CSS2DObject;
}

interface Trail {
  points: { frame: number; position: THREE.Vector3 }[];
  line: THREE.Line;
  color: THREE.Color;
}

interface FrustumItem {
  mount: Mount;
  group: THREE.Group;
  lines: THREE.LineSegments;
  face: THREE.Mesh;
  label: CSS2DObject;
}

export class FieldScene {
  private renderer: THREE.WebGLRenderer;
  private labels: CSS2DRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(45, 1, 0.05, 60);
  private controls: OrbitControls;
  private robot = new THREE.Group();
  private procedural!: THREE.Group;
  private modelGroup = new THREE.Group();
  private raycaster = new THREE.Raycaster();
  private picking = false;
  private pickMarker!: THREE.Group;
  private pickLabel!: CSS2DObject;
  private downAt: { x: number; y: number } | null = null;
  /** Called with a robot-frame point and outward normal when a part is clicked in pick mode. */
  onPick: ((hit: { point: THREE.Vector3; normal: THREE.Vector3; part: string }) => void) | null = null;
  private gridGroup = new THREE.Group();
  private ringLabels: CSS2DObject[] = [];
  private frustumGroup = new THREE.Group();
  private tagGroup = new THREE.Group();
  private trailGroup = new THREE.Group();
  private frustums: FrustumItem[] = [];
  private tags = new Map<string, TagItem>();
  private trails = new Map<string, Trail>();
  private lastFrame = -1;
  private selected: string | null = null;
  private options: SceneOptions = { frustums: true, trails: true, grid: true, labels: true };

  private accent: THREE.Color;
  private disposables: { dispose(): void }[] = [];
  private faceMaterials = new Map<string, THREE.MeshStandardMaterial>();
  private edgeMaterial: THREE.LineBasicMaterial;
  private edgeSelected: THREE.LineBasicMaterial;
  private tagFaceGeometry = new THREE.PlaneGeometry(TAG_SIZE, TAG_SIZE);
  private tagEdgeGeometry = new THREE.EdgesGeometry(this.tagFaceGeometry);
  private tagCoreGeometry = new THREE.PlaneGeometry(TAG_SIZE * 0.62, TAG_SIZE * 0.62);
  private tagCoreMaterial = new THREE.MeshBasicMaterial({ color: 0x0b0d11, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: -1 });

  private dirty = true;
  private lastRender = 0;
  private raf = 0;
  private tween: { from: THREE.Vector3; fromTarget: THREE.Vector3; to: THREE.Vector3; toTarget: THREE.Vector3; start: number; ms: number } | null = null;
  private resizeObserver: ResizeObserver;

  constructor(
    private container: HTMLElement,
    private onSelect: (cameraId: string) => void,
    private onUserMove: () => void,
  ) {
    const bg = new THREE.Color(cssColor("--bg", "#121419"));
    this.accent = new THREE.Color(cssColor("--accent", "#ff6b6b"));

    this.renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: "low-power" });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setClearColor(bg);
    this.renderer.domElement.classList.add("f3-canvas");
    container.appendChild(this.renderer.domElement);

    this.labels = new CSS2DRenderer();
    this.labels.domElement.classList.add("f3-labels");
    container.appendChild(this.labels.domElement);

    this.scene.background = bg;
    this.scene.fog = new THREE.Fog(bg, 7, 17);

    const hemi = new THREE.HemisphereLight(0xd7defa, 0x16181d, 1.6);
    const sun = new THREE.DirectionalLight(0xffffff, 1.5);
    sun.position.set(3, 6, 2.5);
    const fill = new THREE.DirectionalLight(0x9fb4ff, 0.35);
    fill.position.set(-4, 2, -3);
    this.scene.add(hemi, sun, fill);

    this.edgeMaterial = new THREE.LineBasicMaterial({ color: 0xe6e9f0, transparent: true, opacity: 0.55 });
    this.edgeSelected = new THREE.LineBasicMaterial({ color: this.accent });
    this.disposables.push(this.tagFaceGeometry, this.tagEdgeGeometry, this.tagCoreGeometry, this.tagCoreMaterial, this.edgeMaterial, this.edgeSelected);

    this.buildGrid();

    // Robot frame (z up) -> three.js world (y up).
    this.robot.rotation.x = -Math.PI / 2;
    this.procedural = buildRobot(this.disposables);
    this.robot.add(this.procedural, this.modelGroup, this.frustumGroup, this.tagGroup, this.trailGroup);
    this.buildPickMarker();
    this.scene.add(this.robot);

    this.camera.position.copy(PRESETS.free.position);
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.target.copy(PRESETS.free.target);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.minDistance = 0.8;
    this.controls.maxDistance = 16;
    this.controls.maxPolarAngle = Math.PI * 0.495;
    this.controls.addEventListener("change", () => (this.dirty = true));
    this.controls.addEventListener("start", () => {
      this.tween = null;
      this.onUserMove();
    });
    this.controls.update();

    const el = this.renderer.domElement;
    el.addEventListener("pointerdown", (e) => (this.downAt = { x: e.clientX, y: e.clientY }));
    el.addEventListener("pointermove", (e) => this.picking && this.hover(e));
    el.addEventListener("pointerup", (e) => {
      const d = this.downAt;
      this.downAt = null;
      if (this.picking && d && Math.hypot(e.clientX - d.x, e.clientY - d.y) < 5) this.pick(e);
    });

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
    this.resize();

    this.raf = requestAnimationFrame(this.loop);
  }

  // --- Setup ---------------------------------------------------------------

  private buildGrid() {
    const minor = new THREE.GridHelper(18, 36, 0x1d2129, 0x1d2129);
    const major = new THREE.GridHelper(18, 18, 0x343a47, 0x272c36);
    major.position.y = 0.001;
    for (const g of [minor, major]) {
      const mat = g.material as THREE.Material;
      mat.transparent = true;
      mat.opacity = 0.9;
      mat.depthWrite = false;
      this.disposables.push(g.geometry, mat);
      this.gridGroup.add(g);
    }

    // Range rings around the robot, every metre.
    const ringMaterial = new THREE.LineBasicMaterial({ color: 0x4a5160, transparent: true, opacity: 0.45, depthWrite: false });
    this.disposables.push(ringMaterial);
    for (const r of [1, 2, 3, 4]) {
      const pts: THREE.Vector3[] = [];
      for (let i = 0; i < 96; i++) {
        const a = (i / 96) * Math.PI * 2;
        pts.push(new THREE.Vector3(Math.cos(a) * r, 0.002, Math.sin(a) * r));
      }
      const geo = new THREE.BufferGeometry().setFromPoints(pts);
      this.disposables.push(geo);
      this.gridGroup.add(new THREE.LineLoop(geo, ringMaterial));
      const tag = label("f3-ring", `${r} m`);
      tag.position.set(r * 0.7071, 0.002, r * 0.7071);
      this.ringLabels.push(tag);
      this.gridGroup.add(tag);
    }
    this.scene.add(this.gridGroup);
  }

  /** Builds a frustum per camera mount (call when the camera list changes). */
  setMounts(mounts: Mount[]) {
    for (const f of this.frustums) {
      this.frustumGroup.remove(f.group);
      const post = f.group.userData.post as THREE.Mesh | undefined;
      if (post) {
        this.frustumGroup.remove(post);
        post.geometry.dispose();
        (post.material as THREE.Material).dispose();
      }
      f.label.element.remove();
      f.group.traverse((o) => {
        if (o instanceof THREE.Mesh || o instanceof THREE.LineSegments) {
          o.geometry.dispose();
          (o.material as THREE.Material).dispose();
        }
      });
    }
    this.frustums = [];

    const halfW = Math.tan(THREE.MathUtils.degToRad(CAMERA_HFOV_DEG / 2)) * FRUSTUM_DEPTH;
    const halfH = halfW / CAMERA_ASPECT;
    const d = FRUSTUM_DEPTH;
    // Camera frame: x right, y down, z forward.
    const corners = [
      [-halfW, -halfH, d],
      [halfW, -halfH, d],
      [halfW, halfH, d],
      [-halfW, halfH, d],
    ];
    const seg: number[] = [];
    for (let i = 0; i < 4; i++) {
      const a = corners[i];
      const b = corners[(i + 1) % 4];
      seg.push(0, 0, 0, ...a, ...a, ...b);
    }
    // A small "up" notch on the top edge so the frame's orientation reads.
    seg.push(-halfW * 0.18, -halfH, d, 0, -halfH * 1.3, d, 0, -halfH * 1.3, d, halfW * 0.18, -halfH, d);

    for (const mount of mounts) {
      const group = new THREE.Group();
      group.matrixAutoUpdate = false;
      group.matrix.copy(mount.matrix);

      const lineGeo = new THREE.BufferGeometry();
      lineGeo.setAttribute("position", new THREE.Float32BufferAttribute(seg, 3));
      const lines = new THREE.LineSegments(lineGeo, new THREE.LineBasicMaterial({ color: mount.color, transparent: true, opacity: 0.55 }));

      const faceGeo = new THREE.BufferGeometry();
      faceGeo.setAttribute("position", new THREE.Float32BufferAttribute([...corners[0], ...corners[1], ...corners[2], ...corners[0], ...corners[2], ...corners[3]], 3));
      const face = new THREE.Mesh(
        faceGeo,
        new THREE.MeshBasicMaterial({ color: mount.color, transparent: true, opacity: 0.05, side: THREE.DoubleSide, depthWrite: false }),
      );

      const body = new THREE.Mesh(new THREE.BoxGeometry(0.06, 0.04, 0.035), new THREE.MeshStandardMaterial({ color: 0x1b1e24, roughness: 0.6 }));
      body.position.z = -0.012;
      const lens = new THREE.Mesh(new THREE.CylinderGeometry(0.012, 0.012, 0.012, 16), new THREE.MeshStandardMaterial({ color: mount.color, roughness: 0.3, metalness: 0.2 }));
      lens.rotation.x = Math.PI / 2;
      lens.position.z = 0.009;

      const tag = label("f3-cam", mount.camera.name);
      tag.element.style.setProperty("--c", mount.color);
      tag.element.addEventListener("pointerdown", (e) => {
        e.stopPropagation();
        this.onSelect(mount.camera.resourceId);
      });
      tag.position.set(0, -0.075, 0);

      group.add(lines, face, body, lens, tag);
      this.frustumGroup.add(group);

      // A post from the frame up to the camera, in the robot frame.
      const postH = Math.max(0.02, mount.position.z - 0.16);
      const post = new THREE.Mesh(new THREE.BoxGeometry(0.022, 0.022, postH), new THREE.MeshStandardMaterial({ color: 0x5b6373, roughness: 0.5, metalness: 0.4 }));
      post.position.set(mount.position.x, mount.position.y, 0.16 + postH / 2 - 0.02);
      post.visible = this.procedural.visible;
      group.userData.post = post;
      this.frustumGroup.add(post);
      this.frustums.push({ mount, group, lines, face, label: tag });
    }
    this.applySelection();
    this.applyOptions();
  }

  // --- Robot model and picking ------------------------------------------------

  private buildPickMarker() {
    const g = new THREE.Group();
    const dotMat = new THREE.MeshBasicMaterial({ color: this.accent, depthTest: false, transparent: true });
    const dotGeo = new THREE.SphereGeometry(0.012, 12, 8);
    const dot = new THREE.Mesh(dotGeo, dotMat);
    dot.renderOrder = 10;
    const lineGeo = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(), new THREE.Vector3(0, 0, 0.12)]);
    const lineMat = new THREE.LineBasicMaterial({ color: this.accent, depthTest: false, transparent: true });
    const line = new THREE.Line(lineGeo, lineMat);
    line.renderOrder = 10;
    this.disposables.push(dotMat, dotGeo, lineGeo, lineMat);
    g.add(dot, line);
    this.pickLabel = label("f3-pick", "");
    this.pickLabel.position.set(0, 0, 0.16);
    g.add(this.pickLabel);
    g.visible = false;
    this.pickMarker = g;
    this.robot.add(g);
  }

  /** Shows a CAD model in place of the built-in chassis (null restores it). */
  setModel(object: THREE.Object3D | null, opts: { unit: number; up: "y" | "z"; offset: [number, number, number]; yaw: number } | null) {
    this.modelGroup.clear();
    this.procedural.visible = !object;
    if (object && opts) {
      const holder = new THREE.Group();
      const inner = new THREE.Group();
      inner.add(object);
      inner.scale.setScalar(opts.unit);
      if (opts.up === "y") inner.rotation.x = Math.PI / 2;
      holder.add(inner);
      holder.rotation.z = THREE.MathUtils.degToRad(opts.yaw);
      holder.updateMatrixWorld(true);
      // Sit it on the ground, centred on the robot origin.
      const box = new THREE.Box3().setFromObject(holder);
      const c = box.getCenter(new THREE.Vector3());
      holder.position.set(-c.x + opts.offset[0], -c.y + opts.offset[1], -box.min.z + opts.offset[2]);
      this.modelGroup.add(holder);
    }
    for (const f of this.frustums) {
      const post = f.group.userData.post as THREE.Mesh | undefined;
      if (post) post.visible = !object;
    }
    this.dirty = true;
  }

  setPicking(on: boolean) {
    this.picking = on;
    this.pickMarker.visible = false;
    this.renderer.domElement.style.cursor = on ? "crosshair" : "";
    this.dirty = true;
  }

  private hit(event: PointerEvent) {
    const r = this.renderer.domElement.getBoundingClientRect();
    const ndc = new THREE.Vector2(((event.clientX - r.left) / r.width) * 2 - 1, -((event.clientY - r.top) / r.height) * 2 + 1);
    this.raycaster.setFromCamera(ndc, this.camera);
    const target = this.procedural.visible ? this.procedural : this.modelGroup;
    const hits = this.raycaster.intersectObject(target, true).filter((h) => h.object instanceof THREE.Mesh && h.face);
    const h = hits[0];
    if (!h || !h.face) return null;
    const point = this.robot.worldToLocal(h.point.clone());
    const normalWorld = h.face.normal.clone().transformDirection(h.object.matrixWorld);
    const inv = new THREE.Matrix4().copy(this.robot.matrixWorld).invert();
    const normal = normalWorld.transformDirection(inv).normalize();
    let part = h.object.name;
    for (let o: THREE.Object3D | null = h.object; o && !part; o = o.parent) part = o.name;
    return { point, normal, part: part || "part" };
  }

  private hover(event: PointerEvent) {
    const h = this.hit(event);
    this.pickMarker.visible = Boolean(h);
    if (h) {
      this.pickMarker.position.copy(h.point);
      this.pickMarker.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), h.normal);
      this.pickLabel.element.textContent = h.part;
    }
    this.dirty = true;
  }

  private pick(event: PointerEvent) {
    const h = this.hit(event);
    if (h) this.onPick?.(h);
  }

  // --- Updates ---------------------------------------------------------------

  setSelected(cameraId: string | null) {
    this.selected = cameraId;
    this.applySelection();
  }

  private applySelection() {
    for (const f of this.frustums) {
      const on = f.mount.camera.resourceId === this.selected;
      const lm = f.lines.material as THREE.LineBasicMaterial;
      lm.color.set(on ? this.accent : f.mount.color);
      lm.opacity = on ? 0.95 : 0.4;
      const fm = f.face.material as THREE.MeshBasicMaterial;
      fm.color.set(on ? this.accent : f.mount.color);
      fm.opacity = on ? 0.09 : 0.035;
      f.label.element.classList.toggle("selected", on);
    }
    for (const [key, item] of this.tags) {
      const on = key.startsWith(`${this.selected}#`);
      item.edges.material = on ? this.edgeSelected : this.edgeMaterial;
      item.label.element.classList.toggle("selected", on);
    }
    this.dirty = true;
  }

  setOptions(options: SceneOptions) {
    this.options = { ...options };
    this.applyOptions();
  }

  private applyOptions() {
    this.frustumGroup.visible = true;
    for (const f of this.frustums) {
      f.lines.visible = this.options.frustums;
      f.face.visible = this.options.frustums;
      f.label.visible = this.options.labels;
    }
    this.trailGroup.visible = this.options.trails;
    this.gridGroup.visible = this.options.grid;
    for (const r of this.ringLabels) r.visible = this.options.labels && this.options.grid;
    for (const item of this.tags.values()) item.label.visible = this.options.labels && item.group.visible;
    this.dirty = true;
  }

  private faceMaterial(color: string): THREE.MeshStandardMaterial {
    let mat = this.faceMaterials.get(color);
    if (!mat) {
      mat = new THREE.MeshStandardMaterial({ color, roughness: 0.55, emissive: color, emissiveIntensity: 0.18, side: THREE.DoubleSide });
      this.faceMaterials.set(color, mat);
      this.disposables.push(mat);
    }
    return mat;
  }

  private tagItem(tag: PlacedTag): TagItem {
    let item = this.tags.get(tag.key);
    if (item) return item;
    const group = new THREE.Group();
    group.matrixAutoUpdate = false;
    const face = new THREE.Mesh(this.tagFaceGeometry, this.faceMaterial(tag.color));
    const core = new THREE.Mesh(this.tagCoreGeometry, this.tagCoreMaterial);
    core.position.z = 0.0015;
    const edges = new THREE.LineSegments(this.tagEdgeGeometry, this.edgeMaterial);
    const text = label("f3-tag", String(tag.id));
    text.element.style.setProperty("--c", tag.color);
    // Tag frame: y points down the tag's face; put the label above it.
    text.position.set(0, -TAG_SIZE * 0.95, 0);
    group.add(face, core, edges, text);
    this.tagGroup.add(group);
    item = { group, face, edges, label: text };
    this.tags.set(tag.key, item);
    const on = tag.key.startsWith(`${this.selected}#`);
    item.edges.material = on ? this.edgeSelected : this.edgeMaterial;
    text.element.classList.toggle("selected", on);
    return item;
  }

  /** The tags visible on this replay frame, already in the robot frame. */
  setTags(tags: PlacedTag[], frame: number) {
    const seen = new Set<string>();
    for (const tag of tags) {
      const item = this.tagItem(tag);
      item.group.matrix.copy(tag.matrix);
      item.group.matrixWorldNeedsUpdate = true;
      item.group.visible = true;
      item.label.visible = this.options.labels;
      seen.add(tag.key);
    }
    for (const [key, item] of this.tags) {
      if (seen.has(key)) continue;
      item.group.visible = false;
      item.label.visible = false;
    }
    this.updateTrails(tags, frame);
    this.dirty = true;
  }

  private updateTrails(tags: PlacedTag[], frame: number) {
    if (frame === this.lastFrame) return;
    // The replay looped or jumped: old positions belong to another moment.
    const jumped = this.lastFrame >= 0 && (frame < this.lastFrame || frame - this.lastFrame > 6);
    this.lastFrame = frame;
    if (jumped) for (const t of this.trails.values()) t.points.length = 0;

    for (const tag of tags) {
      let trail = this.trails.get(tag.key);
      if (!trail) {
        const geo = new THREE.BufferGeometry();
        geo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(TRAIL_FRAMES * 3), 3));
        geo.setAttribute("color", new THREE.BufferAttribute(new Float32Array(TRAIL_FRAMES * 4), 4));
        const line = new THREE.Line(geo, new THREE.LineBasicMaterial({ vertexColors: true, transparent: true, depthWrite: false }));
        line.frustumCulled = false;
        this.trailGroup.add(line);
        trail = { points: [], line, color: new THREE.Color(tag.color) };
        this.trails.set(tag.key, trail);
      }
      trail.points.push({ frame, position: tag.position.clone() });
    }

    for (const trail of this.trails.values()) {
      while (trail.points.length && trail.points[0].frame <= frame - TRAIL_FRAMES) trail.points.shift();
      const n = trail.points.length;
      trail.line.visible = n >= 2;
      if (n < 2) continue;
      const pos = trail.line.geometry.getAttribute("position") as THREE.BufferAttribute;
      const col = trail.line.geometry.getAttribute("color") as THREE.BufferAttribute;
      for (let i = 0; i < n; i++) {
        const p = trail.points[i];
        pos.setXYZ(i, p.position.x, p.position.y, p.position.z);
        const age = (frame - p.frame) / TRAIL_FRAMES;
        col.setXYZW(i, trail.color.r, trail.color.g, trail.color.b, Math.max(0, 0.75 * (1 - age)));
      }
      pos.needsUpdate = true;
      col.needsUpdate = true;
      trail.line.geometry.setDrawRange(0, n);
    }
  }

  /** Glides the camera to a preset; "free" returns to the default orbit. */
  setView(view: ViewPreset) {
    const preset = PRESETS[view];
    this.tween = {
      from: this.camera.position.clone(),
      fromTarget: this.controls.target.clone(),
      to: preset.position.clone(),
      toTarget: preset.target.clone(),
      start: performance.now(),
      ms: 420,
    };
    this.dirty = true;
  }

  // --- Loop ------------------------------------------------------------------

  private loop = (now: number) => {
    this.raf = requestAnimationFrame(this.loop);
    if (this.tween) {
      const t = Math.min(1, (now - this.tween.start) / this.tween.ms);
      const k = 1 - Math.pow(1 - t, 3);
      this.camera.position.lerpVectors(this.tween.from, this.tween.to, k);
      this.controls.target.lerpVectors(this.tween.fromTarget, this.tween.toTarget, k);
      if (t >= 1) this.tween = null;
      this.dirty = true;
    }
    // With damping, update() keeps easing after a drag and fires "change".
    this.controls.update();
    if (!this.dirty || now - this.lastRender < MIN_FRAME_MS) return;
    this.dirty = false;
    this.lastRender = now;
    this.renderer.render(this.scene, this.camera);
    this.labels.render(this.scene, this.camera);
  };

  private resize() {
    const w = Math.max(1, this.container.clientWidth);
    const h = Math.max(1, this.container.clientHeight);
    this.renderer.setSize(w, h);
    this.labels.setSize(w, h);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.dirty = true;
  }

  dispose() {
    cancelAnimationFrame(this.raf);
    this.resizeObserver.disconnect();
    this.controls.dispose();
    this.setMounts([]);
    for (const trail of this.trails.values()) {
      trail.line.geometry.dispose();
      (trail.line.material as THREE.Material).dispose();
    }
    this.trails.clear();
    this.tags.clear();
    for (const d of this.disposables) d.dispose();
    this.disposables = [];
    this.renderer.dispose();
    this.renderer.forceContextLoss();
    this.renderer.domElement.remove();
    this.labels.domElement.remove();
  }
}
