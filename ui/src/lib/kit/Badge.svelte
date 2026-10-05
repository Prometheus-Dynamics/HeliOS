<script lang="ts">
  // A tiny status label.
  let { tone = "neutral", text, mono = false }: { tone?: "neutral" | "ok" | "warn" | "err" | "info" | "accent"; text: string; mono?: boolean } = $props();
</script>

<span class="badge t-{tone}" class:mono>{text}</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    height: 17px;
    padding: 0 6px;
    font-size: 10.5px;
    font-weight: 600;
    border-radius: var(--r-1);
    white-space: nowrap;
    flex-shrink: 0;
  }
  .mono {
    font-family: var(--font-code);
    font-weight: 500;
  }
  .t-neutral {
    color: var(--fg-2);
    background: var(--s3);
  }
  .t-ok {
    color: var(--ok);
    background: var(--ok-bg);
  }
  .t-warn {
    color: var(--warn);
    background: var(--warn-bg);
  }
  .t-err {
    color: var(--err);
    background: var(--err-bg);
  }
  .t-info {
    color: var(--info);
    background: var(--info-bg);
  }
  .t-accent {
    color: var(--accent-fg);
    background: var(--accent-tint);
  }
</style>
