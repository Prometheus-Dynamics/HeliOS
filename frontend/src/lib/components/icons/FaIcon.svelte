<script lang="ts">
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';

  type Props = {
    icon: IconDefinition;
    title?: string | null;
    class?: string;
    style?: string;
    ariaHidden?: 'true' | 'false' | boolean | null;
  };

  let {
    icon,
    title = null,
    class: className = '',
    style = '',
    ariaHidden = null
  }: Props = $props();

  const svgBox = $derived.by(() => {
    const [width, height] = icon.icon;
    return `0 0 ${width} ${height}`;
  });

  const svgPaths = $derived.by(() => {
    const [, , , , pathData] = icon.icon;
    return Array.isArray(pathData) ? pathData : [pathData];
  });

  const resolvedAriaHidden = $derived.by(() => {
    if (ariaHidden === null || ariaHidden === undefined) {
      return title ? undefined : 'true';
    }
    return typeof ariaHidden === 'boolean' ? (ariaHidden ? 'true' : 'false') : ariaHidden;
  });

  export type $$Props = Props;
</script>

<svg
  class={className}
  viewBox={svgBox}
  role={title ? 'img' : undefined}
  aria-hidden={resolvedAriaHidden}
  xmlns="http://www.w3.org/2000/svg"
  fill="currentColor"
  style={style}
>
  {#if title}
    <title>{title}</title>
  {/if}
  {#each svgPaths as path (path)}
    <path d={path}></path>
  {/each}
</svg>
