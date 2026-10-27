// Where a pipeline's calls go: a peer's pipeline (keyed `<node>/<id>`) through the gateway, under its
// id on the peer; this device's pipelines on its own API under their own ids.
import { expect, test } from "bun:test";
import { api } from "../src/lib/api/client";
import { pipelineDevice } from "../src/lib/pipeline/device";

/** The requests a call makes, as `METHOD path` (the response is an empty object). */
async function requestsOf(call: () => Promise<unknown>): Promise<string[]> {
  const seen: string[] = [];
  const original = globalThis.fetch;
  globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
    seen.push(`${init?.method ?? "GET"} ${String(input)}`);
    return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
  }) as typeof fetch;
  try {
    await call();
  } finally {
    globalThis.fetch = original;
  }
  return seen;
}

test("a peer's pipeline plan is read through the gateway, under the peer's node and the pipeline's own id", async () => {
  const device = pipelineDevice("raze-37bc", "raze-37bc/default", true);
  expect(device.id).toBe("default");
  expect(await requestsOf(() => device.scope.pipelinePlan(device.id))).toEqual(["GET /v1/peers/raze-37bc/pipelines/default/plan"]);
});

test("a peer's pipeline changes go through the gateway too", async () => {
  const device = pipelineDevice("raze-37bc", "raze-37bc/default", true);
  expect(await requestsOf(() => device.scope.restartPipeline(device.id))).toEqual(["POST /v1/peers/raze-37bc/pipelines/default/restart"]);
});

test("this device's pipeline keeps its id and its own API", async () => {
  const device = pipelineDevice("raze-owner", "default", false);
  expect(device.id).toBe("default");
  expect(device.scope).toBe(api);
  expect(await requestsOf(() => device.scope.pipelinePlan(device.id))).toEqual(["GET /v1/pipelines/default/plan"]);
});

test("a peer's pipeline id is never sent to this device as a keyed id", async () => {
  const device = pipelineDevice("raze-37bc", "raze-37bc/default", true);
  const seen = await requestsOf(() => device.scope.pipelinePlan(device.id));
  expect(seen.some((request) => request.includes("raze-37bc%2Fdefault") || request.startsWith("GET /v1/pipelines/"))).toBe(false);
});
