<script lang="ts">
  // Board maker: chessboard, ChArUco or AprilGrid, sized for your paper, as
  // an exact-scale SVG to print at 100%.
  import Icon from "#lib/components/common/Icon.svelte";
  import { downloadText } from "#lib/files.js";
  import Choice from "#lib/kit/Choice.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Section from "#lib/kit/Section.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import { calibration as cal, type BoardKind } from "#lib/stores/calibration.svelte.js";
  import { toasts } from "#lib/stores/toasts.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";

  const PAPER = { letter: [215.9, 279.4], a4: [210, 297], a3: [297, 420], tabloid: [279.4, 431.8] } as const;
  const b = $derived(cal.board);
  const bw = $derived(b.cols * b.square);
  const bh = $derived(b.rows * b.square);
  const paper = $derived.by(() => {
    const [w, h] = PAPER[b.paper];
    return bw > bh ? { w: h, h: w } : { w, h };
  });
  const margin = 10;
  const fits = $derived(bw + margin * 2 <= paper.w && bh + margin * 2 + 8 <= paper.h);
  const ox = $derived((paper.w - bw) / 2);
  const oy = $derived((paper.h - bh) / 2);

  // Deterministic pseudo-marker bits for the preview (the device renders the
  // print file's real codes from the Eidos dictionary).
  function bits(i: number, n: number): boolean[] {
    let s = (i + 1) * 2654435761;
    return Array.from({ length: n * n }, () => ((s = (s * 1103515245 + 12345) >>> 0) >> 16) & 1).map(Boolean);
  }
  const N = $derived(b.dictionary.startsWith("aruco_4") ? 4 : b.dictionary.startsWith("aruco_5") ? 5 : 6);

  function set<K extends keyof typeof b>(k: K, v: (typeof b)[K]) {
    cal.board[k] = v;
    if (k === "square" && cal.board.marker >= (v as number)) cal.board.marker = Math.round((v as number) * 0.75);
    cal.saveBoard();
  }

  let svgEl = $state<SVGSVGElement>();
  function svgText() {
    if (!svgEl) return "";
    const clone = svgEl.cloneNode(true) as SVGSVGElement;
    clone.setAttribute("width", `${paper.w}mm`);
    clone.setAttribute("height", `${paper.h}mm`);
    clone.setAttribute("xmlns", "http://www.w3.org/2000/svg");
    return clone.outerHTML;
  }
  const fileName = $derived(`${b.kind}-${b.cols}x${b.rows}-${b.square}mm-${b.paper}${b.kind === "chessboard" ? "" : "-preview"}.svg`);
</script>

<PaneBar>
  <IconButton icon="download" label="Download SVG (exact scale)" text="SVG" size={24} onclick={() => { downloadText(fileName, svgText(), "image/svg+xml"); if (b.kind !== "chessboard") toasts.info("Preview codes: a connected robot renders the real dictionary codes for print"); }} />
  <IconButton icon="printer" label="Print at 100%" size={24} onclick={() => { const w = window.open(URL.createObjectURL(new Blob([svgText()], { type: "image/svg+xml" }))); w?.addEventListener("load", () => w.print()); }} />
</PaneBar>

