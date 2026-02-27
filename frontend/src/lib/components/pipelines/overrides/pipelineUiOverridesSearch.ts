import type { PipelineUi, PipelineUiItem, PipelineUiControl, PipelineUiNodeDescriptor } from '$lib/features/pipelines/pipelineUiTypes';
import { isBooleanTypeKey, isNumericTypeKey, isPixelTypeKey, normalizeTypeKey } from '$lib/components/flow/pipeline-graph/editorUtils';
import { getDataTypeVariants, getMetadataEnumOptions, isDataTypeSettable, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';

export function parseBindValue(bind: string): { nodeId: string; portKey: string } | null {
  const trimmed = bind.trim();
  const dotIndex = trimmed.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex === trimmed.length - 1) return null;
  const nodeId = trimmed.slice(0, dotIndex).trim();
  const portKey = trimmed.slice(dotIndex + 1).trim();
  if (!nodeId.length || !portKey.length) return null;
  return { nodeId, portKey };
}

export function resolveDescriptorForBind(
  bind: string,
  nodeDescriptors: PipelineUiNodeDescriptor[]
): PipelineUiNodeDescriptor | null {
  const parsed = parseBindValue(bind);
  if (!parsed) return null;
  return (
    nodeDescriptors.find(
        (entry) => {
          if (entry.portKey.trim().toLowerCase() !== parsed.portKey.trim().toLowerCase()) return false;
          if (entry.nodeId === parsed.nodeId) return true;
          if (typeof entry.backendId === 'string' && entry.backendId === parsed.nodeId) return true;
          return typeof entry.sourceId === 'string' && entry.sourceId === parsed.nodeId;
        }
    ) ?? null
  );
}

export function hasEnumOptions(descriptor: PipelineUiNodeDescriptor): boolean {
  const allowedValues = getMetadataEnumOptions(descriptor.metadata);
  if (allowedValues.length > 0) return true;
  const meta = descriptor.metadata as { uiControl?: unknown; ui_control?: unknown } | null;
  const uiControl =
    typeof meta?.uiControl === 'string'
      ? meta.uiControl
      : typeof meta?.ui_control === 'string'
        ? meta.ui_control
        : null;
  if (uiControl && ['select', 'enum', 'dropdown'].includes(uiControl.toLowerCase())) return true;
  const variants = getDataTypeVariants(descriptor.dataType ?? undefined);
  if (variants.length > 0) return true;
  const typeKey = resolveDataTypeKey(descriptor.dataType ?? undefined);
  return (typeKey ?? '').toLowerCase() === 'enum';
}

export function isControlCompatible(
  control: PipelineUiControl | PipelineUiControl['type'],
  descriptor: PipelineUiNodeDescriptor
): boolean {
  const controlType = typeof control === 'string' ? control : control.type;
  const controlOptions = typeof control === 'string' ? null : control.options;
  if (controlType === 'layout_toggle') return false;
  if (!descriptor.dataType || !isDataTypeSettable(descriptor.dataType)) return false;
  const typeKey = resolveDataTypeKey(descriptor.dataType ?? undefined);
  const normalizedTypeKey = normalizeTypeKey(typeKey);
  const isNumeric = isNumericTypeKey(typeKey);
  const isBool = isBooleanTypeKey(typeKey);
  const isPixel = isPixelTypeKey(typeKey);
  const isEnum = hasEnumOptions(descriptor);
  const hasControlOptions = Array.isArray(controlOptions) && controlOptions.length > 0;
  if (controlType === 'slider' || controlType === 'dual_slider') return isNumeric;
  if (controlType === 'toggle') return isBool;
  if (controlType === 'select') {
    if (isPixel || isBool) return false;
    if (isEnum || hasControlOptions) return true;
    return normalizedTypeKey === 'string' || normalizedTypeKey === 'str' || normalizedTypeKey === 'text';
  }
  if (controlType === 'color') return isPixel;
  if (controlType === 'input') return !isPixel && !isEnum;
  return false;
}

