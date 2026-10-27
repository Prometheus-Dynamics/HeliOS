<script lang="ts">
  // The robot link: the team number and NetworkTables (where robot code reads the results), and,
  // on a device that has security, who may change it.
  import NtSection from "#lib/components/robot/NtSection.svelte";
  import SecuritySection from "#lib/components/security/SecuritySection.svelte";
  import Section from "#lib/kit/Section.svelte";
  import { device } from "#lib/stores/device.svelte.js";
</script>

<div class="scroll">
  <Section title="Robot code" key="robot-nt">
    <NtSection />
  </Section>
  {#if device.has("security")}
    <SecuritySection />
  {/if}
</div>

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
</style>
