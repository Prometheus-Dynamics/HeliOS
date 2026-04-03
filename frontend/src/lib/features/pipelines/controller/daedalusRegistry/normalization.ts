import type { DaedalusRegistryNode, DaedalusRegistryFanInPort } from '$lib/api/client';
import type { PipelineRegistryEntry } from '$lib/types/pipeline';
import { buildTypeRegistryLookup } from './cache';
import type { DaedalusRegistryPort, DaedalusRegistryType } from './types';
import {
  applyPortDefaults,
  extractGpuMetadata,
  extractPortDescriptions,
  extractPortMetadata,
  normalizeFanInPorts,
  normalizedString,
  toPortMap
} from './parsing';

const normalizedRegistryCache = new WeakMap<
  DaedalusRegistryNode[],
  { types: DaedalusRegistryType[] | null | undefined; entries: PipelineRegistryEntry[] }
>();

export function normalizeDaedalusRegistry(
  nodes: DaedalusRegistryNode[],
  types?: DaedalusRegistryType[] | null
): PipelineRegistryEntry[] {
  const cached = normalizedRegistryCache.get(nodes);
  if (cached && cached.types === types) {
    return cached.entries;
  }

  const typeRegistry = buildTypeRegistryLookup(types);
  const entries = (nodes ?? [])
    .map((node) => {
      const metadataRecord = (node?.metadata ?? {}) as Record<string, unknown>;
      const id = typeof node?.id === 'string' && node.id.trim() ? node.id.trim() : null;
      if (!id) return null;
      const name = typeof node.label === 'string' && node.label.trim() ? node.label.trim() : id;
      const summary = normalizedString(metadataRecord['summary']);
      const gpu = extractGpuMetadata(metadataRecord);
      const descriptions = extractPortDescriptions(metadataRecord);
      const portMetadata = extractPortMetadata(metadataRecord);
      const faninInputs = normalizeFanInPorts(
        (node as unknown as { fanin_inputs?: DaedalusRegistryFanInPort[] }).fanin_inputs,
        typeRegistry
      );
      if (Array.isArray(node.input_ports)) {
        applyPortDefaults(portMetadata.inputs, node.input_ports as unknown as Array<string | DaedalusRegistryPort>);
      }
      if (Array.isArray(node.output_ports)) {
        applyPortDefaults(portMetadata.outputs, node.output_ports as unknown as Array<string | DaedalusRegistryPort>);
      }
      const tags = Array.isArray(node.feature_flags)
        ? node.feature_flags.filter((flag): flag is string => typeof flag === 'string' && flag.trim().length > 0)
        : [];
      const categories = [];
      const entry: PipelineRegistryEntry = {
        id,
        metadata: {
          name,
          ...(typeof node.plugin === 'string' && node.plugin.trim() ? { provider: node.plugin.trim() } : {}),
          ...(summary ? { summary } : {}),
          ...(gpu ? { gpu } : {}),
          ...(tags.length > 0 ? { tags } : {}),
          ...(categories.length > 0 ? { categories } : {}),
          ...(Object.keys(portMetadata.inputs).length > 0 ? { inputPorts: portMetadata.inputs } : {}),
          ...(Object.keys(portMetadata.outputs).length > 0 ? { outputPorts: portMetadata.outputs } : {})
        },
        inputs: toPortMap(
          Array.isArray(node.input_ports)
            ? (node.input_ports ?? [])
            : Array.isArray(node.inputs)
              ? node.inputs
              : [],
          descriptions.inputs,
          typeRegistry
        ),
        outputs: toPortMap(
          Array.isArray(node.output_ports)
            ? (node.output_ports ?? [])
            : Array.isArray(node.outputs)
              ? node.outputs
              : [],
          descriptions.outputs,
          typeRegistry
        )
      };
      if (faninInputs.length > 0) {
        entry.faninInputs = faninInputs;
      }
      return entry;
    })
    .filter((entry): entry is PipelineRegistryEntry => Boolean(entry));

  normalizedRegistryCache.set(nodes, { types, entries });
  return entries;
}
