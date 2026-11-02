// The 3D scene of a live map: the point cloud (grey per point), the camera's path, the keyframes
// as small frustums, the mapper's tags, the dense mesh (grey or normal shaded: one mesh per submap,
// each placed by its pose), the field's tags and the robot once the map is aligned to the field.
// The path and the keyframes are drawn from the trajectory store, not from the snapshot. Two views: Field (orbit the field, z up as WPILib
// has it) and Camera (from the camera's own pose, with its lens' field of view). Layers toggle one
// by one. Aligned, the map is in WPILib's field frame and is turned into the scene's y-up frame; not
// aligned, up is unknown, so there is no floor and no field. The view frames the content once per
// run (and again when it aligns), so the user's own orbiting is never undone by a new snapshot.
// The scene owns its GPU resources and frees them in `dispose`.

import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { CSS2DObject, CSS2DRenderer } from "three/examples/jsm/renderers/CSS2DRenderer.js";
import type { MapSnapshot, MapTransform, StreamedTrajectory } from "#lib/vision/map.js";
import { greyColours, type DenseChunkStore, type DenseMeshView } from "#lib/vision/dense.js";
import type { FieldTag } from "#lib/stores/field.svelte.js";
import { cssVarColor } from "./colors";

/** Half-angles of the drawn frustums, radians (a 60 x 45 degree view). */
const HALF_H = THREE.MathUtils.degToRad(30);
const HALF_V = THREE.MathUtils.degToRad(22.5);
/** Depth of a keyframe's frustum, metres, and of the current camera's. */
const KEYFRAME_DEPTH = 0.09;
const CURRENT_DEPTH = 0.22;
/** The field view's lens, degrees, and the camera view's when the lens is not known. */
const FIELD_FOV = 50;
const FALLBACK_FOV = 45;
/** A robot as a box, metres: its footprint and height (its origin is on the floor). */
const ROBOT_SIZE: [number, number, number] = [0.8, 0.8, 0.5];
/** The default view's direction, from the content's centre (y up, in the scene). */
const DEFAULT_DIRECTION = new THREE.Vector3(1, 0.75, 1).normalize();
/** The camera's optical frame (x right, y down, z forward) as the scene's (x right, y up, z back). */
const OPTICAL_TO_SCENE = new THREE.Matrix4().makeScale(1, -1, -1);

export type SceneMode = "field" | "camera";
export type MeshShading = "grey" | "normal";

/** The layers the scene draws. */
export interface SceneLayers {
  points: boolean;
  path: boolean;
  keyframes: boolean;
  mesh: boolean;
  fieldTags: boolean;
  mapTags: boolean;
  camera: boolean;
  robot: boolean;
  floor: boolean;
}

export const ALL_LAYERS: SceneLayers = { points: true, path: true, keyframes: true, mesh: true, fieldTags: true, mapTags: true, camera: true, robot: true, floor: true };

export interface SceneData {
  snapshot: MapSnapshot | null;
  /** The camera's pose in the scene's frame (the field's once aligned); null without one. */
  camera: MapTransform | null;
  /** The robot's pose on the field (`robot_in_field`); null without a mount. */
  robot: MapTransform | null;
  /** The field layout's tags, drawn where the field is (aligned). */
  fieldTags: FieldTag[];
  /** Whether the map is aligned to the field: the field's tags and the robot are drawn only then. */
  aligned: boolean;
  /** The trajectory store: the path and the keyframes are drawn from it. */
  trajectory: StreamedTrajectory | null;
  /** The dense mapper's submap chunks, each a mesh placed by its pose. */
  chunks: DenseChunkStore | null;
  /** The camera's lens: vertical field of view (radians) and aspect; null when it is not known. */
  lens: { fovY: number; aspect: number } | null;
  mode: SceneMode;
  shading: MeshShading;
  layers: SceneLayers;
}

function quaternionOf(t: MapTransform): THREE.Quaternion {
  return new THREE.Quaternion(t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w);
}

function positionOf(t: MapTransform): THREE.Vector3 {
  return new THREE.Vector3(t.translation.x, t.translation.y, t.translation.z);
}