export function controlRequiresBinding(control: PipelineUiControl): boolean {
  return (
    control.type === 'slider' ||
    control.type === 'dual_slider' ||
    control.type === 'toggle' ||
    control.type === 'select' ||
    control.type === 'input' ||
    control.type === 'color'
  );
}

export function collectControls(items: ReadonlyArray<PipelineUiItem>): PipelineUiControl[] {
  const controls: PipelineUiControl[] = [];
  for (const item of items) {
    if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
      controls.push(...collectControls(item.items));
    } else if (item.type === 'tabs') {
      item.tabs.forEach((tab) => controls.push(...collectControls(tab.content)));
    } else if (
      item.type === 'slider' ||
      item.type === 'dual_slider' ||
      item.type === 'toggle' ||
      item.type === 'layout_toggle' ||
      item.type === 'select' ||
      item.type === 'input' ||
      item.type === 'color' ||
      item.type === 'hsv' ||
      item.type === 'hsv_range'
    ) {
      controls.push(item as PipelineUiControl);
    }
  }
  return controls;
}

export function matchesQuery(value: string | number | boolean | null | undefined, query: string): boolean {
  if (!value || !query) return false;
  return String(value).toLowerCase().includes(query);
}

export function itemMatchesQuery(item: PipelineUiItem, query: string): boolean {
  if (!query) return true;
  if (matchesQuery(item.id, query)) return true;
  switch (item.type) {
    case 'title':
    case 'text':
      return matchesQuery(item.text, query);
    case 'divider':
      return matchesQuery(item.label, query);
    case 'group':
    case 'accordion':
    case 'stack':
      return matchesQuery(item.title, query) || matchesQuery(item.description, query);
    case 'tabs':
      return (
        matchesQuery(item.title, query) ||
        matchesQuery(item.description, query) ||
        item.tabs.some((tab) => matchesQuery(tab.title, query))
      );
    default: {
      const control = item as PipelineUiControl;
      if (matchesQuery(control.label, query)) return true;
      if (matchesQuery(control.help, query)) return true;
      if (typeof control.bind === 'string' && matchesQuery(control.bind, query)) return true;
      if (control.bind && typeof control.bind === 'object') {
        const bind = control.bind as Record<string, unknown>;
        if (typeof bind.min === 'string' && matchesQuery(bind.min, query)) return true;
        if (typeof bind.max === 'string' && matchesQuery(bind.max, query)) return true;
        if (typeof bind.h === 'string' && matchesQuery(bind.h, query)) return true;
        if (typeof bind.s === 'string' && matchesQuery(bind.s, query)) return true;
        if (typeof bind.v === 'string' && matchesQuery(bind.v, query)) return true;
        if (bind.h && typeof bind.h === 'object') {
          const h = bind.h as Record<string, unknown>;
          if (typeof h.min === 'string' && matchesQuery(h.min, query)) return true;
          if (typeof h.max === 'string' && matchesQuery(h.max, query)) return true;
        }
        if (bind.s && typeof bind.s === 'object') {
          const s = bind.s as Record<string, unknown>;
          if (typeof s.min === 'string' && matchesQuery(s.min, query)) return true;
          if (typeof s.max === 'string' && matchesQuery(s.max, query)) return true;
        }
        if (bind.v && typeof bind.v === 'object') {
          const v = bind.v as Record<string, unknown>;
          if (typeof v.min === 'string' && matchesQuery(v.min, query)) return true;
          if (typeof v.max === 'string' && matchesQuery(v.max, query)) return true;
        }
      }
      return false;
    }
  }
}

