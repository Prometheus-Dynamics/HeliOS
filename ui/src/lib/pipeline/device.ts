// Where a pipeline's calls go. A peer's pipeline is keyed `<node>/<id>` in the cluster (every board
// has a pipeline `default`), so its id on its own device is the key without the node, and its calls
// go through the gateway to that peer. This device's pipelines keep their ids and its own API.

import { api, scoped, type HeliosApi } from "#lib/api/client.js";
import { deviceId } from "#lib/api/peers.js";

/** The device a pipeline runs on: the pipeline's id there, and the API that reaches that device. */
export interface PipelineDevice {
  id: string;
  scope: HeliosApi;
}

/** A pipeline's device, from its node, its cluster key, and whether its node is a routed peer. */
export function pipelineDevice(nodeId: string, key: string, remote: boolean): PipelineDevice {
  return { id: deviceId(nodeId, key), scope: remote ? scoped(nodeId) : api };
}
