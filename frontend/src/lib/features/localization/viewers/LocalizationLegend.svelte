<script lang="ts">
  import type { LocalizationFieldDefinition } from '$lib/features/localization/viewers/localizationViewerTypes';

  type Props = {
    activeField: LocalizationFieldDefinition | null;
    minimapExpanded: boolean;
  };

  const { activeField, minimapExpanded }: Props = $props();
</script>

<div class="pointer-events-none absolute left-4 top-4 space-y-1 text-micro-tight uppercase tracking-[0.4em] text-surface-500">
  <p>Top-down map</p>
  <p class="text-micro-tight tracking-[0.5em] text-surface-400">
    {activeField ? `${activeField.name} view` : 'Isolated view'}
  </p>
  <p class="text-micro-tight tracking-[0.4em] text-surface-400">
    {minimapExpanded ? 'Click to collapse' : 'Click to expand'}
  </p>
</div>
