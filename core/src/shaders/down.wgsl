// down.wgsl — downsample 2× con 5 taps (caja + diagonales).

struct DownParams { texel: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> P: DownParams;
@group(0) @binding(1) var tIn: texture_2d<f32>;
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
  var c = textureSampleLevel(tIn, samp, in.uv, 0.0).rgb * 4.0;
  c += textureSampleLevel(tIn, samp, in.uv + P.texel * vec2(-1.0, -1.0), 0.0).rgb;
  c += textureSampleLevel(tIn, samp, in.uv + P.texel * vec2(1.0, -1.0), 0.0).rgb;
  c += textureSampleLevel(tIn, samp, in.uv + P.texel * vec2(-1.0, 1.0), 0.0).rgb;
  c += textureSampleLevel(tIn, samp, in.uv + P.texel * vec2(1.0, 1.0), 0.0).rgb;
  return vec4(c / 8.0, 1.0);
}
