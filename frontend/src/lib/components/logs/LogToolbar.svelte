<script lang="ts">
  import type { LogFilter, LogStreamSummary } from '$lib/features/logs/types';
  import LogStreamList from '$lib/components/logs/LogStreamList.svelte';
  import LogSettings from '$lib/components/logs/LogSettings.svelte';

  type Props = {
    logStreams: LogStreamSummary[];
    selectedLogStream: string | null;
    logFilter: LogFilter;
    logDownloadWindow: string;
    logDownloadStatus: string | null;
    logDownloadError: string | null;
    logDownloadBusy: boolean;
    onStreamChange: (value: string) => void;
    onFilterChange: (value: LogFilter) => void;
    onWindowChange: (value: string) => void;
    onDownload: () => void;
  };

  let {
    logStreams,
    selectedLogStream = $bindable<string | null>(null),
    logFilter = $bindable<LogFilter>('all'),
    logDownloadWindow = $bindable('15m'),
    logDownloadStatus,
    logDownloadError,
    logDownloadBusy,
    onStreamChange,
    onFilterChange,
    onWindowChange,
    onDownload
  }: Props = $props();
</script>

<div class="space-y-2">
  <div class="flex flex-wrap items-center gap-3">
    <LogStreamList
      {logStreams}
      bind:selectedLogStream={selectedLogStream}
      bind:logFilter={logFilter}
      onStreamChange={onStreamChange}
      onFilterChange={onFilterChange}
    />
    <LogSettings
      bind:logDownloadWindow={logDownloadWindow}
      {logDownloadStatus}
      {logDownloadError}
      {logDownloadBusy}
      {selectedLogStream}
      onWindowChange={onWindowChange}
      onDownload={onDownload}
    />
  </div>
</div>
