<script lang="ts">
  import type { ValidationIssue } from '$lib/api/client';
  import ValidationIssueList from '$lib/components/ui/ValidationIssueList.svelte';
  import type { RegisterExperience } from '$lib/components/register-camera/registerCameraModalHelpers';

  type Props = {
    registerExperience: RegisterExperience;
    submitError: string | null;
    submitValidationIssues: ValidationIssue[];
    onRegisterExperienceChange: (next: RegisterExperience) => void;
  };

  let { registerExperience, submitError, submitValidationIssues, onRegisterExperienceChange }: Props = $props();
</script>

<div>
  <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Register stream</p>
  <p class="text-xl font-semibold text-surface-50">
    {registerExperience === 'simple' ? 'Quick camera setup' : 'Pick a camera, backend, and mode'}
  </p>
  <p class="text-sm text-surface-400">
    {registerExperience === 'simple'
      ? 'Choose camera, stream type, resolution, and optional pipeline/template.'
      : 'Detected devices from /peripherals with a fallback to /streams/backends.'}
  </p>
  {#if submitError}
    <div class="mt-4 space-y-3 rounded-lg border border-error-500/30 bg-error-500/10 px-4 py-3">
      <div>
        <p class="text-[0.7rem] font-semibold uppercase tracking-[0.24em] text-error-100">Last start attempt failed</p>
        <p class="mt-2 text-sm text-surface-100">{submitError}</p>
      </div>
      {#if submitValidationIssues.length}
        <ValidationIssueList issues={submitValidationIssues} title="Stream incompatibilities" compact />
      {/if}
    </div>
  {/if}
  <div class="mt-3 inline-flex rounded-md border border-surface-700 bg-surface-950/70 p-1">
    <button
      type="button"
      class={`rounded px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.2em] transition ${
        registerExperience === 'simple'
          ? 'bg-primary-500/20 text-primary-100'
          : 'text-surface-400 hover:text-surface-100'
      }`}
      onclick={() => onRegisterExperienceChange('simple')}
    >
      Simple
    </button>
    <button
      type="button"
      class={`rounded px-3 py-1.5 text-xs font-semibold uppercase tracking-[0.2em] transition ${
        registerExperience === 'advanced'
          ? 'bg-primary-500/20 text-primary-100'
          : 'text-surface-400 hover:text-surface-100'
      }`}
      onclick={() => onRegisterExperienceChange('advanced')}
    >
      Advanced
    </button>
  </div>
</div>
