// A low-poly FRC-style chassis in the robot frame (x forward, y left, z up):
// a 0.8 m square frame, bumpers, swerve modules, a small mast and an arrow
// showing which way is forward. Muted tones; the accent is for selection.

import * as THREE from "three";

const FRAME = 0.8;
const BUMPER_T = 0.085;
const BUMPER_H = 0.12;

export function buildRobot(disposables: { dispose(): void }[]): THREE.Group {
  const robot = new THREE.Group();
  const own = <T extends { dispose(): void }>(x: T) => (disposables.push(x), x);

  const bumper = own(new THREE.MeshStandardMaterial({ color: 0x3a4152, roughness: 0.85 }));
  const metal = own(new THREE.MeshStandardMaterial({ color: 0x7b8394, roughness: 0.45, metalness: 0.55 }));
  const dark = own(new THREE.MeshStandardMaterial({ color: 0x20242c, roughness: 0.8 }));
  const rubber = own(new THREE.MeshStandardMaterial({ color: 0x14161a, roughness: 0.95 }));
  const trim = own(new THREE.MeshStandardMaterial({ color: 0x8b93a5, roughness: 0.6 }));
  const edge = own(new THREE.LineBasicMaterial({ color: 0x5a6274, transparent: true, opacity: 0.5 }));

  function box(w: number, d: number, h: number, mat: THREE.Material, x: number, y: number, z: number, outline = false) {
    const geo = own(new THREE.BoxGeometry(w, d, h));
    const mesh = new THREE.Mesh(geo, mat);
    mesh.position.set(x, y, z);
    robot.add(mesh);
    if (outline) {
      const lines = new THREE.LineSegments(own(new THREE.EdgesGeometry(geo)), edge);
      lines.position.copy(mesh.position);
      robot.add(lines);
    }
    return mesh;
  }

  // Soft contact shadow.
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 128;
  const ctx = canvas.getContext("2d");
  if (ctx) {
    const g = ctx.createRadialGradient(64, 64, 10, 64, 64, 64);
    g.addColorStop(0, "rgba(0,0,0,0.55)");
    g.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, 128, 128);
  }
  const shadowTex = own(new THREE.CanvasTexture(canvas));
  const shadow = new THREE.Mesh(
    own(new THREE.PlaneGeometry(1.8, 1.8)),
    own(new THREE.MeshBasicMaterial({ map: shadowTex, transparent: true, depthWrite: false })),
  );
  shadow.position.z = 0.003;
  robot.add(shadow);

  // Bumpers (outer ring) and frame rails.
  const outer = FRAME + 2 * BUMPER_T;
  const bz = 0.035 + BUMPER_H / 2;
  box(outer, BUMPER_T, BUMPER_H, bumper, 0, FRAME / 2 + BUMPER_T / 2, bz, true);
  box(outer, BUMPER_T, BUMPER_H, bumper, 0, -FRAME / 2 - BUMPER_T / 2, bz, true);
  box(BUMPER_T, FRAME, BUMPER_H, bumper, FRAME / 2 + BUMPER_T / 2, 0, bz, true);
  box(BUMPER_T, FRAME, BUMPER_H, bumper, -FRAME / 2 - BUMPER_T / 2, 0, bz, true);
  const rail = 0.05;
  box(FRAME, rail, rail, metal, 0, FRAME / 2 - rail / 2, 0.11);
  box(FRAME, rail, rail, metal, 0, -FRAME / 2 + rail / 2, 0.11);
  box(rail, FRAME - 2 * rail, rail, metal, FRAME / 2 - rail / 2, 0, 0.11);
  box(rail, FRAME - 2 * rail, rail, metal, -FRAME / 2 + rail / 2, 0, 0.11);
  box(FRAME - 0.04, FRAME - 0.04, 0.008, dark, 0, 0, 0.088);

  // Swerve modules at the corners.
  const wheelGeo = own(new THREE.CylinderGeometry(0.048, 0.048, 0.035, 14));
  for (const sx of [-1, 1]) {
    for (const sy of [-1, 1]) {
      const x = sx * (FRAME / 2 - 0.085);
      const y = sy * (FRAME / 2 - 0.085);
      const wheel = new THREE.Mesh(wheelGeo, rubber);
      wheel.position.set(x, y, 0.048);
      robot.add(wheel);
      box(0.1, 0.1, 0.05, metal, x, y, 0.155, true);
      box(0.05, 0.05, 0.05, dark, x, y, 0.205);
    }
  }

  // Electronics bay, battery and a small superstructure.
  box(0.28, 0.22, 0.06, dark, 0.06, 0, 0.122, true);
  box(0.18, 0.08, 0.1, rubber, -0.22, 0, 0.14);
  for (const sy of [-1, 1]) box(0.04, 0.04, 0.52, metal, -0.12, sy * 0.24, 0.35);
  box(0.06, 0.52, 0.04, metal, -0.12, 0, 0.6, true);

  // Forward arrow on the deck.
  const shape = new THREE.Shape();
  shape.moveTo(0.2, 0);
  shape.lineTo(0.07, 0.09);
  shape.lineTo(0.07, 0.035);
  shape.lineTo(-0.06, 0.035);
  shape.lineTo(-0.06, -0.035);
  shape.lineTo(0.07, -0.035);
  shape.lineTo(0.07, -0.09);
  shape.closePath();
  const arrow = new THREE.Mesh(own(new THREE.ShapeGeometry(shape)), trim);
  arrow.position.set(0.12, 0, 0.154);
  robot.add(arrow);

  return robot;
}
