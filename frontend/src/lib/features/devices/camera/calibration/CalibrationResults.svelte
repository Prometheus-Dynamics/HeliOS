<script lang="ts">
  import { estimateCalibrationFovDegs } from '../cameraCalibrationUtils';

  type CalibrationResult = {
    calibration: {
      fx: number;
      fy: number;
      cx: number;
      cy: number;
      k1: number;
      k2: number;
      p1: number;
      p2: number;
      k3: number;
      undistortIters: number;
      lensModel?: 'pinhole' | 'fisheye';
    };
    reprojectionErrorPx: number;
    viewsUsed: number;
    pointsUsed: number;
    warnings?: string[];
    debugViews?: Array<{
      image: string;
      overlay?: string | null;
      tagsDetected: number;
      pointsDetected: number;
      used: boolean;
      coverageRatio: number;
    }>;
  };

  type CalibrationResultsProps = {
    calibrationResult: CalibrationResult | null;
    sourceResolution: { width: number; height: number } | null;
    formatMaybeNumber: (value: number | null, digits?: number) => string;
    overlayUrlForImage: (name: string) => string | null;
  };

  const {
    calibrationResult,
    sourceResolution,
    formatMaybeNumber,
    overlayUrlForImage
  }: CalibrationResultsProps = $props();

  export type $$Props = CalibrationResultsProps;
</script>

{#if calibrationResult}
  {@const fovs = estimateCalibrationFovDegs(sourceResolution, calibrationResult.calibration)}
  <div class="grid gap-2 [grid-template-columns:repeat(auto-fit,minmax(18rem,1fr))]">
    <div class="rounded border border-surface-800/60 bg-surface-950/10 p-3">
      <div class="flex items-start justify-between gap-3">
        <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Result</p>
      </div>
      <p class="mt-2 text-xs text-surface-200">
        Reprojection error ~{Number(calibrationResult.reprojectionErrorPx ?? 0).toFixed(2)}px · Views {calibrationResult.viewsUsed ?? 0} · Points {calibrationResult.pointsUsed ?? 0}
      </p>
      <div class="mt-2 grid gap-1 text-xs text-surface-200">
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">Resolution</span>
          <span class="font-mono">
            {sourceResolution ? `${sourceResolution.width}×${sourceResolution.height}` : '—'}
          </span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">HFOV</span>
          <span class="font-mono">
            {formatMaybeNumber(fovs.hfov, 2)}°
          </span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">VFOV</span>
          <span class="font-mono">
            {formatMaybeNumber(fovs.vfov, 2)}°
          </span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">DFOV</span>
          <span class="font-mono">
            {formatMaybeNumber(fovs.dfov, 2)}°
          </span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">Model</span>
          <span class="font-mono">{calibrationResult.calibration.lensModel ?? 'pinhole'}</span>
        </div>
      </div>
      <p class="mt-2 text-2xs text-surface-500">
        For pinhole, FOV is derived from resolution and fx/fy. For fisheye, we invert the fisheye model (k4 assumed 0) to estimate FOV.
        Undistort previews can appear cropped depending on the rectification.
      </p>
    </div>

    <div class="rounded border border-surface-800/60 bg-surface-950/10 p-3">
      <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Intrinsics</p>
      <div class="mt-2 grid gap-1 text-xs text-surface-200">
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">fx</span>
          <span class="font-mono">{Number(calibrationResult.calibration.fx ?? 0).toFixed(2)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">fy</span>
          <span class="font-mono">{Number(calibrationResult.calibration.fy ?? 0).toFixed(2)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">cx</span>
          <span class="font-mono">{Number(calibrationResult.calibration.cx ?? 0).toFixed(2)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">cy</span>
          <span class="font-mono">{Number(calibrationResult.calibration.cy ?? 0).toFixed(2)}</span>
        </div>
      </div>
    </div>

    <div class="rounded border border-surface-800/60 bg-surface-950/10 p-3">
      <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Distortion</p>
      <div class="mt-2 grid gap-1 text-xs text-surface-200">
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">k1</span>
          <span class="font-mono">{Number(calibrationResult.calibration.k1 ?? 0).toExponential(3)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">k2</span>
          <span class="font-mono">{Number(calibrationResult.calibration.k2 ?? 0).toExponential(3)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">k3</span>
          <span class="font-mono">{Number(calibrationResult.calibration.k3 ?? 0).toExponential(3)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">p1</span>
          <span class="font-mono">{Number(calibrationResult.calibration.p1 ?? 0).toExponential(3)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">p2</span>
          <span class="font-mono">{Number(calibrationResult.calibration.p2 ?? 0).toExponential(3)}</span>
        </div>
        <div class="flex justify-between gap-3">
          <span class="text-surface-500">undistort iters</span>
          <span class="font-mono">{Number(calibrationResult.calibration.undistortIters ?? 5)}</span>
        </div>
      </div>
    </div>
  </div>

  {#if Array.isArray(calibrationResult.debugViews) && calibrationResult.debugViews.length}
    <div class="mt-3 rounded border border-surface-800/60 bg-surface-950/10 p-3">
      <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Per-Image Detections</p>
      <div class="mt-2 overflow-x-auto">
        <table class="min-w-full text-left text-xs text-surface-200">
          <thead class="text-2xs uppercase tracking-[0.3em] text-surface-500">
            <tr>
              <th class="px-2 py-1">image</th>
              <th class="px-2 py-1">tags</th>
              <th class="px-2 py-1">points</th>
              <th class="px-2 py-1">coverage</th>
              <th class="px-2 py-1">used</th>
              <th class="px-2 py-1">overlay</th>
            </tr>
          </thead>
          <tbody>
            {#each calibrationResult.debugViews as view (view.image)}
              {@const overlayUrl = overlayUrlForImage(view.image)}
              <tr class="border-t border-surface-800/60">
                <td class="max-w-[22rem] truncate px-2 py-1 font-mono text-micro text-surface-300">{view.image}</td>
                <td class="px-2 py-1 font-mono">{view.tagsDetected ?? 0}</td>
                <td class="px-2 py-1 font-mono">{view.pointsDetected ?? 0}</td>
                <td class="px-2 py-1 font-mono">{Number((view.coverageRatio ?? 0) * 100).toFixed(2)}%</td>
                <td class="px-2 py-1">{view.used ? 'yes' : 'no'}</td>
                <td class="px-2 py-1">
                  {#if overlayUrl}
                    <a class="underline underline-offset-2" href={overlayUrl} target="_blank" rel="noreferrer">
                      open
                    </a>
                  {:else}
                    —
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="mt-2 text-2xs text-surface-500">Overlay colors are per-tag ID; coverage is quad-area / image-area.</p>
    </div>
  {/if}
  {#if Array.isArray(calibrationResult.warnings) && calibrationResult.warnings.length}
    <ul class="mt-3 list-disc space-y-1 pl-5 text-sm text-surface-400">
      {#each calibrationResult.warnings as warn, idx (`warn-${idx}`)}
        <li>{warn}</li>
      {/each}
    </ul>
  {/if}
{/if}
