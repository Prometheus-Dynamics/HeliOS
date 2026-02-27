<script lang="ts">
  import { onDestroy, createEventDispatcher } from 'svelte';

  type EncodedFormat = 'h264' | 'h265';

  const { url, format, fitClass = 'object-contain object-center' } = $props<{
    url: string;
    format: EncodedFormat;
    fitClass?: string;
  }>();

  const dispatch = createEventDispatcher<{ error: { message: string }; frame: { w: number; h: number } }>();

  let canvas: HTMLCanvasElement | null = null;
  let running = false;
  let abortController: AbortController | null = null;
  let decoder: VideoDecoder | null = null;
  let frameIndex = 0;
  let configured = false;
  let hasFrame = false;
  let stallTimer: ReturnType<typeof setInterval> | null = null;
  let awaitingResyncKeyframe = false;

  class ByteQueue {
    private chunks: Uint8Array[] = [];
    private headOffset = 0;
    private total = 0;

    push(chunk: Uint8Array): void {
      if (!chunk.length) return;
      this.chunks.push(chunk);
      this.total += chunk.length;
    }

    available(): number {
      return this.total;
    }

    private readByteAt(offset: number): number {
      let idx = 0;
      let off = offset + this.headOffset;
      while (idx < this.chunks.length) {
        const chunk = this.chunks[idx];
        if (off < chunk.length) return chunk[off]!;
        off -= chunk.length;
        idx++;
      }
      return 0;
    }

    readU32LE(): number | null {
      if (this.total < 4) return null;
      const b0 = this.readByteAt(0);
      const b1 = this.readByteAt(1);
      const b2 = this.readByteAt(2);
      const b3 = this.readByteAt(3);
      return (b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)) >>> 0;
    }

    consume(count: number): void {
      let remaining = Math.min(count, this.total);
      this.total -= remaining;
      while (remaining > 0 && this.chunks.length) {
        const head = this.chunks[0]!;
        const headRemaining = head.length - this.headOffset;
        if (remaining < headRemaining) {
          this.headOffset += remaining;
          return;
        }
        remaining -= headRemaining;
        this.chunks.shift();
        this.headOffset = 0;
      }
    }

    readBytes(count: number): Uint8Array | null {
      if (count <= 0) return new Uint8Array(0);
      if (this.total < count) return null;
      const out = new Uint8Array(count);
      let written = 0;
      let idx = 0;
      let offset = this.headOffset;
      while (written < count && idx < this.chunks.length) {
        const chunk = this.chunks[idx]!;
        const take = Math.min(count - written, chunk.length - offset);
        out.set(chunk.subarray(offset, offset + take), written);
        written += take;
        idx += 1;
        offset = 0;
      }
      this.consume(count);
      return out;
    }

    reset(): void {
      this.chunks = [];
      this.headOffset = 0;
      this.total = 0;
    }
  }

  function stop(): void {
    running = false;
    abortController?.abort();
    abortController = null;
    if (stallTimer) {
      clearInterval(stallTimer);
      stallTimer = null;
    }
    try {
      decoder?.close();
    } catch {
      // ignore
    }
    decoder = null;
    configured = false;
    hasFrame = false;
    frameIndex = 0;
    awaitingResyncKeyframe = false;
  }

  function emitError(message: string): void {
    dispatch('error', { message });
  }

  function hasWebCodecs(): boolean {
    return typeof VideoDecoder !== 'undefined';
  }

  function removeEmulationPreventionBytes(rbsp: Uint8Array): Uint8Array {
    const out: number[] = [];
    for (let i = 0; i < rbsp.length; i++) {
      if (i >= 2 && rbsp[i] === 0x03 && rbsp[i - 1] === 0x00 && rbsp[i - 2] === 0x00) {
        continue;
      }
      out.push(rbsp[i]);
    }
    return Uint8Array.from(out);
  }

  function splitAnnexB(buf: Uint8Array): Uint8Array[] {
    const units: Uint8Array[] = [];
    const n = buf.length;
    let i = 0;
    let start = -1;

    const isStartCodeAt = (idx: number): number => {
      if (idx + 3 <= n && buf[idx] === 0x00 && buf[idx + 1] === 0x00 && buf[idx + 2] === 0x01) {
        return 3;
      }
      if (
        idx + 4 <= n &&
        buf[idx] === 0x00 &&
        buf[idx + 1] === 0x00 &&
        buf[idx + 2] === 0x00 &&
        buf[idx + 3] === 0x01
      ) {
        return 4;
      }
      return 0;
    };

    while (i < n) {
      const sc = isStartCodeAt(i);
      if (sc) {
        if (start >= 0) {
          const unit = buf.subarray(start, i);
          if (unit.length) units.push(unit);
        }
        i += sc;
        start = i;
        continue;
      }
      i++;
    }
    if (start >= 0 && start < n) {
      const unit = buf.subarray(start);
      if (unit.length) units.push(unit);
    }
    return units;
  }

  function isKeyframe(data: Uint8Array, kind: EncodedFormat): boolean {
    const units = splitAnnexB(data);
    if (kind === 'h264') {
      return units.some((unit) => (unit[0] & 0x1f) === 5);
    }
    return units.some((unit) => ((unit[0] >> 1) & 0x3f) === 19 || ((unit[0] >> 1) & 0x3f) === 20 || ((unit[0] >> 1) & 0x3f) === 21);
  }

  function hex2(v: number): string {
    return v.toString(16).padStart(2, '0');
  }

  function bytesToHex(bytes: Uint8Array): string {
    return Array.from(bytes)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
  }

  function trimHexPairsRight(hex: string): string {
    let out = hex.toLowerCase();
    while (out.endsWith('00') && out.length > 2) {
      out = out.slice(0, -2);
    }
    return out;
  }

  class BitReader {
    private bytes: Uint8Array;
    private bitOffset = 0;
    constructor(bytes: Uint8Array) {
      this.bytes = bytes;
    }
    readBits(count: number): number {
      let value = 0;
      for (let i = 0; i < count; i++) {
        const byteIndex = this.bitOffset >> 3;
        const bitIndex = 7 - (this.bitOffset & 7);
        const bit = byteIndex < this.bytes.length ? (this.bytes[byteIndex] >> bitIndex) & 1 : 0;
        value = (value << 1) | bit;
        this.bitOffset++;
      }
      return value >>> 0;
    }
    skipBits(count: number): void {
      this.bitOffset += count;
    }
  }

  function findH264CodecString(packet: Uint8Array): string | null {
    const units = splitAnnexB(packet);
    for (const unit of units) {
      if ((unit[0] & 0x1f) !== 7) continue;
      const rbsp = removeEmulationPreventionBytes(unit.subarray(1));
      if (rbsp.length < 3) return null;
      const profile = rbsp[0];
      const constraints = rbsp[1];
      const level = rbsp[2];
      return `avc1.${hex2(profile)}${hex2(constraints)}${hex2(level)}`;
    }
    return null;
  }

  function findHevcCodecString(packet: Uint8Array): string | null {
    const units = splitAnnexB(packet);
    for (const unit of units) {
      const nalType = (unit[0] >> 1) & 0x3f;
      if (nalType !== 32) continue; // VPS
      const rbsp = removeEmulationPreventionBytes(unit.subarray(2));
      const br = new BitReader(rbsp);
      br.readBits(4); // vps_video_parameter_set_id
      br.readBits(2); // vps_reserved_three_2bits
      br.readBits(6); // vps_max_layers_minus1
      const maxSubLayersMinus1 = br.readBits(3);
      br.readBits(1); // vps_temporal_id_nesting_flag
      br.readBits(16); // vps_reserved_0xffff_16bits

      const profileSpace = br.readBits(2);
      const tierFlag = br.readBits(1);
      const profileIdc = br.readBits(5);
      let compat = 0;
      for (let i = 0; i < 32; i++) {
        compat = (compat << 1) | br.readBits(1);
      }
      const constraintBytes = new Uint8Array(6);
      for (let i = 0; i < 6; i++) {
        constraintBytes[i] = br.readBits(8);
      }
      const levelIdc = br.readBits(8);

      const subProfilePresent: number[] = [];
      const subLevelPresent: number[] = [];
      for (let i = 0; i < maxSubLayersMinus1; i++) {
        subProfilePresent.push(br.readBits(1));
        subLevelPresent.push(br.readBits(1));
      }
      if (maxSubLayersMinus1 > 0) {
        for (let i = maxSubLayersMinus1; i < 8; i++) {
          br.readBits(2);
        }
      }
      for (let i = 0; i < maxSubLayersMinus1; i++) {
        if (subProfilePresent[i]) br.skipBits(88);
        if (subLevelPresent[i]) br.skipBits(8);
      }

      const profileSpaceChar = profileSpace === 1 ? 'A' : profileSpace === 2 ? 'B' : profileSpace === 3 ? 'C' : '';
      const profilePart = `${profileSpaceChar}${profileIdc}`;
      const compatHex = Math.max(compat >>> 0, 0).toString(16);
      const tierPart = `${tierFlag ? 'H' : 'L'}${levelIdc}`;
      const constraintHex = trimHexPairsRight(bytesToHex(constraintBytes));
      return `hvc1.${profilePart}.${compatHex}.${tierPart}.${constraintHex}`;
    }
    return null;
  }

  async function ensureConfigured(packet: Uint8Array): Promise<boolean> {
    if (configured) return true;
    const codec =
      format === 'h264' ? findH264CodecString(packet) : findHevcCodecString(packet);
    if (!codec) return false;

    const config: VideoDecoderConfig = {
      codec,
      optimizeForLatency: true,
      hardwareAcceleration: 'prefer-hardware'
    };

    try {
      const supported = await VideoDecoder.isConfigSupported(config);
      if (!supported.supported) {
        emitError(`WebCodecs does not support ${codec}`);
        return false;
      }
      decoder?.configure(config);
      configured = true;
      return true;
    } catch (err) {
      emitError(err instanceof Error ? err.message : 'WebCodecs configuration failed');
      return false;
    }
  }

  async function start(): Promise<void> {
    stop();
    if (!hasWebCodecs()) {
      emitError('WebCodecs VideoDecoder is not available in this browser');
      return;
    }
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) {
      emitError('Unable to create canvas context');
      return;
    }

    abortController = new AbortController();
    const controller = abortController;
    running = true;

    let lastDataAt = Date.now();
    const fail = (message: string): void => {
      if (!running) return;
      running = false;
      if (stallTimer) {
        clearInterval(stallTimer);
        stallTimer = null;
      }
      emitError(message);
    };
    stallTimer = setInterval(() => {
      if (!running) return;
      if (Date.now() - lastDataAt < 2500) return;
      controller.abort();
      fail('Stream stalled (no data)');
    }, 500);

    hasFrame = false;
    decoder = new VideoDecoder({
      output: (frame) => {
        try {
          if (canvas && (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight)) {
            canvas.width = frame.displayWidth;
            canvas.height = frame.displayHeight;
          }
          ctx.drawImage(frame, 0, 0, canvas?.width ?? frame.displayWidth, canvas?.height ?? frame.displayHeight);
          if (!hasFrame) {
            hasFrame = true;
            dispatch('frame', { w: frame.displayWidth, h: frame.displayHeight });
          }
        } finally {
          frame.close();
        }
      },
      error: (err) => {
        emitError(err instanceof Error ? err.message : 'Decoder error');
      }
    });

    try {
      const response = await fetch(url, {
        method: 'GET',
        cache: 'no-store',
        headers: {
          accept: 'application/octet-stream',
          'cache-control': 'no-cache',
          pragma: 'no-cache'
        },
        signal: controller.signal
      });
      if (!response.ok || !response.body) {
        fail(`Stream request failed (${response.status})`);
        return;
      }

      const reader = response.body.getReader();
      const queue = new ByteQueue();
      const frameDurationUs = 33_333;
      const SOFT_QUEUE_LIMIT = 6;
      const HARD_QUEUE_LIMIT = 18;

      while (running) {
        const { value, done } = await reader.read();
        if (done) break;
        if (!value || value.length === 0) continue;
        lastDataAt = Date.now();

        // Prevent unbounded growth if the parser gets out of sync.
        if (queue.available() > 32 * 1024 * 1024) {
          queue.reset();
        }
        queue.push(value);

        while (queue.available() >= 4) {
          const len = queue.readU32LE();
          if (len == null) break;
          if (queue.available() < 4 + len) break;
          queue.consume(4);
          const packet = queue.readBytes(len);
          if (!packet) break;

          const key = isKeyframe(packet, format);
          if (!configured) {
            if (!key) continue;
            const ok = await ensureConfigured(packet);
            if (!ok) continue;
          }

          const queueDepth = decoder?.decodeQueueSize ?? 0;
          if (queueDepth >= HARD_QUEUE_LIMIT) {
            awaitingResyncKeyframe = true;
            try {
              decoder?.reset();
            } catch {
              // ignore; next keyframe will reconfigure
            }
            configured = false;
            if (!key) continue;
            const ok = await ensureConfigured(packet);
            if (!ok) continue;
            awaitingResyncKeyframe = false;
          }

          if (awaitingResyncKeyframe) {
            if (!key) continue;
            const ok = await ensureConfigured(packet);
            if (!ok) continue;
            awaitingResyncKeyframe = false;
          } else if ((decoder?.decodeQueueSize ?? 0) >= SOFT_QUEUE_LIMIT && !key) {
            // Drop non-key frames when decode is falling behind to keep preview near-live.
            continue;
          }

          const chunk = new EncodedVideoChunk({
            type: key ? 'key' : 'delta',
            timestamp: frameIndex * frameDurationUs,
            data: packet
          });
          frameIndex += 1;
          decoder.decode(chunk);
        }
      }
      if (running && !controller.signal.aborted) {
        fail('Stream ended unexpectedly');
      }
    } catch (err) {
      if (controller.signal.aborted) return;
      fail(err instanceof Error ? err.message : 'Stream error');
    }
  }

  onDestroy(() => {
    stop();
  });

  $effect(() => {
    if (!canvas) return;
    // Include `url` + `format` so changes restart the decoder pipeline.
    const _ = url + format;
    void _;
    void start();
  });
</script>

<canvas class={`block h-full w-full bg-black ${fitClass}`} bind:this={canvas} aria-label="Encoded stream preview"></canvas>
