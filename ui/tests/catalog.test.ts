import { expect, test } from "bun:test";
import { CATALOG, CATALOG_BY_ID, mergeDeviceCatalog } from "../src/lib/api/catalog";

test("the device's catalog becomes the palette", () => {
  mergeDeviceCatalog({
    format: "helios.node-catalog",
    schema_version: 1,
    plugins: [],
    nodes: [
      { id: "io.host_bridge", plugin: "daedalus.builtin.host_boundary", inputs: [], outputs: [] },
      {
        id: "eidos:filter.sobel_new",
        plugin: "eidos",
        summary: "Sobel gradient.",
        inputs: [
          { name: "frame", type_key: "styx:framelease" },
          { name: "radius", type_key: 'typeexpr:{"Scalar":"USize"}', default: 2, min: 1, max: 7, description: "Kernel radius." },
          { name: "gain", type_key: 'typeexpr:{"Scalar":"Float"}', default: 1.5 },
          { name: "border", type_key: 'typeexpr:{"Enum":[]}', default: "clamp", options: ["clamp", "mirror"] },
          { name: "signed", type_key: 'typeexpr:{"Scalar":"Bool"}', default: false },
        ],
        outputs: [{ name: "frame", type_key: "styx:framelease" }],
      },
      { id: "eidos:aruco.mask_prep_runs", plugin: "eidos", inputs: [{ name: "frame", type_key: "styx:framelease" }, { name: "radius", type_key: 'typeexpr:{"Scalar":"USize"}', default: 3 }], outputs: [{ name: "runs", type_key: "eidos:runs" }] },
    ],
  });
  const sobel = CATALOG_BY_ID["eidos:filter.sobel_new"];
  expect(sobel.title).toBe("Sobel new");
  expect(sobel.category).toBe("Filter");
  expect(sobel.inputs).toEqual([{ name: "frame", type: "styx:framelease" }]);
  expect(sobel.params.map((p) => p.kind)).toEqual(["int", "float", "enum", "bool"]);
  expect(sobel.params[0]).toMatchObject({ min: 1, max: 7, default: 2, help: "Kernel radius." });
  // Known nodes keep the UI's title, and take the device's ports and parameters.
  expect(CATALOG_BY_ID["eidos:aruco.mask_prep_runs"].title).toBe("Mask prep (runs)");
  expect(CATALOG_BY_ID["eidos:aruco.mask_prep_runs"].params.map((p) => p.name)).toEqual(["radius"]);
  // A bare declaration keeps the UI's entry; UI-level nodes stay offered; the rest are hidden.
  expect(CATALOG_BY_ID["io.host_bridge"].available).toBe(true);
  expect(CATALOG_BY_ID["styx:camera"].available).toBe(true);
  expect(CATALOG_BY_ID["eidos:mask.otsu"].available).toBe(false);
  expect(CATALOG.length).toBe(Object.keys(CATALOG_BY_ID).length);
});
