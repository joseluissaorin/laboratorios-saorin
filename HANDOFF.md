# HANDOFF — filmlook: GPU-native film-emulation video renderer (macOS, Metal+VideoToolbox)

> **⚠️ ACTUALIZADO 30-07-2026 — EL OBJETIVO ESTÁ CUMPLIDO.** Los "current bugs"
> de abajo están TODOS resueltos y el requisito de >350 fps está superado:
> **413–464 fps e2e** con ProRes 422 HQ (2 sesiones VT round-robin por frame →
> los 2 motores ProRes del M4 Max), ~440 fps con ProRes 4444, ~164 fps con HEVC
> 10-bit (techo físico: el M4 Max solo tiene UN motor HEVC utilizable, medido).
> Mux por AVAssetWriter passthrough con audio en vivo. El clip de referencia de
> 92,9 s renderiza en 14,7 s. Léete `TECHNICAL_REPORT.md` (raíz de
> film-look-lab): documenta los 5 bugs que causaban el vídeo negro/corrupto,
> las técnicas y todas las mediciones. Este documento se conserva como
> contexto histórico del diseño.

You are taking over a deep, working-but-unfinished engineering project. Read this fully before touching anything.

## Mission

Render 4K 10-bit video with a full film-emulation look (baked LUTs + procedural film grain/halation/bloom/vignette/etc.) at **>350 fps end-to-end on an Apple M4 Max Mac** (hard requirement), at master quality (ProRes 4444 or HEVC 10-bit high bitrate), cross-platform in principle (Windows/Linux later via wgpu), agent/CLI-driveable.

Current verified state on the M4 Max: **~60 fps e2e with encode, ~277 fps decode+render-only (bench)**. The GPU shader chain costs **~0.5 ms/frame**. The requirement is achievable — every zero-copy stage is 0.2–2 ms — but the VideoToolbox encode path has 2–3 remaining bugs (see "Current bugs").

## What filmlook is (the product)

A DaVinci-Resolve-free renderer for the user's signature look. Two parts:

1. **Baked color**: two chained 3D LUTs applied in the grade shader:
   - LUT A: input transform (e.g. `Luna_I-Log_to_Rec709_BT1886_s65_v2.cube`, Insta360 I-Log → 709)
   - LUT B: creative grade for 709 signal (`pre 709 conversion 65 puntos - Cube_1.hald.cube`, baked by the user in Resolve via HaldCLUT).
2. **Procedural film emulation** (one big fragment shader, ported GLSL→WGSL→MSL, all three exist):
   - grain (FFT tileable plate texture + crisp quantum-cell texelFetch layer + neg/print asymmetry + tonal response shadows/mids/highs + per-channel weights + defocus)
   - halation (two blurred lobes ¼+⅛ res, radius-dependent hue orange-core→red-fringe, threshold, whiten)
   - bloom (veiling glare, threshold, warm tint), softness/diffusion, acutance (edge halo)
   - cos⁴-law vignette (size/roundness/center), radial chromatic aberration
   - gate weave (+rotation), flicker (fast) + film breath (slow random walk + CMY drift), dust & scratches
   - FILM COLOR stage: luminance-coupled hue skews (cyan→blue, green→yellow, red→orange in highs; magenta→red, blue→cyan in shadows; yellow stable = skin line), layer crosstalk, subtractive-density saturation (saturation darkens, mids hold, shadows keep hue residue), 2383-style print stage (S-curve, teal D-min, warm highlights, gamut ceiling)
   - push/pull response remap, film highlight compression (Dmax sponge), slow-shutter temporal IIR accumulator, film-gate frame with imperfect wobbled edges
   - Params arrive as a JSON prefs file (same schema as `~/Downloads/filmlook-prefs.json`).

## Repo layout (`auto-davinci/film-look-lab/`)

- `app/` — Tauri app (working UI for tuning; WebKit webview, used for look development, NOT the perf path)
- `js/`, `index.html`, `server.py` — original web lab (works, legacy)
- `core/` — wgpu Rust renderer (works: correct output verified, ~21 fps e2e via ffmpeg pipes; WGSL shaders in `core/src/shaders/`)
- `metal/` — **the current focus**: pure Metal + VideoToolbox zero-copy renderer. THIS is what you must finish.
  - `metal/src/vt_ffi.rs` — FFI to VideoToolbox/CoreVideo/CoreMedia (C APIs)
  - `metal/src/metal_pipe.rs` — Metal device, pipelines, targets, Renderer
  - `metal/src/shaders/chain.metal` — the whole shader chain in MSL (grade/down/blur/accum/comp)
  - `metal/src/decode_vt.rs` — VTDecompressionSession → CVPixelBuffer → CVMetalTextureCache → MTLTexture (zero-copy, WORKS) + Annex-B demux parser (EPB-aware, WORKS)
  - `metal/src/encode_vt.rs` — VTCompressionSession, CVPixelBufferPool (IOSurface, BGRA), AVCC→AnnexB bitstream → ffmpeg mux pipe
  - `metal/src/main.rs` — CLI: demux (ffmpeg hevc_mp4toannexb pipe) → VT decode → Metal render → VT encode → mux
