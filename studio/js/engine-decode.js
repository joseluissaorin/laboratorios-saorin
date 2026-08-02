// engine-decode.js — decodificador con ACCESO ALEATORIO, como un NLE de
// verdad: se lee el ÍNDICE del contenedor (moov, unos KB, aunque viva en la
// cola del fichero) con peticiones Range, y los bytes de cada GOP se piden
// bajo demanda. Abrir un máster de 1 GB cuesta ~2 MB de red, no 1 GB.

const CHUNK = 1 << 20;   // 1 MB por petición de parseo

async function fetchRange(url, from, to) {
  const r = await fetch(url, { headers: { Range: `bytes=${from}-${to - 1}` } });
  if (!r.ok && r.status !== 206) throw new Error("range " + r.status);
  return await r.arrayBuffer();
}

export class ClipDecoder {
  async open(url) {
    if (!("VideoDecoder" in window)) throw new Error("sin WebCodecs");
    this.url = url;

    // tamaño total (Content-Range de una petición de 1 byte)
    const probe = await fetch(url, { headers: { Range: "bytes=0-0" } });
    const cr = probe.headers.get("Content-Range");
    this.size = cr ? parseInt(cr.split("/")[1], 10) : 0;
    if (!this.size) {
      const h = await fetch(url, { method: "HEAD" });
      this.size = parseInt(h.headers.get("Content-Length") || "0", 10);
    }
    if (!this.size) throw new Error("sin tamaño");

    // parsear SOLO las cabeceras: mp4box dice con nextParsePosition dónde
    // seguir — se salta el mdat entero y aterriza en el moov de la cola
    const file = MP4Box.createFile(false);   // false: NO retener los datos
    let ready = null;
    file.onReady = (info) => { ready = info; };
    file.onError = (e) => { throw new Error("mp4box: " + e); };
    let pos = 0;
    let vueltas = 0;
    while (!ready && vueltas++ < 64) {
      const to = Math.min(pos + CHUNK, this.size);
      const buf = await fetchRange(url, pos, to);
      buf.fileStart = pos;
      const next = file.appendBuffer(buf);
      file.flush();
      if (ready) break;
      if (next === undefined || next === null) break;
      // si pide algo que ya hemos cubierto, avanzar secuencialmente
      pos = next > pos && next < this.size ? next : to;
      if (pos >= this.size) break;
    }
    if (!ready) throw new Error("moov no encontrado");
    const track = ready.videoTracks[0];
    this.fps = track.nb_samples / (track.duration / track.timescale);
    this.duration = track.duration / track.timescale;
    this.width = track.video.width;
    this.height = track.video.height;

    // la tabla de samples COMPLETA (offset/size/cts/sync) sale del stbl,
    // sin tocar un solo byte del mdat
    const trak = file.getTrackById(track.id);
    if (!trak.samples || trak.samples.length < track.nb_samples) {
      try { file.updateSampleLists(); } catch {}
    }
    this.samples = (trak.samples || []).map((s) => ({
      off: s.offset, size: s.size,
      cts: s.cts, timescale: s.timescale, duration: s.duration,
      is_sync: s.is_sync,
    }));
    if (!this.samples.length) throw new Error("sin tabla de samples");

    let description = null;
    for (const entry of trak.mdia.minf.stbl.stsd.entries) {
      const box = entry.hvcC || entry.avcC || entry.vpcC;
      if (box) {
        const stream = new DataStream(undefined, 0, DataStream.BIG_ENDIAN);
        box.write(stream);
        description = new Uint8Array(stream.buffer, 8);
        break;
      }
    }
    this.codec = track.codec;
    this.description = description;
    const support = await VideoDecoder.isConfigSupported({ codec: this.codec, description });
    if (!support.supported) throw new Error(`códec no soportado: ${this.codec}`);
    this._mkDecoder();
    this._i = 0;
    this._span = null;   // {from, to, buf} — la ventana de bytes en memoria
    this._bulk = false;
    this.startBulk();    // el fichero entero, en segundo plano
    return this;
  }

  /** trae el fichero COMPLETO a memoria en segundo plano: tras esto, cada
      seek/next es local (cero red) — la velocidad de ayer con el arranque de
      hoy. Los ficheros gigantes se quedan en modo Range. */
  startBulk() {
    if (this._bulk || this.size > 2.2e9) return;
    this._bulk = true;
    fetch(this.url).then((r) => r.arrayBuffer()).then((buf) => {
      if (buf.byteLength === this.size) {
        this._span = { from: 0, to: this.size, buf };
        this._todo = true;
      }
    }).catch(() => { this._bulk = false; });
  }

