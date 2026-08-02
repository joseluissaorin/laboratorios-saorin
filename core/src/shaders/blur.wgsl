// blur.wgsl — gaussiana separable 9 taps, radio escalable.

struct BlurParams { dir: vec2<f32>, radius: f32, pad: f32 };
@group(0) @binding(0) var<uniform> P: BlurParams;
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
  var w = array<f32, 5>(0.227027, 0.1945946, 0.1216216, 0.054054, 0.016216);
  var c = textureSampleLevel(tIn, samp, in.uv, 0.0).rgb * w[0];
  for (var i = 1; i < 5; i++) {
    let off = P.dir * f32(i) * P.radius;
    c += textureSampleLevel(tIn, samp, in.uv + off, 0.0).rgb * w[i];
    c += textureSampleLevel(tIn, samp, in.uv - off, 0.0).rgb * w[i];
  }
  return vec4(c, 1.0);
}