- `tools/` — LUT/hald/drx/grain-plate utilities; `assets/` — grain.bin (FFT plate, 1024² f16), LUTs as .bin
- Test assets in `~/Downloads/lut360 saorin/`: `VID_20260714_205527_099.mp4` (4K HEVC 10-bit 59.94fps, 92.9s, 1GB), LUTs, `ref_graded.png` (Resolve reference)

## Verified working (do not re-verify, build on these)

1. **Zero-copy decode**: VTDecompressionSession decodes HEVC → CVPixelBuffer P010 → `CVMetalTextureCacheCreateTextureFromImage` → MTLTextures (Y R16Unorm, UV RG16Unorm). ~0 ms/frame import. Gotchas already fixed: `CVMetalTextureCacheCreate` takes **5 args** (textureAttributes before cacheOut); CMSampleBuffer needs the **format description** or VT drops frames; device pointer must come from `msg_send![&*device, self]`.
2. **The Metal render itself is correct**: CPU readback of the encode-input CVPixelBuffer showed correct graded pixels (`[211,92,18,255]` sky teal etc.) at 4K.
3. **Speed**: decode+render-only bench = **276 fps e2e** (render+wait 0.9 ms). Demux via ffmpeg annex-B pipe takes 0.7s for 5568 NALs (fine).
4. **Mux works**: ffmpeg `-f hevc -i -` pipe; must write VT's OWN parameter sets (from `CMVideoFormatDescriptionGetHEVCParameterSetAtIndex` on the output sample's format description) — NOT the source's, or RPS errors.
5. `AllowFrameReordering` must be `kCFBooleanFalse` explicitly (NULL = default true = B-frames = out-of-order NALs in a timestamp-less raw stream).
6. ffmpeg's `yuv420p10le` rawvideo is **LSB-aligned** (0–1023), P010 from VT is MSB-aligned unorm16 — the Metal shader samples unorm directly, correct.

## Current bugs (the work left)

As of the last run (`metal_frame5.png` generated but NOT yet visually verified):

1. **Color cast (green)**: VT converts BGRA→YUV with BT.601 by default. Fix attempted: set `kVTCompressionPropertyKey_ColorPrimaries/YCbCrMatrix/TransferFunction` to "ITU_R_709_2" via `cfstr()` (the kVTColorPrimaries_ITU_R_709_2 symbol does NOT exist in the SDK — use string values). VERIFY the cast is gone.
2. **Orientation**: Metal textures are top-down (no GL flip). The `1.0 - in.uv.y` flip in `fs_grade` (chain.metal) was removed for Metal — VERIFY output is right-side-up (last verified frame before the fix was upside-down).
3. **encode-submit ~8.5 ms/frame**: VTCompressionSessionEncodeFrame should submit in <0.5 ms. Suspects: CVPixelBufferPool exhaustion (alloc blocking — raise `kCVPixelBufferPoolMinimumBufferCountKey` ~8), or the drain thread's `wait_until_completed` being counted in this bucket. Profile first, then fix. This is THE number between 60 and 300+ fps.
4. Possible quality settings: `RealTime=true` is currently set (limits quality/parallelism?) — evaluate removing.
5. GOP-parallel decode (2–3 staggered VT sessions on GOP boundaries) is designed but NOT implemented — needed only if single-session decode tops out below 350 fps after encode is fixed (M4 Max single VT HEVC 4K session ≈ 300–500 fps).

## Hard requirement

**>350 fps end-to-end 4K** on this M4 Max: demux+decode+render+encode+mux, correct output (visually matching the Tauri/web preview look), ProRes 4444 (VT hardware ProRes on M4 — `kVTVideoCodecType_AppleProRes4444`, needs own MOV muxer or frame-packet mux) or HEVC 10-bit ≥40Mbps. GPU shader chain is 0.5 ms; budget per stage ≤1 ms.

## Constraints & style

- Rust, minimal deps. VT/CoreVideo/CoreMedia are C APIs — FFI is fine (already in vt_ffi.rs). Metal via `metal` crate.
- No per-frame CPU pixel copies anywhere. No per-frame texture creation if avoidable (CVMetalTextureCache per frame is tolerable; pool everything else).
- Fences/completion handlers instead of `wait_until_completed` where possible.
- Bench discipline: `--bench` mode exists; always report e2e fps + per-stage ms.
- The Tauri app (`app/`) is the visual reference for the look — the Metal render must match it.
- Run: `cd film-look-lab/metal && cargo build --release && ./target/release/filmlook-metal render "<input>" -o /tmp/out.mp4 --lut-in "<ilog cube>" --lut "<grade cube>" --prefs ~/Downloads/filmlook-prefs.json --max-frames 240` (paths with spaces need quotes).
