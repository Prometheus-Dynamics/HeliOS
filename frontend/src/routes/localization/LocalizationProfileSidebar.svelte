<script lang="ts">
  import SidebarSearchSection from '$lib/components/filters/SidebarSearchSection.svelte';
  import {
    PROFILE_COLORS,
    profileColorForId
  } from '$lib/features/localization/utils';
  import type {
    LocalizationPoseSpace,
    LocalizationProfile
  } from '$lib/features/localization/localizationConfig';

  type Props = {
    profileTransferBusy: boolean;
    canExport: boolean;
    profileImportInputEl?: HTMLInputElement;
    profileSearch: string;
    filteredProfiles: LocalizationProfile[];
    activeProfileId: string;
    coordinateSpace: LocalizationPoseSpace;
    profileSupportedSpacesById: Record<string, LocalizationPoseSpace[]>;
    profiles: LocalizationProfile[];
    profileIndexById: Map<string, number>;
    onCreateProfile: () => void;
    onExportProfiles: () => void | Promise<void>;
    onOpenImportProfilesDialog: () => void;
    onHandleProfileImportInput: (event: Event) => void;
    onSetActiveProfile: (profileId: string) => void | Promise<void>;
    onSetProfileEnabled: (profileId: string, enabled: boolean) => void;
    onSetProfileViewEnabled: (profileId: string, enabled: boolean) => void;
    onHandleProfileColorInput: (profileId: string, event: Event) => void;
    onOpenDeleteProfileModal: (profile: LocalizationProfile) => void;
    showOutputsOverlay: boolean;
    showMetricsOverlay: boolean;
  };

  let {
    profileTransferBusy,
    canExport,
    profileImportInputEl = $bindable(undefined),
    profileSearch = $bindable(''),
    filteredProfiles,
    activeProfileId,
    coordinateSpace,
    profileSupportedSpacesById,
    profiles,
    profileIndexById,
    onCreateProfile,
    onExportProfiles,
    onOpenImportProfilesDialog,
    onHandleProfileImportInput,
    onSetActiveProfile,
    onSetProfileEnabled,
    onSetProfileViewEnabled,
    onHandleProfileColorInput,
    onOpenDeleteProfileModal,
    showOutputsOverlay = $bindable(false),
    showMetricsOverlay = $bindable(false)
  }: Props = $props();
</script>

