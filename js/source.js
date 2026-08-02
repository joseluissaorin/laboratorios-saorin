// source.js — demux (mp4box) + decode hardware (WebCodecs) del HEVC 10-bit
// original. Entrega planos YUV Uint16 por frame; el caller decide el fallback.

export class Source {
  constructor(){
    this.queue = [];
    this.samples = [];
    this.playing = false;
    this.time = 0;
    this._playWall = 0;
    this._feeding = 0;
    this._needKeyframe = true;
    this.onFrame = null;
  }

  async open(url, status = () => {}){
    status("descargando vídeo…");
    const buf = await (await fetch(url)).arrayBuffer();
    status("demuxing…");
    buf.fileStart = 0;
    const file = MP4Box.createFile();
    const info = await new Promise((res, rej) => {
      file.onReady = res; file.onError = rej;
      file.appendBuffer(buf); file.flush();
    });
    const track = info.videoTracks[0];
    this.duration = track.duration / track.timescale;
    this.fps = track.nb_samples / this.duration;
    this.width = track.video.width; this.height = track.video.height;

    this.samples = await new Promise((res) => {
      const acc = [];
      file.onSamples = (id, user, samples) => { acc.push(...samples); res(acc); };
      file.setExtractionOptions(track.id, null, { nbSamples: 1e9 });
      file.start();
      file.flush();
    });

    let description = null;
    const trak = file.getTrackById(track.id);
    for (const entry of trak.mdia.minf.stbl.stsd.entries){
      const box = entry.hvcC || entry.avcC || entry.vpcC;
      if (box){
        const stream = new DataStream(undefined, 0, DataStream.BIG_ENDIAN);
        box.write(stream);
        description = new Uint8Array(stream.buffer, 8);
        break;
      }
    }
    const codec = track.codec;
    status(`codec ${codec} · ${this.width}×${this.height}@${this.fps.toFixed(2)} · ${this.samples.length} samples`);

    let hwOk = false;
    if ("VideoDecoder" in window){
      try {
        const support = await VideoDecoder.isConfigSupported({
          codec, description, hardwareAcceleration: "prefer-hardware",
        });
        hwOk = !!support.supported;
      } catch (e) { hwOk = false; }
    }
    if (!hwOk) throw new Error("WebCodecs no decodifica " + codec);

    this.decoder = new VideoDecoder({
      output: (frame) => this._onFrame(frame),
      error: (e) => { (window.__dbg || (() => {}))("decoder ERR: " + e.message); status("error decoder: " + e.message); },
    });
    this.codec = codec;
    this.decoder.configure({ codec, description, hardwareAcceleration: "prefer-hardware" });
    this.seek(0);
  }

  _destride(buf, plane, rows, rowBytes){
    if (plane.stride === rowBytes)
      return new Uint16Array(buf, plane.offset, rows * rowBytes / 2);
    const out = new Uint16Array(rows * rowBytes / 2);
    const src = new Uint8Array(buf, plane.offset);
    const dst = new Uint8Array(out.buffer);
    for (let r = 0; r < rows; r++)
      dst.set(src.subarray(r * plane.stride, r * plane.stride + rowBytes), r * rowBytes);
    return out;
  }

  _onFrame(frame){
    try {
      (window.__dbg = window.__dbg || ((t) => { const el = document.querySelector("#debug"); if (el) el.textContent = (el.textContent + "\n" + t).split("\n").slice(-14).join("\n"); }));
      window.__dbg("out: " + frame.format + " " + frame.codedWidth + "x" + frame.codedHeight);
      this.__onFrame(frame);
    } catch (e) {
      window.__dbg("onFrame throw: " + e.message);
      frame.close();
    }
  }

