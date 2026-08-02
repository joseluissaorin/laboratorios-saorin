// decode.js — decodificación secuencial WebCodecs para el render.
// Sin seeks: demux (mp4box) + VideoDecoder al ritmo del hardware (cientos de
// fps en 4K), entregando VideoFrames en orden. Fallback a null si no hay soporte.

export class SeqDecoder {
  async open(url){
    if (!("VideoDecoder" in window)) return false;
    const buf = await (await fetch(url)).arrayBuffer();
    buf.fileStart = 0;
    const file = MP4Box.createFile();
    const info = await new Promise((res, rej) => {
      file.onReady = res; file.onError = rej;
      file.appendBuffer(buf); file.flush();
    });
    const track = info.videoTracks[0];
    this.fps = track.nb_samples / (track.duration / track.timescale);
    this.duration = track.duration / track.timescale;
    this.width = track.video.width; this.height = track.video.height;
    this.samples = await new Promise((res) => {
      const acc = [];
      file.onSamples = (id, u, s) => { acc.push(...s); res(acc); };
      file.setExtractionOptions(track.id, null, { nbSamples: 1e9 });
      file.start(); file.flush();
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
    try {
      const support = await VideoDecoder.isConfigSupported({ codec, description });
      if (!support.supported) return false;
    } catch { return false; }
    this.queue = [];
    this.decoder = new VideoDecoder({
      output: (f) => { this.queue.push(f); },
      error: (e) => { this.err = e; },
    });
    this.decoder.configure({ codec, description });
    this._i = 0;
    return true;
  }

  _feed(){
    while (this.queue.length < 8 && this.decoder.decodeQueueSize < 6
           && this._i < this.samples.length && this.decoder.state === "configured"){
      const s = this.samples[this._i++];
      this.decoder.decode(new EncodedVideoChunk({
        type: s.is_sync ? "key" : "delta",
        timestamp: Math.round(s.cts * 1e6 / s.timescale),
        duration: Math.round(s.duration * 1e6 / s.timescale),
        data: s.data,
      }));
    }
    if (this._i >= this.samples.length && this.decoder.state === "configured")
      this.decoder.flush().catch(() => {});
  }

  // siguiente frame en orden de presentación (o null al acabar)
  async next(){
    for (;;){
      this._feed();
      if (this.queue.length){
        this.queue.sort((a, b) => a.timestamp - b.timestamp);
        return this.queue.shift();
      }
      if (this._i >= this.samples.length && this.decoder.decodeQueueSize === 0)
        return null;
      await new Promise(r => setTimeout(r, 1));
    }
  }
}
