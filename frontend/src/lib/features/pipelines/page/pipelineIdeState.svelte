<script lang="ts" module>
  import { browser } from '$app/environment';
  import { env as publicEnv } from '$env/dynamic/public';
  import { apiFetch, apiFetchResponse } from '$lib/api/core/http';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import { SvelteSet, SvelteURL } from 'svelte/reactivity';

  type PluginListEntry = { name: string; detail: string; description: string };

  export function createPipelineIdeState() {
    const NODE_LANGUAGE_ALIASES = new SvelteSet(['node', 'nodejs', 'typescript', 'ts', 'javascript', 'js']);
    const IDE_ENABLED_FALLBACK = (publicEnv.PUBLIC_IDE_ENABLED ?? 'true').toLowerCase() !== 'false';
    const IDE_WORKSPACE_DIR_FALLBACK =
      publicEnv.PUBLIC_IDE_WORKSPACE_DIR?.trim() || '/var/lib/helios/sdk/default';
    const IDE_PORT_FALLBACK = Number.parseInt(publicEnv.PUBLIC_IDE_PORT ?? '5805', 10) || 5805;
    const IDE_PROJECTS_DIR_FALLBACK =
      publicEnv.PUBLIC_IDE_PROJECTS_DIR?.trim() || `${IDE_WORKSPACE_DIR_FALLBACK.replace(/\/+$/, '')}/projects`;

    const state = $state({
      IDE_ENABLED_FALLBACK,
      IDE_WORKSPACE_DIR_FALLBACK,
      IDE_PORT_FALLBACK,
      IDE_PROJECTS_DIR_FALLBACK,
      ideEnabled: IDE_ENABLED_FALLBACK,
      ideWorkspaceDir: IDE_WORKSPACE_DIR_FALLBACK,
      ideProjectsDir: IDE_PROJECTS_DIR_FALLBACK,
      idePort: IDE_PORT_FALLBACK,
      ideUrl: `http://127.0.0.1:${IDE_PORT_FALLBACK}/`,
      ideIframeUrl: `http://127.0.0.1:${IDE_PORT_FALLBACK}/`,
      ideProjects: [] as string[],
      pluginProjectModalOpen: false,
      pluginProjectName: '',
      pluginProjectLanguage: 'rust',
      pluginProjectBusy: false,
      pluginProjectError: null as string | null,
      customNodeSearch: ''
    });

    const visiblePlugins = $derived.by<PluginListEntry[]>(() => {
      const uniqueNames = new SvelteSet<string>();
      for (const name of state.ideProjects) {
        const trimmed = name.trim();
        if (trimmed) {
          uniqueNames.add(trimmed);
        }
      }
      const entries = Array.from(uniqueNames)
        .sort((a, b) => a.localeCompare(b))
        .map((name) => ({
          name,
          detail: 'Project',
          description: 'SDK plugin project'
        }));
      const needle = state.customNodeSearch.trim().toLowerCase();
      if (!needle) return entries;
      return entries.filter((entry) => entry.name.toLowerCase().includes(needle));
    });

    async function refreshIdeInfo(): Promise<void> {
      if (!browser) return;
      try {
        const prevIdeUrl = state.ideUrl;
        const payload = await apiFetch<Record<string, unknown>>('/device/ide', {
          headers: { Accept: 'application/json' }
        });
        if (typeof payload?.enabled === 'boolean') {
          state.ideEnabled = payload.enabled;
        }
        if (typeof payload?.workspace_dir === 'string' && payload.workspace_dir.trim().length) {
          state.ideWorkspaceDir = payload.workspace_dir.trim();
        }
        if (typeof payload?.projects_dir === 'string' && payload.projects_dir.trim().length) {
          state.ideProjectsDir = payload.projects_dir.trim();
        }
        if (typeof payload?.port === 'number' && Number.isFinite(payload.port)) {
          state.idePort = payload.port;
        }
        if (typeof payload?.url === 'string' && payload.url.trim().length) {
          state.ideUrl = payload.url.trim();
          if (!state.ideIframeUrl || state.ideIframeUrl === prevIdeUrl) {
            state.ideIframeUrl = state.ideUrl;
          }
        }
      } catch {
        // ignore; fall back to env/default-derived values
      }
    }

    async function refreshIdeProjects(): Promise<void> {
      if (!browser) return;
      try {
        const payload = await apiFetch<Record<string, unknown>>('/device/ide/projects', {
          headers: { Accept: 'application/json' }
        });
        if (Array.isArray(payload?.projects)) {
          state.ideProjects = payload.projects
            .map((entry: { name?: string } | null) => (typeof entry?.name === 'string' ? entry.name.trim() : ''))
            .filter((name) => name.length > 0);
        }
      } catch {
        // ignore; fall back to existing list
      }
    }

    function openPluginProjectModal() {
      state.pluginProjectName = '';
      state.pluginProjectLanguage = 'rust';
      state.pluginProjectError = null;
      state.pluginProjectModalOpen = true;
    }

    function closePluginProjectModal() {
      if (state.pluginProjectBusy) return;
      state.pluginProjectModalOpen = false;
      state.pluginProjectError = null;
    }

    async function createPluginProject(): Promise<void> {
      if (!state.pluginProjectName.trim()) {
        state.pluginProjectError = 'Provide a plugin project name.';
        return;
      }
      const normalizedLanguage = state.pluginProjectLanguage.trim().toLowerCase();
      if (NODE_LANGUAGE_ALIASES.has(normalizedLanguage)) {
        state.pluginProjectError = 'Node/TypeScript SDK is temporarily disabled for this release.';
        return;
      }
      state.pluginProjectBusy = true;
      state.pluginProjectError = null;
      try {
        const response = await apiFetchResponse('/device/ide/projects', {
          method: 'POST',
          headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
          body: JSON.stringify({
            name: state.pluginProjectName.trim(),
            language: state.pluginProjectLanguage
          })
        });
        if (!response.ok) {
          const message = await response.text();
          throw new Error(message || `Create failed (${response.status})`);
        }
        const payload = await response.json();
        if (typeof payload?.name === 'string' && payload.name.trim()) {
          openPluginInIde(payload.name.trim());
        }
        await refreshIdeProjects();
        state.pluginProjectModalOpen = false;
      } catch (err) {
        state.pluginProjectError = buildErrorMessage({ error: err, fallback: 'Unable to create plugin project.' });
      } finally {
        state.pluginProjectBusy = false;
      }
    }

    function openIde() {
      if (!state.ideEnabled || !browser) return;
      window.open(state.ideUrl, '_blank', 'noopener,noreferrer');
    }

    function openPluginInIde(pluginName: string) {
      if (!state.ideEnabled) return;
      const folderPath = `${state.ideProjectsDir.replace(/\/+$/, '')}/${pluginName}`;
      const url = new SvelteURL(state.ideUrl);
      url.searchParams.set('folder', folderPath);
      state.ideIframeUrl = url.toString();
    }

    return {
      state,
      get visiblePlugins() {
        return visiblePlugins;
      },
      refreshIdeInfo,
      refreshIdeProjects,
      openPluginProjectModal,
      closePluginProjectModal,
      createPluginProject,
      openIde,
      openPluginInIde
    };
  }
</script>