<div class="maker">
  <div class="opts">
    <Section title="Board" key="board-kind">
      <Prop label="Type">
        <Seg label="Board type" value={b.kind} options={[{ value: "charuco", label: "ChArUco" }, { value: "chessboard", label: "Chess" }, { value: "aprilgrid", label: "AprilGrid" }] as { value: BoardKind; label: string }[]} onchange={(v) => set("kind", v)} />
      </Prop>
      <Prop label="Columns × rows">
        <Num value={b.cols} min={3} max={20} label="Columns" width={52} onchange={(v) => set("cols", v)} />
        <Num value={b.rows} min={3} max={20} label="Rows" width={52} onchange={(v) => set("rows", v)} />
      </Prop>
      <Prop label="Square"><Num value={b.square} min={5} max={120} step={0.5} unit="mm" width={78} label="Square size" onchange={(v) => set("square", v)} /></Prop>
      {#if b.kind !== "chessboard"}
        <Prop label="Marker"><Num value={b.marker} min={3} max={b.square - 1} step={0.5} unit="mm" width={78} label="Marker size" onchange={(v) => set("marker", v)} /></Prop>
        <Prop label="Dictionary">
          <Choice value={b.dictionary} options={b.kind === "aprilgrid" ? ["apriltag_36h11", "apriltag_25h9", "apriltag_16h5"] : ["aruco_4x4_50", "aruco_5x5_100", "aruco_6x6_250"]} onchange={(v) => set("dictionary", v)} />
        </Prop>
      {/if}
      <Prop label="Paper"><Seg label="Paper" value={b.paper} options={[{ value: "letter", label: "Letter" }, { value: "a4", label: "A4" }, { value: "a3", label: "A3" }, { value: "tabloid", label: "11×17" }]} onchange={(v) => set("paper", v)} /></Prop>
    </Section>
    <div class="check" class:bad={!fits}>
      <Icon name={fits ? "circle-check" : "alert-triangle"} size={14} />
      {#if fits}
        Board is {(bw / 10).toFixed(1)} × {(bh / 10).toFixed(1)} cm and fits the page.
      {:else}
        {(bw / 10).toFixed(1)} × {(bh / 10).toFixed(1)} cm does not fit {b.paper}. Use smaller squares or bigger paper.
      {/if}
    </div>
    <ul class="tips">
      <li>Print at <b>100% / actual size</b>, never "fit to page".</li>
      <li>Measure one square with a ruler and enter the real size if it differs.</li>
      <li>Mount it on something rigid and flat; a bent board ruins a calibration.</li>
      {#if b.kind !== "chessboard"}<li>The preview's codes are placeholders; the robot fills in real {b.dictionary} codes.</li>{/if}
    </ul>
  </div>
  <div class="preview">
    <svg bind:this={svgEl} viewBox="0 0 {paper.w} {paper.h}" class="page">
      <rect width={paper.w} height={paper.h} fill="#fff" />
      <g transform="translate({ox},{oy})">
        {#if b.kind === "aprilgrid"}
          {@const gap = b.square - b.marker}
          {#each Array.from({ length: b.rows }) as _, r (r)}
            {#each Array.from({ length: b.cols }) as _, c (c)}
              {@const x = c * b.square + gap / 2}
              {@const y = r * b.square + gap / 2}
              {@const cell = b.marker / (N + 4)}
              <rect {x} {y} width={b.marker} height={b.marker} fill="#000" />
              <rect x={x + cell} y={y + cell} width={b.marker - 2 * cell} height={b.marker - 2 * cell} fill="#fff" />
              <rect x={x + 2 * cell} y={y + 2 * cell} width={b.marker - 4 * cell} height={b.marker - 4 * cell} fill="#000" />
              {#each bits(r * b.cols + c, N) as bit, k (k)}
                {#if bit}<rect x={x + (2 + (k % N)) * cell} y={y + (2 + Math.floor(k / N)) * cell} width={cell} height={cell} fill="#fff" />{/if}
              {/each}
              <rect x={x - gap / 2} y={y - gap / 2} width={gap / 2} height={gap / 2} fill="#000" />
            {/each}
          {/each}
        {:else}
          {#each Array.from({ length: b.rows }) as _, r (r)}
            {#each Array.from({ length: b.cols }) as _, c (c)}
              {#if (r + c) % 2 === 0}
                <rect x={c * b.square} y={r * b.square} width={b.square} height={b.square} fill="#000" />
              {:else if b.kind === "charuco"}
                {@const m = b.marker}
                {@const x = c * b.square + (b.square - m) / 2}
                {@const y = r * b.square + (b.square - m) / 2}
                {@const cell = m / (N + 2)}
                {@const idx = Math.floor((r * b.cols + c) / 2)}
                <rect {x} {y} width={m} height={m} fill="#000" />
                {#each bits(idx, N) as bit, k (k)}
                  {#if bit}<rect x={x + (1 + (k % N)) * cell} y={y + (1 + Math.floor(k / N)) * cell} width={cell} height={cell} fill="#fff" />{/if}
                {/each}
              {/if}
            {/each}
          {/each}
          <rect width={bw} height={bh} fill="none" stroke="#000" stroke-width="0.2" />
        {/if}
      </g>
      <text x={margin} y={paper.h - 5} font-size="3" fill="#555" font-family="sans-serif">
        HeliOS · {b.kind} {b.cols}×{b.rows} · square {b.square} mm{b.kind !== "chessboard" ? ` · marker ${b.marker} mm · ${b.dictionary}` : ""} · print at 100%
      </text>
      <line x1={margin} y1={paper.h - 12} x2={margin + 50} y2={paper.h - 12} stroke="#000" stroke-width="0.4" />
      <text x={margin + 52} y={paper.h - 11} font-size="2.6" fill="#555" font-family="sans-serif">50 mm check</text>
    </svg>
  </div>
</div>

<style>
  .maker {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .opts {
    width: 300px;
    flex-shrink: 0;
    overflow-y: auto;
    border-right: 1px solid var(--line);
  }
  .check {
    display: flex;
    gap: 7px;
    margin: 8px 10px;
    padding: 7px 9px;
    font-size: 11.5px;
    line-height: 1.4;
    color: var(--ok);
    background: var(--ok-bg);
    border-radius: var(--r-2);
  }
  .check.bad {
    color: var(--warn);
    background: var(--warn-bg);
  }
  .tips {
    margin: 0;
    padding: 0 14px 10px 28px;
    font-size: 11.5px;
    color: var(--fg-3);
    line-height: 1.6;
  }
  .tips b {
    color: var(--fg-2);
  }
  .preview {
    flex: 1;
    min-width: 0;
    display: grid;
    place-items: center;
    padding: 14px;
    background: var(--inset);
  }
  .page {
    max-width: 100%;
    max-height: 100%;
    width: auto;
    height: 100%;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  }
</style>
