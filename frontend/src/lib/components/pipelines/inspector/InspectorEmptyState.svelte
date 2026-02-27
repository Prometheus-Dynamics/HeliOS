<script lang="ts">
  type Props = {
    message: string;
  };

  const { message }: Props = $props();
</script>

<section class="rounded border border-surface-800/80 bg-surface-950/80 p-6 text-center text-surface-400">
  {message}
</section>