export function filterItems(items: ReadonlyArray<PipelineUiItem>, query: string): PipelineUiItem[] {
  const next: PipelineUiItem[] = [];
  for (const item of items) {
    if (item.type === 'group' || item.type === 'accordion' || item.type === 'stack') {
      const childItems = filterItems(item.items, query);
      if (itemMatchesQuery(item, query) || childItems.length > 0) {
        next.push({ ...item, items: childItems } as PipelineUiItem);
      }
      continue;
    }
    if (item.type === 'tabs') {
      const nextTabs = item.tabs
        .map((tab) => ({ ...tab, content: filterItems(tab.content, query) }))
        .filter((tab) => tab.content.length > 0 || matchesQuery(tab.title, query));
      if (itemMatchesQuery(item, query) || nextTabs.length > 0) {
        next.push({ ...item, tabs: nextTabs } as PipelineUiItem);
      }
      continue;
    }
    if (itemMatchesQuery(item, query)) {
      next.push(item);
    }
  }
  return next;
}

export type BindingWarning = {
  id: string;
  label: string;
  reason: string;
};

export type BindingCandidate = PipelineUiNodeDescriptor & { key: string };

export function buildBindingCandidates(
  nodeDescriptors: PipelineUiNodeDescriptor[],
  selectedControl: PipelineUiControl | null,
  bindingSearch: string
): BindingCandidate[] {
  const query = bindingSearch.trim().toLowerCase();
  const control = selectedControl ?? null;
  return nodeDescriptors
    .map((entry) => ({
      ...entry,
      key: `${entry.nodeId}.${entry.portKey}`
    }))
    .filter((entry) => {
      if (!control) return false;
      return isControlCompatible(control, entry);
    })
    .filter((entry) => {
      if (!query) return true;
      return (
        entry.key.toLowerCase().includes(query) ||
        entry.nodeId.toLowerCase().includes(query) ||
        entry.portKey.toLowerCase().includes(query) ||
        entry.nodeLabel.toLowerCase().includes(query)
      );
    });
}

export function buildBindingWarnings(
  editMode: boolean,
  ui: PipelineUi,
  fallbackItems: PipelineUiItem[],
  nodeDescriptors: PipelineUiNodeDescriptor[]
): BindingWarning[] {
  if (!editMode) return [];
  const rootItems =
    ui.layout?.type === 'tabs'
      ? ui.layout.tabs.flatMap((tab) => tab.content)
      : ui.layout?.type === 'stack'
        ? ui.layout.items
        : fallbackItems;
  const warnings: BindingWarning[] = [];
  const controls = collectControls(rootItems);
  for (const control of controls) {
    if (!controlRequiresBinding(control)) continue;
    const label = control.label?.trim() || control.id;
    if (control.type === 'dual_slider') {
      const bind = typeof control.bind === 'object' ? (control.bind as { min: string; max: string }) : { min: '', max: '' };
      const minBind = bind.min?.trim?.() ?? '';
      const maxBind = bind.max?.trim?.() ?? '';
      if (!minBind || !maxBind) {
        const missing = [!minBind ? 'min' : null, !maxBind ? 'max' : null].filter(Boolean).join(' & ');
        warnings.push({ id: control.id, label, reason: `Missing ${missing} binding` });
        continue;
      }
      const minDescriptor = resolveDescriptorForBind(minBind, nodeDescriptors);
      const maxDescriptor = resolveDescriptorForBind(maxBind, nodeDescriptors);
      if (!minDescriptor || !maxDescriptor) {
        warnings.push({ id: control.id, label, reason: 'Binding not found' });
        continue;
      }
      if (!isControlCompatible(control, minDescriptor) || !isControlCompatible(control, maxDescriptor)) {
        warnings.push({ id: control.id, label, reason: 'Incompatible port type' });
      }
      continue;
    }
    const bindValue = typeof control.bind === 'string' ? control.bind.trim() : '';
    if (!bindValue) {
      warnings.push({ id: control.id, label, reason: 'Missing binding' });
      continue;
    }
    const descriptor = resolveDescriptorForBind(bindValue, nodeDescriptors);
    if (!descriptor) {
      warnings.push({ id: control.id, label, reason: 'Binding not found' });
      continue;
    }
    if (!isControlCompatible(control, descriptor)) {
      warnings.push({ id: control.id, label, reason: 'Incompatible port type' });
    }
  }
  return warnings;
}
