<script lang="ts">
  import type { ValidationIssue } from '$lib/ts-bindings/http/client';

  type Props = {
    issues?: ValidationIssue[] | null;
    title?: string;
    compact?: boolean;
  };

  const {
    issues = [],
    title = 'Validation report',
    compact = false
  }: Props = $props();

  const normalizedIssues = $derived(
    Array.isArray(issues)
      ? issues.filter(
          (issue): issue is ValidationIssue =>
            Boolean(issue) &&
            typeof issue.code === 'string' &&
            typeof issue.message === 'string' &&
            typeof issue.path === 'string'
        )
      : []
  );
</script>

{#if normalizedIssues.length}
  <section class={`rounded-lg border border-warning-500/30 bg-warning-500/8 ${compact ? 'px-3 py-2' : 'px-4 py-3'}`}>
    <p class={`font-semibold uppercase tracking-[0.22em] text-warning-100 ${compact ? 'text-[0.65rem]' : 'text-[0.7rem]'}`}>{title}</p>
    <div class={`mt-2 space-y-2 ${compact ? 'text-xs' : 'text-sm'}`}>
      {#each normalizedIssues as issue, index (`${issue.code}-${issue.path}-${index}`)}
        <div class="rounded-md border border-warning-500/20 bg-surface-950/40 px-3 py-2">
          <div class="flex flex-wrap items-center gap-2">
            <span class="rounded-sm border border-warning-400/30 bg-warning-500/10 px-2 py-0.5 font-semibold uppercase tracking-[0.18em] text-warning-100">
              {issue.code}
            </span>
            {#if issue.path.trim().length}
              <code class="text-xs text-surface-300">{issue.path}</code>
            {/if}
          </div>
          <p class="mt-2 text-surface-100">{issue.message}</p>
          {#if issue.remediation}
            <p class="mt-2 text-xs text-warning-50/90">Next: {issue.remediation}</p>
          {/if}
        </div>
      {/each}
    </div>
  </section>
{/if}