  __onFrame(frame){
    const ts = frame.timestamp / 1e6, dur = (frame.duration || 0) / 1e6;
    const w = frame.codedWidth, h = frame.codedHeight;
    const fmt = frame.format || "";
    const ten = /P10|010/i.test(fmt);
    this.bitDepth = ten ? 10 : 8;
    const size = frame.allocationSize();
    const buf = new ArrayBuffer(size);
    frame.copyTo(buf).then((layout) => {
      if (!this._fmtLogged){ this._fmtLogged = true; (window.__dbg || (() => {}))("copyTo planes=" + layout.length + " " + layout.map(p => "stride" + p.stride + "/len" + p.length).join(" ")); }
      let y, u, v, cw = w >> 1, ch = h >> 1;
      if (layout.length === 3){               // I420 / I420P10
        const bpp = ten ? 2 : 1;
        if (ten){
          y = this._destride(buf, layout[0], h, w * bpp);
          u = this._destride(buf, layout[1], ch, cw * bpp);
          v = this._destride(buf, layout[2], ch, cw * bpp);
        } else {                              // 8-bit → u16 escalado
          const cv = (p, rows, rb) => {
            const s = new Uint8Array(buf, p.offset, rows * rb);
            const d = new Uint16Array(s.length);
            for (let i = 0; i < s.length; i++) d[i] = s[i] << 8;
            return d;
          };
          y = cv(layout[0], h, w); u = cv(layout[1], ch, cw); v = cv(layout[2], ch, cw);
        }
      } else if (layout.length === 2){        // NV12 / P010: desinterlevar UV
        const bpp = ten ? 2 : 1;
        y = this._destride(buf, layout[0], h, w * bpp);
        const uv = this._destride(buf, layout[1], ch, w * bpp);
        u = new Uint16Array(cw * ch); v = new Uint16Array(cw * ch);
        for (let i = 0; i < cw * ch; i++){ u[i] = uv[2*i]; v[i] = uv[2*i+1]; }
      } else {
        frame.close();
        throw new Error("formato de frame no soportado: " + fmt);
      }
      frame.close();
      this.queue.push({ ts, dur, y, u, v, w, h, cw, ch, ten });
      this.queue.sort((a, b) => a.ts - b.ts);
      if (this.onFrame) this.onFrame();
    }).catch(() => frame.close());
  }

  _feed(){
    if (!this.decoder || this.decoder.state === "closed") return;
    while (this.queue.length < 8 && this.decoder.decodeQueueSize < 4
           && this._feeding < this.samples.length){
      const s = this.samples[this._feeding++];
      if (this._needKeyframe && !s.is_sync) continue;
      this._needKeyframe = false;
      this.decoder.decode(new EncodedVideoChunk({
        type: s.is_sync ? "key" : "delta",
        timestamp: Math.round(s.cts * 1e6 / s.timescale),
        duration: Math.round(s.duration * 1e6 / s.timescale),
        data: s.data,
      }));
    }
  }

  seek(t){
    t = Math.max(0, Math.min(t, this.duration - 0.05));
    this.time = t;
    if (this.playing) this._playWall = performance.now() / 1000 - t;
    this.queue = [];
    let k = 0;
    for (let i = 0; i < this.samples.length; i++){
      if (this.samples[i].is_sync && this.samples[i].cts / this.samples[i].timescale <= t) k = i;
      if (this.samples[i].cts / this.samples[i].timescale > t) break;
    }
    this._feeding = k;
    this._needKeyframe = true;
    this._feed();
  }

  play(){ this.playing = true; this._playWall = performance.now() / 1000 - this.time; }
  pause(){ this.playing = false; }
  toggle(){ this.playing ? this.pause() : this.play(); }

  current(){
    if (this.playing){
      this.time = performance.now() / 1000 - this._playWall;
      if (this.time >= this.duration) this.seek(0);
    }
    this._feed();
    const tol = 0.5 / (this.fps || 60);
    let best = null;
    for (const q of this.queue){
      if (q.ts <= this.time + tol) best = q; else break;
    }
    if (best){
      const i = this.queue.indexOf(best);
      if (i > 4) this.queue.splice(0, i - 4);   // conserva un pequeño historial
    }
    return best;
  }
}
