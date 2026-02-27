<script lang="ts">
  type Props = {
    errorMessage: string | null;
  };

  const { errorMessage }: Props = $props();
</script>

{#if errorMessage}
  <div class="rounded border border-error-500/40 bg-error-500/10 px-4 py-2 text-sm text-error-200">
    {errorMessage}
  </div>
{/if}