/** Frustum edges in the camera's frame (apex at the origin, looking down +z), as line-segment pairs. */
function frustumLocal(depth: number): number[] {
  const w = Math.tan(HALF_H) * depth;
  const h = Math.tan(HALF_V) * depth;
  const corners = [
    [-w, -h, depth],
    [w, -h, depth],
    [w, h, depth],
    [-w, h, depth],
  ];
  const out: number[] = [];
  for (const c of corners) out.push(0, 0, 0, ...c);
  for (let i = 0; i < 4; i++) out.push(...corners[i], ...corners[(i + 1) % 4]);
  return out;
}

/** Appends one frustum, placed by `t`, to a flat list of line vertices. */
function appendFrustum(out: number[], t: MapTransform, depth: number) {
  const q = quaternionOf(t);
  const p = positionOf(t);
  const local = frustumLocal(depth);
  const v = new THREE.Vector3();
  for (let i = 0; i < local.length; i += 3) {
    v.set(local[i], local[i + 1], local[i + 2]).applyQuaternion(q).add(p);
    out.push(v.x, v.y, v.z);
  }
}

/** The quaternion of a field tag: its yaw (about z) then its pitch (about y), as the field layout writes them. */
function fieldTagQuaternion(tag: FieldTag): THREE.Quaternion {
  const yaw = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), THREE.MathUtils.degToRad(tag.yaw));
  const pitch = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), THREE.MathUtils.degToRad(tag.pitch));
  return yaw.multiply(pitch);
}

/** One submap's mesh in the scene: the chunk and pose it was built from (a change rebuilds it). */
interface DenseSlot {
  mesh: THREE.Mesh;
  chunk: DenseMeshView | null;
  pose: unknown;
}

export class MapScene {
  private renderer: THREE.WebGLRenderer;
  private labels: CSS2DRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(FIELD_FOV, 1, 0.02, 200);
  private controls: OrbitControls;
  private disposables: { dispose(): void }[] = [];
  /** The map's frame within the scene (turned z-up to y-up once aligned). */
  private world = new THREE.Group();
  private cloud: THREE.Points | null = null;
  private cloudSource: MapSnapshot["points"] | null = null;
  private path: THREE.Line | null = null;
  /** The trajectory store and its version the path and keyframes were built from. */
  private trajectorySource: StreamedTrajectory | null = null;
  private trajectoryVersion = -1;
  private keyframesSource: StreamedTrajectory | null = null;
  private keyframesVersion = -1;
  private keyframes: THREE.LineSegments;
  private current: THREE.LineSegments;
  private currentDot: THREE.Mesh;
  private robot: THREE.Group;
  private robotEdges: THREE.LineSegments;
  private mapTags = new THREE.Group();
  private mapTagKey = "";
  private fieldTags = new THREE.Group();
  private fieldTagKey = "";
  private mapTagLabels: CSS2DObject[] = [];
  private fieldTagLabels: CSS2DObject[] = [];
  /** The dense mesh: a group of one mesh per submap, in the map's frame. */
  private dense = new THREE.Group();
  private denseSlots = new Map<number, DenseSlot>();
  private denseSource: DenseChunkStore | null = null;
  private denseVersion = -1;
  private meshGrey: THREE.MeshStandardMaterial;
  private meshNormal: THREE.MeshNormalMaterial;
  private axes = new THREE.AxesHelper(0.5);
  private floor: THREE.GridHelper;
  /** The run (and alignment) the view was last framed for. */
  private framedFor = "";
  private mode: SceneMode = "field";
  private lens: { fovY: number; aspect: number } | null = null;
  private size = { w: 1, h: 1 };
  private raf = 0;
  private resizeObserver: ResizeObserver;
  private dirty = true;
  /** The camera's pose in the scene's frame (the Camera view stands at it). */
  private lastPose: MapTransform | null = null;
  private colours: { accent: THREE.Color; field: THREE.Color; placed: THREE.Color; trail: THREE.Color; keyframe: THREE.Color; robot: THREE.Color };

