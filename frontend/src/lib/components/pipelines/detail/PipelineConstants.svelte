<script lang="ts">
  type Props = {
    error: string | null | undefined;
  };

  const { error }: Props = $props();
</script>

{#if error}
  <div class="rounded border border-error-600/60 bg-error-950/40 px-4 py-3 text-sm text-error-50">
    <div class="text-micro-tight font-semibold uppercase tracking-[0.3em] text-error-200">Graph error</div>
    <p class="mt-1 leading-relaxed">{error}</p>
  </div>
{/if}
