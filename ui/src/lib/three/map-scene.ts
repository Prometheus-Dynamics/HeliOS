// The live map in 3D: the point cloud (grey per point), the camera's trajectory, the keyframes
// as small frustums, the tags as squares with their ids, the current camera as a highlighted
// frustum, and the field's axes once the map is aligned. Orbit controls; the scene draws the
// map frame as the mapper gives it (the first clear tag, or the field). It owns its GPU
// resources and frees them in `dispose`.

import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { CSS2DObject, CSS2DRenderer } from "three/examples/jsm/renderers/CSS2DRenderer.js";
import type { MapPose, MapSnapshot, MapTransform } from "#lib/vision/map.js";
import { cssVarColor } from "./colors";

/** Half-angles of the drawn frustums, radians (a 60 x 45 degree view). */
const HALF_H = THREE.MathUtils.degToRad(30);
const HALF_V = THREE.MathUtils.degToRad(22.5);
/** Depth of a keyframe's frustum, metres, and of the current camera's. */
const KEYFRAME_DEPTH = 0.09;
const CURRENT_DEPTH = 0.22;

export interface MapSceneData {
  snapshot: MapSnapshot | null;
  pose: MapPose | null;
  /** The field layout's tag ids: tags with these ids are drawn as field tags. */
  fieldIds: ReadonlySet<number>;
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

export class MapScene {
  private renderer: THREE.WebGLRenderer;
  private labels: CSS2DRenderer;
  private scene = new THREE.Scene();
  private camera = new THREE.PerspectiveCamera(50, 1, 0.02, 80);
  private controls: OrbitControls;
  private disposables: { dispose(): void }[] = [];
  private cloud: THREE.Points | null = null;
  private cloudSource: MapSnapshot["points"] | null = null;
  private trajectory: THREE.Line | null = null;
  private keyframes: THREE.LineSegments;
  private current: THREE.LineSegments;
  private currentDot: THREE.Mesh;
  private tags = new THREE.Group();
  private tagLabels: CSS2DObject[] = [];
  private axes = new THREE.AxesHelper(0.5);
  private floor: THREE.GridHelper;
  private colours: { accent: THREE.Color; field: THREE.Color; placed: THREE.Color; trail: THREE.Color; keyframe: THREE.Color };
  private raf = 0;
  private resizeObserver: ResizeObserver;
  private dirty = true;
  private lastTagKey = "";