  constructor(private container: HTMLElement) {
    const bg = new THREE.Color(cssVarColor("var(--bg)", "#121419"));
    this.colours = {
      accent: new THREE.Color(cssVarColor("var(--accent)", "#ff6b6b")),
      field: new THREE.Color(cssVarColor("var(--accent)", "#ff6b6b")),
      placed: new THREE.Color(cssVarColor("var(--fg-2)", "#c7cad3")),
      trail: new THREE.Color(cssVarColor("var(--fg-3)", "#8a909c")),
      keyframe: new THREE.Color(cssVarColor("var(--fg-2)", "#c7cad3")),
      robot: new THREE.Color(cssVarColor("var(--info)", "#60a5fa")),
    };

    this.renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: "low-power" });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    this.renderer.setClearColor(bg);
    this.renderer.domElement.classList.add("map-canvas");
    container.appendChild(this.renderer.domElement);

    this.labels = new CSS2DRenderer();
    this.labels.domElement.classList.add("map-labels");
    container.appendChild(this.labels.domElement);

    this.scene.background = bg;
    this.scene.fog = new THREE.Fog(bg, 25, 90);
    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x445066, 1.1));
    const sun = new THREE.DirectionalLight(0xffffff, 1.3);
    sun.position.set(3, 6, 2);
    this.scene.add(sun);

    this.floor = new THREE.GridHelper(24, 48, 0x465062, 0x2a303b);
    const floorMaterial = this.floor.material as THREE.Material;
    floorMaterial.transparent = true;
    floorMaterial.opacity = 0.9;
    floorMaterial.depthWrite = false;
    this.disposables.push(this.floor.geometry, floorMaterial);
    this.scene.add(this.floor);

    this.keyframes = this.lines(this.colours.keyframe, 0.5);
    this.current = this.lines(this.colours.accent, 1);
    const dotGeometry = new THREE.SphereGeometry(0.018, 12, 8);
    const dotMaterial = new THREE.MeshBasicMaterial({ color: this.colours.accent });
    this.currentDot = new THREE.Mesh(dotGeometry, dotMaterial);
    this.disposables.push(dotGeometry, dotMaterial);

    const robotGeometry = new THREE.EdgesGeometry(new THREE.BoxGeometry(...ROBOT_SIZE));
    const robotMaterial = new THREE.LineBasicMaterial({ color: this.colours.robot });
    this.robotEdges = new THREE.LineSegments(robotGeometry, robotMaterial);
    this.disposables.push(robotGeometry, robotMaterial);
    this.robot = new THREE.Group();
    this.robot.add(this.robotEdges);
    this.robot.visible = false;

    this.meshGrey = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.9, metalness: 0, side: THREE.DoubleSide });
    this.meshNormal = new THREE.MeshNormalMaterial({ side: THREE.DoubleSide });
    this.dense.visible = false;
    this.disposables.push(this.meshGrey, this.meshNormal);

    this.world.add(this.keyframes, this.current, this.currentDot, this.robot, this.mapTags, this.fieldTags, this.dense);
    this.scene.add(this.world);

    this.axes.visible = false;
    this.world.add(this.axes);

    this.camera.position.set(3.4, 2.4, 3.6);
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.target.set(0, 0.8, 0);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.minDistance = 0.4;
    this.controls.maxDistance = 80;
    this.controls.addEventListener("change", () => (this.dirty = true));
    this.controls.update();

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
    this.resize();
    this.raf = requestAnimationFrame(this.loop);
  }

  private lines(colour: THREE.Color, opacity: number): THREE.LineSegments {
    const geometry = new THREE.BufferGeometry();
    const material = new THREE.LineBasicMaterial({ color: colour, transparent: true, opacity });
    this.disposables.push(geometry, material);
    return new THREE.LineSegments(geometry, material);
  }

  /** Shows the map's snapshot and the layers, in the view the data asks for. */
  update(data: SceneData) {
    const { layers, aligned } = data;
    this.mode = data.mode;
    this.lens = data.lens;
    this.world.rotation.x = aligned ? -Math.PI / 2 : 0;
    this.axes.visible = aligned;
    this.floor.visible = aligned && layers.floor;
    const snapshot = data.snapshot;
    this.updatePoints(snapshot?.points ?? null, layers.points);
    this.updatePath(data.trajectory, layers.path);
    this.updateKeyframes(data.trajectory, layers.keyframes);
    this.updateFieldTags(data.fieldTags, aligned && layers.fieldTags, snapshot?.tag_size_m ?? 0.1651);
    this.updateMapTags(snapshot, aligned ? new Set(data.fieldTags.map((t) => t.id)) : new Set(), layers.mapTags);
    this.updateDense(data.chunks, data.shading, layers.mesh);
    this.updateCurrent(data.camera, layers.camera);
    this.updateRobot(data.robot, aligned && layers.robot);
    this.frameOnce(snapshot, aligned);
    this.lastPose = data.camera;
    this.syncCamera();
    this.placeCamera(data.camera);
    this.dirty = true;
  }

  /** Points the view at the content once per run and alignment, so orbiting is never undone. */
  private frameOnce(snapshot: MapSnapshot | null, aligned: boolean) {
    if (this.mode !== "field" || !snapshot || snapshot.points.count === 0) return;
    const key = `${snapshot.run_id}:${aligned}`;
    if (key === this.framedFor) return;
    this.framedFor = key;
    this.frameContent();
  }

  /** The view of the content as a whole, from the default direction (`reset`), or from where it is (`fit`). */
  fit() {
    if (this.mode === "field") this.frameContent();
  }

  /** The default three-quarter view of the content. */
  resetView() {
    if (this.mode !== "field") return;
    this.frameContent(DEFAULT_DIRECTION);
  }

  private frameContent(direction?: THREE.Vector3) {
    this.world.updateMatrixWorld(true);
    const box = this.contentBox();
    if (!box) return;
    const centre = box.getCenter(new THREE.Vector3());
    const radius = Math.max(box.getSize(new THREE.Vector3()).length() / 2, 0.5);
    const from = direction ?? this.camera.position.clone().sub(this.controls.target).normalize();
    this.controls.target.copy(centre);
    this.camera.position.copy(centre).addScaledVector(from, radius * 2.2);
    this.controls.update();
    this.dirty = true;
  }

  /** The bounds of what is drawn (the cloud, the mesh, the path, the tags), or null when nothing is. */
  private contentBox(): THREE.Box3 | null {
    const box = new THREE.Box3();
    let any = false;
    for (const object of [this.cloud, this.dense, this.path, this.fieldTags, this.mapTags]) {
      if (!object?.visible) continue;
      const b = new THREE.Box3().setFromObject(object);
      if (b.isEmpty()) continue;
      box.union(b);
      any = true;
    }
    return any ? box : null;
  }

  private updatePoints(points: MapSnapshot["points"] | null, visible: boolean) {
    if (!points || points.count === 0) {
      if (this.cloud) this.cloud.visible = false;
      return;
    }
    if (!this.cloud) {
      const geometry = new THREE.BufferGeometry();
      const material = new THREE.PointsMaterial({ size: 0.03, sizeAttenuation: true, vertexColors: true, transparent: true, opacity: 0.95 });
      this.disposables.push(geometry, material);
      this.cloud = new THREE.Points(geometry, material);
      this.world.add(this.cloud);
    }
    this.cloud.visible = visible;
    if (this.cloudSource !== points) {
      // Each snapshot is a new buffer: upload it once, with the grey of each point as its colour.
      const colours = new Float32Array(points.count * 3);
      for (let i = 0; i < points.count; i++) {
        const v = 0.32 + 0.68 * (points.grey[i] / 255);
        colours[i * 3] = v * 0.94;
        colours[i * 3 + 1] = v * 0.97;
        colours[i * 3 + 2] = v;
      }
      const geometry = this.cloud.geometry;
      geometry.setAttribute("position", new THREE.BufferAttribute(points.xyz.subarray(0, points.count * 3), 3));
      geometry.setAttribute("color", new THREE.BufferAttribute(colours, 3));
      geometry.setDrawRange(0, points.count);
      geometry.computeBoundingSphere();
      this.cloudSource = points;
    }
  }

  /**
   * The path, from the trajectory store's poses (decimated, and the corrections applied). Rebuilt only
   * when the store changed; the positions are read from it, not copied per frame.
   */
  private updatePath(trajectory: StreamedTrajectory | null, visible: boolean) {
    const poses = trajectory?.poses() ?? [];
    if (poses.length < 2) {
      if (this.path) this.path.visible = false;
      return;
    }
    if (!this.path) {
      const geometry = new THREE.BufferGeometry();
      const material = new THREE.LineBasicMaterial({ color: this.colours.trail, transparent: true, opacity: 0.9 });
      this.disposables.push(geometry, material);
      this.path = new THREE.Line(geometry, material);
      this.world.add(this.path);
    }
    this.path.visible = visible;
    if (this.trajectorySource === trajectory && this.trajectoryVersion === trajectory?.version) return;
    this.trajectorySource = trajectory;
    this.trajectoryVersion = trajectory?.version ?? -1;
    const positions = new Float32Array(poses.length * 3);
    poses.forEach((f, i) => {
      positions[i * 3] = f.pose.translation.x;
      positions[i * 3 + 1] = f.pose.translation.y;
      positions[i * 3 + 2] = f.pose.translation.z;
    });
    this.path.geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    this.path.geometry.computeBoundingSphere();
  }

  /** The keyframes as frustums, from the trajectory store's keyframe poses (the same version rule as the path). */
  private updateKeyframes(trajectory: StreamedTrajectory | null, visible: boolean) {
    this.keyframes.visible = visible;
    if (this.keyframesSource === trajectory && this.keyframesVersion === (trajectory?.version ?? -1)) return;
    this.keyframesSource = trajectory;
    this.keyframesVersion = trajectory?.version ?? -1;
    const vertices: number[] = [];
    for (const f of trajectory?.poses() ?? []) if (f.keyframe) appendFrustum(vertices, f.pose, KEYFRAME_DEPTH);
    this.keyframes.geometry.setAttribute("position", new THREE.Float32BufferAttribute(vertices, 3));
    this.keyframes.geometry.computeBoundingSphere();
  }

  /** A tag as a square with its id, in `group` (the tag's own frame: its normal is local +x for the field, +z for the map). */
  private tagSquare(size: number, colour: THREE.Color, opacity: number, label: string, className: string, normalAlongX: boolean): THREE.Group {
    const geometry = new THREE.PlaneGeometry(size, size);
    if (normalAlongX) geometry.rotateY(Math.PI / 2);
    const material = new THREE.MeshBasicMaterial({ color: colour, transparent: true, opacity, side: THREE.DoubleSide, depthWrite: false });
    const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), new THREE.LineBasicMaterial({ color: colour }));
    const group = new THREE.Group();
    group.add(new THREE.Mesh(geometry, material), edges);
    const span = document.createElement("span");
    span.className = className;
    span.textContent = label;
    const object = new CSS2DObject(span);
    object.position.set(0, size * 0.75, 0);
    group.add(object);
    return group;
  }

  /**
   * Empties a group of tag squares, freeing their GPU resources and label elements. The map's tags
   * are rebuilt with each snapshot (their poses refine), so what a rebuild replaces must go at once,
   * not when the pane closes.
   */
  private clearTags(group: THREE.Group, labels: CSS2DObject[]) {
    for (const label of labels) label.removeFromParent();
    for (const tag of [...group.children]) {
      group.remove(tag);
      tag.traverse((object) => {
        if (object instanceof THREE.Mesh || object instanceof THREE.LineSegments) {
          object.geometry.dispose();
          (object.material as THREE.Material).dispose();
        }
      });
    }
  }

  /** The map's own tags (where the mapper put them), without the field's ones when the layout draws them. */
  private updateMapTags(snapshot: MapSnapshot | null, fieldIds: ReadonlySet<number>, visible: boolean) {
    this.mapTags.visible = visible;
    const tags = (snapshot?.tags ?? []).filter((t) => !fieldIds.has(t.id));
    const size = snapshot?.tag_size_m ?? 0.1651;
    const key = `${size}:${tags.map((t) => t.id).join(",")}:${snapshot?.run_id ?? 0}:${snapshot?.seq ?? 0}`;
    if (key === this.mapTagKey) return;
    this.mapTagKey = key;
    this.clearTags(this.mapTags, this.mapTagLabels);
    this.mapTagLabels = [];
    for (const tag of tags) {
      const group = this.tagSquare(size, this.colours.placed, 0.28, `#${tag.id}`, "map-tag", false);
      group.position.copy(positionOf(tag.pose));
      group.quaternion.copy(quaternionOf(tag.pose));
      this.mapTags.add(group);
      const label = group.children.find((c): c is CSS2DObject => c instanceof CSS2DObject);
      if (label) this.mapTagLabels.push(label);
    }
  }

  /** The field layout's tags, where the field is (the map is aligned to it): squares facing as the layout has them. */
  private updateFieldTags(tags: FieldTag[], visible: boolean, size: number) {
    this.fieldTags.visible = visible;
    if (!visible) return;
    const key = `${size}:${tags.map((t) => `${t.id}:${t.x}:${t.y}:${t.z}:${t.yaw}:${t.pitch}`).join("|")}`;
    if (key === this.fieldTagKey) return;
    this.fieldTagKey = key;
    this.clearTags(this.fieldTags, this.fieldTagLabels);
    this.fieldTagLabels = [];
    for (const tag of tags) {
      const group = this.tagSquare(size, this.colours.field, 0.5, `#${tag.id}`, "map-tag field", true);
      group.position.set(tag.x, tag.y, tag.z);
      group.quaternion.copy(fieldTagQuaternion(tag));
      this.fieldTags.add(group);
      const label = group.children.find((c): c is CSS2DObject => c instanceof CSS2DObject);
      if (label) this.fieldTagLabels.push(label);
    }
  }

  /**
   * The dense mesh, one mesh per submap: each chunk is in submap-local coordinates and its pose places
   * it in the map's frame. Only what the store changed is rebuilt; a submap's geometry is uploaded once.
   */
  private updateDense(store: DenseChunkStore | null, shading: MeshShading, visible: boolean) {
    const material = shading === "normal" ? this.meshNormal : this.meshGrey;
    if (!store) {
      this.clearDense();
      this.dense.visible = false;
      this.denseSource = null;
      return;
    }
    if (store !== this.denseSource || store.version !== this.denseVersion) {
      this.denseSource = store;
      this.denseVersion = store.version;
      for (const [id, slot] of this.denseSlots) {
        const entry = store.submaps.get(id);
        if (!entry?.chunk) this.dropDenseSlot(id, slot);
      }
      for (const entry of store.submaps.values()) {
        if (!entry.chunk) continue;
        let slot = this.denseSlots.get(entry.submap);
        if (!slot) {
          slot = { mesh: new THREE.Mesh(new THREE.BufferGeometry(), this.meshGrey), chunk: null, pose: null };
          this.dense.add(slot.mesh);
          this.denseSlots.set(entry.submap, slot);
        }
        if (slot.chunk !== entry.chunk) {
          slot.chunk = entry.chunk;
          this.uploadChunk(slot.mesh, entry.chunk);
        }
        if (slot.pose !== entry.pose) {
          slot.pose = entry.pose;
          if (entry.pose) {
            slot.mesh.position.copy(positionOf(entry.pose.transform));
            slot.mesh.quaternion.copy(quaternionOf(entry.pose.transform));
          }
        }
        slot.mesh.visible = entry.pose !== null && entry.chunk.indices.length > 0;
      }
    }
    for (const slot of this.denseSlots.values()) slot.mesh.material = material;
    this.dense.visible = visible && this.denseSlots.size > 0;
  }

  /** A chunk's geometry, uploaded into `mesh` (its previous geometry freed). */
  private uploadChunk(mesh: THREE.Mesh, chunk: DenseMeshView) {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(chunk.positions, 3));
    geometry.setIndex(new THREE.BufferAttribute(chunk.indices, 1));
    if (chunk.normals) geometry.setAttribute("normal", new THREE.BufferAttribute(chunk.normals, 3));
    else geometry.computeVertexNormals();
    // Vertices without a grey are a neutral grey, so the grey shading still reads.
    const colours = chunk.grey ? greyColours(chunk.grey) : new Float32Array(chunk.vertices * 3).fill(0.62);
    geometry.setAttribute("color", new THREE.BufferAttribute(colours, 3));
    geometry.computeBoundingSphere();
    const previous = mesh.geometry;
    mesh.geometry = geometry;
    previous.dispose();
  }

  private dropDenseSlot(id: number, slot: DenseSlot) {
    this.dense.remove(slot.mesh);
    slot.mesh.geometry.dispose();
    this.denseSlots.delete(id);
  }

  private clearDense() {
    for (const [id, slot] of this.denseSlots) this.dropDenseSlot(id, slot);
    this.denseSource = null;
  }

  private updateCurrent(camera: MapTransform | null, visible: boolean) {
    if (!camera || !visible) {
      this.current.visible = false;
      this.currentDot.visible = false;
      return;
    }
    const vertices: number[] = [];
    appendFrustum(vertices, camera, CURRENT_DEPTH);
    this.current.geometry.setAttribute("position", new THREE.Float32BufferAttribute(vertices, 3));
    this.current.geometry.computeBoundingSphere();
    this.current.visible = true;
    this.currentDot.visible = true;
    this.currentDot.position.copy(positionOf(camera));
  }

  /** The robot's box at its pose on the field (z up, its origin on the floor). */
  private updateRobot(robot: MapTransform | null, visible: boolean) {
    if (!robot || !visible) {
      this.robot.visible = false;
      return;
    }
    this.robot.visible = true;
    this.robot.quaternion.copy(quaternionOf(robot));
    this.robot.position.copy(positionOf(robot)).add(new THREE.Vector3(0, 0, ROBOT_SIZE[2] / 2));
  }

  /**
   * The camera: the Field view is the orbit; the Camera view puts the scene's camera at the camera's
   * own pose, with its lens' field of view when it is known (the view keeps the lens' vertical field
   * and widens to fit the viewport's aspect).
   */
  private syncCamera() {
    const { w, h } = this.size;
    const aspect = w / h || 1;
    if (this.mode === "camera" && this.lens) {
      this.controls.enabled = false;
      const tanY = Math.tan(this.lens.fovY / 2);
      const tanX = tanY * this.lens.aspect;
      const fovY = aspect >= this.lens.aspect ? this.lens.fovY : 2 * Math.atan(tanX / aspect);
      this.camera.fov = THREE.MathUtils.radToDeg(fovY);
      this.camera.aspect = aspect;
      this.camera.updateProjectionMatrix();
    } else {
      this.controls.enabled = true;
      if (this.camera.fov !== FIELD_FOV || this.camera.aspect !== aspect) {
        this.camera.fov = FIELD_FOV;
        this.camera.aspect = aspect;
        this.camera.updateProjectionMatrix();
      }
    }
    this.dirty = true;
  }

  /** Places the scene's camera at the camera pose (Camera view), each frame the pose changes. */
  private placeCamera(pose: MapTransform | null) {
    if (this.mode !== "camera" || !pose) return;
    this.world.updateMatrix();
    const local = new THREE.Matrix4().compose(positionOf(pose), quaternionOf(pose), new THREE.Vector3(1, 1, 1));
    const m = new THREE.Matrix4().multiplyMatrices(this.world.matrix, local).multiply(OPTICAL_TO_SCENE);
    m.decompose(this.camera.position, this.camera.quaternion, this.camera.scale);
    this.camera.updateMatrixWorld(true);
  }

  private resize() {
    const { clientWidth: w, clientHeight: h } = this.container;
    if (!w || !h) return;
    this.size = { w, h };
    this.renderer.setSize(w, h, false);
    this.labels.setSize(w, h);
    this.syncCamera();
  }

  private loop = () => {
    this.raf = requestAnimationFrame(this.loop);
    // The Camera view stands at the camera's pose: the orbit controls must not move it.
    let moved = false;
    if (this.mode === "camera") this.placeCamera(this.lastPose);
    else moved = this.controls.update();
    if (!this.dirty && !moved) return;
    this.dirty = false;
    this.renderer.render(this.scene, this.camera);
    this.labels.render(this.scene, this.camera);
  };

  /** Stops the loop and frees the GPU resources. */
  dispose() {
    cancelAnimationFrame(this.raf);
    this.resizeObserver.disconnect();
    this.controls.dispose();
    this.clearDense();
    this.clearTags(this.mapTags, this.mapTagLabels);
    this.clearTags(this.fieldTags, this.fieldTagLabels);
    for (const d of this.disposables) d.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
    this.labels.domElement.remove();
  }
}