  _mkDecoder() {
    this.queue = [];
    this.decoder = new VideoDecoder({
      output: (f) => { this.queue.push(f); },
      error: (e) => { console.error("decoder", e); },
    });
    this.decoder.configure({ codec: this.codec, description: this.description });
  }

  sampleT(s) { return s.cts / s.timescale; }

  /** cola de un solo carril: seek/next JAMÁS se solapan sobre el mismo
      decodificador (dos llamadas concurrentes se roban los frames y una
      recrea el decoder mientras la otra lo usa: el congelado con tirones) */
  _carril(fn) {
    const prev = this._cola || Promise.resolve();
    const p = prev.then(fn, fn);
    this._cola = p.catch(() => {});
    return p;
  }

  seek(t) { return this._carril(() => this._seek(t)); }
  next() { return this._carril(() => this._next()); }

  /** trae a memoria los bytes que cubren los samples [i0, i1] (una petición) */
  async _ensureSpan(i0, i1) {
    if (this._todo) return;          // el fichero entero ya está en RAM
    let from = Infinity, to = 0;
    for (let i = i0; i <= i1 && i < this.samples.length; i++) {
      from = Math.min(from, this.samples[i].off);
      to = Math.max(to, this.samples[i].off + this.samples[i].size);
    }
    if (this._span && this._span.from <= from && this._span.to >= to) return;
    // margen hacia delante: el siguiente lote llega gratis
    to = Math.min(to + 24 * CHUNK, this.size);
    this._span = { from, to, buf: await fetchRange(this.url, from, to) };
  }

  _chunkOf(i) {
    const s = this.samples[i];
    if (!this._span || s.off < this._span.from || s.off + s.size > this._span.to) return null;
    return new EncodedVideoChunk({
      type: s.is_sync ? "key" : "delta",
      timestamp: Math.round(s.cts * 1e6 / s.timescale),
      duration: Math.round(s.duration * 1e6 / s.timescale),
      data: new Uint8Array(this._span.buf, s.off - this._span.from, s.size),
    });
  }

  /** salta al último keyframe ≤ t y decodifica hasta alcanzar t */
  async _seek(t) {
    for (const f of this.queue) f.close();
    if (this.decoder.state !== "closed") this.decoder.close();
    this._mkDecoder();
    let key = 0, end = 0;
    for (let i = 0; i < this.samples.length; i++) {
      if (this.samples[i].is_sync && this.sampleT(this.samples[i]) <= t + 1e-4) key = i;
      end = i;
      if (this.sampleT(this.samples[i]) > t) break;
    }
    this._i = key;
    await this._ensureSpan(key, Math.min(end + 8, this.samples.length - 1));
    for (;;) {
      const f = await this._next();
      if (!f) return null;
      if (f.timestamp / 1e6 >= t - 0.5 / this.fps) return f;
      f.close();
    }
  }

  async _feed() {
    while (this.queue.length < 8 && this.decoder.decodeQueueSize < 6
           && this._i < this.samples.length && this.decoder.state === "configured") {
      let ch = this._chunkOf(this._i);
      if (!ch) {
        await this._ensureSpan(this._i, Math.min(this._i + 32, this.samples.length - 1));
        ch = this._chunkOf(this._i);
        if (!ch) return;
      }
      this._i++;
      this.decoder.decode(ch);
    }
    if (this._i >= this.samples.length && this.decoder.state === "configured")
      this.decoder.flush().catch(() => {});
  }

  /** siguiente frame en orden de presentación (o null al acabar) */
  async _next() {
    for (;;) {
      await this._feed();
      if (this.queue.length) {
        this.queue.sort((a, b) => a.timestamp - b.timestamp);
        return this.queue.shift();
      }
      if (this._i >= this.samples.length && this.decoder.decodeQueueSize === 0)
        return null;
      await new Promise((r) => setTimeout(r, 1));
    }
  }

  close() {
    for (const f of this.queue) f.close();
    this.queue = [];
    this._span = null;
    if (this.decoder && this.decoder.state !== "closed") this.decoder.close();
  }
}