  constructor(private container: HTMLElement) {
    const bg = new THREE.Color(cssVarColor("var(--bg)", "#121419"));
    this.colours = {
      accent: new THREE.Color(cssVarColor("var(--accent)", "#ff6b6b")),
      field: new THREE.Color(cssVarColor("var(--accent)", "#ff6b6b")),
      placed: new THREE.Color(cssVarColor("var(--fg-2)", "#c7cad3")),
      trail: new THREE.Color(cssVarColor("var(--fg-3)", "#8a909c")),
      keyframe: new THREE.Color(cssVarColor("var(--fg-2)", "#c7cad3")),
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
    this.scene.fog = new THREE.Fog(bg, 9, 22);

    this.floor = new THREE.GridHelper(10, 20, 0x2a2f3a, 0x1d2129);
    const floorMaterial = this.floor.material as THREE.Material;
    floorMaterial.transparent = true;
    floorMaterial.opacity = 0.8;
    floorMaterial.depthWrite = false;
    this.disposables.push(this.floor.geometry, floorMaterial);
    this.scene.add(this.floor);

    this.keyframes = this.lines(this.colours.keyframe, 0.5);
    this.current = this.lines(this.colours.accent, 1);
    const dotGeometry = new THREE.SphereGeometry(0.018, 12, 8);
    const dotMaterial = new THREE.MeshBasicMaterial({ color: this.colours.accent });
    this.currentDot = new THREE.Mesh(dotGeometry, dotMaterial);
    this.disposables.push(dotGeometry, dotMaterial);
    this.scene.add(this.keyframes, this.current, this.currentDot, this.tags);

    this.axes.visible = false;
    this.scene.add(this.axes);

    this.camera.position.set(3.4, 2.4, 3.6);
    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.target.set(0, 0.8, 0);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.minDistance = 0.4;
    this.controls.maxDistance = 30;
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

  /** Shows the map's latest snapshot and pose. */
  update(data: MapSceneData) {
    this.updateCloud(data.snapshot);
    this.updateTrajectory(data.snapshot);
    this.updateKeyframes(data.snapshot);
    this.updateTags(data.snapshot, data.fieldIds);
    this.updateCurrent(data.pose);
    this.axes.visible = data.snapshot?.aligned === true;
    this.dirty = true;
  }

  private updateCloud(snapshot: MapSnapshot | null) {
    const points = snapshot?.points ?? null;
    if (!points || points.count === 0) {
      if (this.cloud) this.cloud.visible = false;
      return;
    }
    if (!this.cloud) {
      const geometry = new THREE.BufferGeometry();
      const material = new THREE.PointsMaterial({ size: 0.018, sizeAttenuation: true, vertexColors: true, transparent: true, opacity: 0.8 });
      this.disposables.push(geometry, material);
      this.cloud = new THREE.Points(geometry, material);
      this.scene.add(this.cloud);
    }
    this.cloud.visible = true;
    if (this.cloudSource !== points) {
      // Each snapshot is a new buffer: upload it once, with the grey of each point as its colour.
      const colours = new Float32Array(points.count * 3);
      for (let i = 0; i < points.count; i++) colours.fill(points.grey[i] / 255, i * 3, i * 3 + 3);
      const geometry = this.cloud.geometry;
      geometry.setAttribute("position", new THREE.BufferAttribute(points.xyz.subarray(0, points.count * 3), 3));
      geometry.setAttribute("color", new THREE.BufferAttribute(colours, 3));
      geometry.setDrawRange(0, points.count);
      geometry.computeBoundingSphere();
      this.cloudSource = points;
    }
  }

  private updateTrajectory(snapshot: MapSnapshot | null) {
    const path = snapshot?.trajectory ?? [];
    if (path.length < 2) {
      if (this.trajectory) this.trajectory.visible = false;
      return;
    }
    const positions = new Float32Array(path.length * 3);
    path.forEach((f, i) => positions.set([f.pose.translation.x, f.pose.translation.y, f.pose.translation.z], i * 3));
    if (!this.trajectory) {
      const geometry = new THREE.BufferGeometry();
      const material = new THREE.LineBasicMaterial({ color: this.colours.trail, transparent: true, opacity: 0.9 });
      this.disposables.push(geometry, material);
      this.trajectory = new THREE.Line(geometry, material);
      this.scene.add(this.trajectory);
    }
    this.trajectory.visible = true;
    this.trajectory.geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    this.trajectory.geometry.computeBoundingSphere();
  }

  private updateKeyframes(snapshot: MapSnapshot | null) {
    const vertices: number[] = [];
    for (const f of snapshot?.trajectory ?? []) if (f.keyframe) appendFrustum(vertices, f.pose, KEYFRAME_DEPTH);
    this.keyframes.geometry.setAttribute("position", new THREE.Float32BufferAttribute(vertices, 3));
    this.keyframes.geometry.computeBoundingSphere();
  }

  private updateTags(snapshot: MapSnapshot | null, fieldIds: ReadonlySet<number>) {
    const tags = snapshot?.tags ?? [];
    const size = snapshot?.tag_size_m ?? 0.1651;
    const key = `${size}:${tags.map((t) => `${t.id}/${fieldIds.has(t.id) ? 1 : 0}`).join(",")}:${snapshot?.run_id ?? 0}`;
    if (key === this.lastTagKey) return;
    this.lastTagKey = key;
    for (const child of [...this.tags.children]) this.tags.remove(child);
    for (const label of this.tagLabels) label.removeFromParent();
    this.tagLabels = [];
    for (const tag of tags) {
      const field = fieldIds.has(tag.id);
      const colour = field ? this.colours.field : this.colours.placed;
      const geometry = new THREE.PlaneGeometry(size, size);
      const material = new THREE.MeshBasicMaterial({ color: colour, transparent: true, opacity: field ? 0.5 : 0.28, side: THREE.DoubleSide, depthWrite: false });
      const edges = new THREE.LineSegments(new THREE.EdgesGeometry(geometry), new THREE.LineBasicMaterial({ color: colour }));
      const square = new THREE.Mesh(geometry, material);
      const group = new THREE.Group();
      group.add(square, edges);
      group.position.copy(positionOf(tag.pose));
      group.quaternion.copy(quaternionOf(tag.pose));
      this.tags.add(group);
      this.disposables.push(geometry, material, edges.geometry, edges.material as THREE.Material);
      const label = document.createElement("span");
      label.className = field ? "map-tag field" : "map-tag";
      label.textContent = `#${tag.id}`;
      const object = new CSS2DObject(label);
      object.position.set(0, size * 0.75, 0);
      group.add(object);
      this.tagLabels.push(object);
    }
  }

  private updateCurrent(pose: MapPose | null) {
    const estimate = pose?.valid ? pose.estimate : null;
    if (!estimate) {
      this.current.visible = false;
      this.currentDot.visible = false;
      return;
    }
    const vertices: number[] = [];
    appendFrustum(vertices, estimate.reference_from_camera, CURRENT_DEPTH);
    this.current.geometry.setAttribute("position", new THREE.Float32BufferAttribute(vertices, 3));
    this.current.geometry.computeBoundingSphere();
    this.current.visible = true;
    this.currentDot.visible = true;
    this.currentDot.position.copy(positionOf(estimate.reference_from_camera));
  }

  private resize() {
    const { clientWidth: w, clientHeight: h } = this.container;
    if (!w || !h) return;
    this.renderer.setSize(w, h, false);
    this.labels.setSize(w, h);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.dirty = true;
  }

  private loop = () => {
    this.raf = requestAnimationFrame(this.loop);
    const moved = this.controls.update();
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
    for (const d of this.disposables) d.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
    this.labels.domElement.remove();
  }
}
