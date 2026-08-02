// motion.wgsl — métrica de movimiento inter-frame (para auto shutter).

@group(0) @binding(0) var tA: texture_2d<f32>;
@group(0) @binding(1) var tB: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

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
  let a = textureSampleLevel(tA, samp, in.uv, 0.0).rgb;
  let b = textureSampleLevel(tB, samp, in.uv, 0.0).rgb;
  return vec4(dot(abs(a - b), vec3(0.3333)), 0.0, 0.0, 1.0);
}
