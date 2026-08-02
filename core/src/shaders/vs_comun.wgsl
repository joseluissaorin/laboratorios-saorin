// vs_comun.wgsl — el triángulo de pantalla completa, y NADA MÁS. Lo comparten
// los pases que no traen el suyo (los empaquetadores a planos).
//
// Ojo: aquí no van declaraciones de recursos. Al partir `grade_bi.wgsl` se me
// colaron sus bindings y winlab reventaba con «redefinition of samp» al
// pegarle este vértice a otro fragmento que declara los suyos.
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
  var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
  var o: VsOut;
  o.pos = vec4(p[vi], 0.0, 1.0);
  o.uv = vec2(p[vi].x, -p[vi].y) * 0.5 + 0.5;
  return o;
}

struct GradeU {
  src_mode: u32, full_range: u32, lut_na: u32, lut_nb: u32,
  lut_a_on: u32, lut_b_on: u32, yuv_norm: f32, gain: f32,
  push_pull: f32, compress: f32, compress_wp: f32, compress_range: f32,
  src_w: f32, src_h: f32, pad0: f32, pad1: f32,
  enc_a: vec4<f32>, enc_b: vec4<f32>, paso: vec4<f32>,
  peso: f32, pad3: f32, pad4: f32, pad5: f32,
};
