<script lang="ts">
  // The IMU, live: the board in 3D, its status ring, its sensors over the last seconds, and the
  // numbers (imu/ImuPanel.svelte). The same view the Hardware screen shows under IMU.
  import ImuPanel from "#lib/imu/ImuPanel.svelte";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
</script>

<PaneBar>
  <span class="title">IMU</span>
</PaneBar>

<div class="scroll">
  <ImuPanel fill />
</div>

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-top: 6px;
  }
  .title {
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-2);
  }
</style>
