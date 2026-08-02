// accum.wgsl — slow shutter: integración temporal IIR. También sirve de blit.

struct AccumParams { feedback: f32, reset: u32, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> P: AccumParams;
@group(0) @binding(1) var tCurr: texture_2d<f32>;
@group(0) @binding(2) var tPrev: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut;
  o.pos = vec4(p[vi], 0.0, 1.0);
  o.uv = vec2(p[vi].x, -p[vi].y) * 0.5 + 0.5;  // fila 0 = arriba: cada pase preserva orientación (fix fantasma invertido)
  return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let c = textureSampleLevel(tCurr, samp, in.uv, 0.0).rgb;
  let p = textureSampleLevel(tPrev, samp, in.uv, 0.0).rgb;
  if P.reset == 1u { return vec4(c, 1.0); }
  return vec4(mix(c, p, P.feedback), 1.0);
}
