// CAD/mesh loading, split into its own chunk: the loaders only download when
// someone loads a model.

import * as THREE from "three";

export async function loadModelFile(file: File, ext: string): Promise<{ object: THREE.Object3D; parts: number; triangles: number; guessUnit: number }> {
  const buffer = await file.arrayBuffer();
  let object: THREE.Object3D;
  switch (ext) {
    case "glb":
    case "gltf": {
      const { GLTFLoader } = await import("three/examples/jsm/loaders/GLTFLoader.js");
      const gltf = await new GLTFLoader().parseAsync(ext === "glb" ? buffer : new TextDecoder().decode(buffer), "");
      object = gltf.scene;
      break;
    }
    case "stl": {
      const { STLLoader } = await import("three/examples/jsm/loaders/STLLoader.js");
      const geo = new STLLoader().parse(buffer);
      geo.computeVertexNormals();
      object = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color: 0x8b93a5, roughness: 0.6, metalness: 0.2 }));
      object.name = file.name.replace(/\.stl$/i, "");
      break;
    }
    case "obj": {
      const { OBJLoader } = await import("three/examples/jsm/loaders/OBJLoader.js");
      object = new OBJLoader().parse(new TextDecoder().decode(buffer));
      break;
    }
    case "ply": {
      const { PLYLoader } = await import("three/examples/jsm/loaders/PLYLoader.js");
      const geo = new PLYLoader().parse(buffer);
      geo.computeVertexNormals();
      object = new THREE.Mesh(geo, new THREE.MeshStandardMaterial({ color: 0x8b93a5, roughness: 0.6, vertexColors: geo.hasAttribute("color") }));
      break;
    }
    case "3mf": {
      const { ThreeMFLoader } = await import("three/examples/jsm/loaders/3MFLoader.js");
      object = new ThreeMFLoader().parse(buffer);
      break;
    }
    default:
      throw new Error(`.${ext} is not a supported model format`);
  }

  let parts = 0;
  let triangles = 0;
  object.traverse((o) => {
    if (o instanceof THREE.Mesh) {
      parts++;
      const g = o.geometry as THREE.BufferGeometry;
      triangles += (g.index ? g.index.count : g.getAttribute("position").count) / 3;
      if (!o.name) o.name = `part ${parts}`;
      // Plain, readable materials; CAD exports often come out black or glossy.
      const mats = Array.isArray(o.material) ? o.material : [o.material];
      for (const m of mats) if (m instanceof THREE.MeshStandardMaterial) m.side = THREE.DoubleSide;
    }
  });

  // Guess units from the size: an FRC robot is ~1 m across.
  const size = new THREE.Box3().setFromObject(object).getSize(new THREE.Vector3());
  const extent = Math.max(size.x, size.y, size.z);
  const guessUnit = extent > 200 ? 0.001 : extent > 20 ? 0.0254 : extent > 4 ? 0.01 : 1;
  return { object, parts, triangles: Math.round(triangles), guessUnit };
}