<aside class="w-full shrink-0 space-y-3 overflow-visible rounded border border-surface-800/60 bg-surface-950/40 p-3 text-xs text-surface-400 lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]">
  <div class="space-y-2">
    <button
      class="btn btn-xs preset-filled-primary-500 w-full uppercase tracking-[0.22em]"
      type="button"
      onclick={onCreateProfile}
    >
      New Profile
    </button>
    <div class="grid grid-cols-2 gap-1.5">
      <button
        class="btn btn-2xs preset-tonal uppercase tracking-[0.22em]"
        type="button"
        onclick={() => void onExportProfiles()}
        disabled={profileTransferBusy || !canExport}
      >
        Export
      </button>
      <button
        class="btn btn-2xs preset-tonal uppercase tracking-[0.22em]"
        type="button"
        onclick={onOpenImportProfilesDialog}
        disabled={profileTransferBusy}
      >
        Import
      </button>
    </div>
    <input
      type="file"
      accept=".json,application/json"
      class="sr-only"
      bind:this={profileImportInputEl}
      onchange={onHandleProfileImportInput}
    />
  </div>

  <SidebarSearchSection
    label="Search profiles"
    placeholder="Name or id"
    description="Name, id"
    bind:value={profileSearch}
    ariaLabel="Search profiles"
    size="compact"
  />

  <div>
    <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Profiles</p>
    <div class="mt-2 space-y-1.5">
      {#if profiles.length === 0}
        <p class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 p-2.5 text-micro-tight text-surface-500">
          No profiles yet.
        </p>
      {:else if filteredProfiles.length === 0}
        <p class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 p-2.5 text-micro-tight text-surface-500">
          No profiles match this search/filter.
        </p>
      {:else}
        {#each filteredProfiles as profile (profile.id)}
          {@const isSelected = activeProfileId === profile.id}
          {@const runtimeEnabled = profile.enabled !== false}
          {@const visible = profile.viewEnabled === true}
          {@const color = profileColorForId(profile.id, profiles, profileIndexById, PROFILE_COLORS)}
          {@const supported = (profileSupportedSpacesById?.[profile.id] ?? []).includes(coordinateSpace)}
          <div
            class={`w-full rounded border px-2.5 py-1.5 text-left transition ${
              isSelected
                ? 'border-primary-400/70 bg-primary-500/10 text-white shadow-lg shadow-primary-500/20'
                : 'border-surface-700/40 text-surface-300 hover:border-surface-600/80'
            } ${supported ? '' : 'opacity-60'} ${runtimeEnabled ? '' : 'opacity-70'}`}
            role="button"
            tabindex="0"
            aria-pressed={isSelected ? 'true' : 'false'}
            onclick={() => void onSetActiveProfile(profile.id)}
            onkeydown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                void onSetActiveProfile(profile.id);
              }
            }}
          >
            <div class="flex items-start gap-3">
              <div
                class={`relative mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full border transition ${
                  isSelected ? 'border-white/40 hover:border-primary-200' : 'border-white/20 hover:border-primary-200/70'
                }`}
                title={`Set color for ${profile.name}`}
              >
                <span class="h-3 w-3 rounded-full border border-white/40" style={`background-color: ${color};`}></span>
                <input
                  type="color"
                  value={color}
                  class="absolute inset-0 cursor-pointer opacity-0"
                  aria-label={`Set color for ${profile.name}`}
                  onchange={(event) => onHandleProfileColorInput(profile.id, event)}
                  onclick={(event) => event.stopPropagation()}
                />
              </div>
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-2">
                  <p class={`truncate text-xs font-semibold ${isSelected ? 'text-white' : 'text-surface-100'}`}>
                    {profile.name}
                  </p>
                  <button
                    type="button"
                    class={`shrink-0 rounded border px-1.5 py-[1px] text-micro-tight uppercase tracking-[0.16em] transition ${
                      runtimeEnabled
                        ? 'border-sky-500/60 bg-sky-500/10 text-sky-100 hover:border-sky-400/80 hover:bg-sky-500/20'
                        : 'border-rose-500/60 bg-rose-500/10 text-rose-100 hover:border-rose-400/80 hover:bg-rose-500/20'
                    }`}
                    aria-label={`Toggle ${profile.name} localization runtime`}
                    title={runtimeEnabled ? `Disable ${profile.name} for localization runtime` : `Enable ${profile.name} for localization runtime`}
                    onclick={(event) => {
                      event.stopPropagation();
                      onSetProfileEnabled(profile.id, !runtimeEnabled);
                    }}
                  >
                    {runtimeEnabled ? 'Enabled' : 'Disabled'}
                  </button>
                  <button
                    type="button"
                    class={`shrink-0 rounded border px-1.5 py-[1px] text-micro-tight uppercase tracking-[0.16em] transition ${
                      visible
                        ? 'border-emerald-500/60 bg-emerald-500/10 text-emerald-100 hover:border-emerald-400/80 hover:bg-emerald-500/20'
                        : 'border-surface-600/60 bg-surface-800/40 text-surface-300 hover:border-surface-500/80 hover:bg-surface-700/50'
                    }`}
                    aria-label={`Toggle ${profile.name} visibility in 3D view`}
                    title={visible ? `Hide ${profile.name} in 3D view` : `Show ${profile.name} in 3D view`}
                    onclick={(event) => {
                      event.stopPropagation();
                      onSetProfileViewEnabled(profile.id, !visible);
                    }}
                  >
                    {visible ? 'Visible' : 'Hidden'}
                  </button>
                  {#if !supported}
                    <span class="shrink-0 rounded border border-amber-500/60 bg-amber-500/10 px-1.5 py-[1px] text-micro-tight uppercase tracking-[0.16em] text-amber-100">
                      Unsupported
                    </span>
                  {/if}
                </div>
              </div>
              <div class="flex items-center gap-1">
                <button
                  class={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full border text-rose-300 transition hover:text-rose-100 disabled:cursor-not-allowed disabled:opacity-40 ${
                    isSelected
                      ? 'border-rose-300/40 hover:border-rose-200'
                      : 'border-rose-500/40 hover:border-rose-400'
                  }`}
                  type="button"
                  aria-label={`Delete profile ${profile.name}`}
                  title={profiles.length <= 1 ? 'At least one profile is required' : `Delete profile ${profile.name}`}
                  disabled={profiles.length <= 1}
                  onclick={(event) => {
                    event.stopPropagation();
                    onOpenDeleteProfileModal(profile);
                  }}
                >
                  🗑
                </button>
                <button
                  class={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full border text-surface-400 transition hover:text-primary-100 ${
                    isSelected
                      ? 'border-white/30 hover:border-primary-300'
                      : 'border-surface-600/60 hover:border-primary-400'
                  }`}
                  type="button"
                  aria-label={`Open settings for ${profile.name}`}
                  title={`Open settings for ${profile.name}`}
                  onclick={(event) => {
                    event.stopPropagation();
                    void onSetActiveProfile(profile.id);
                    showOutputsOverlay = true;
                    showMetricsOverlay = false;
                  }}
                >
                  ⚙
                </button>
              </div>
            </div>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</aside>
