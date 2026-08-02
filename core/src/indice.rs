//! El índice del contenedor: se lee el moov UNA vez (milisegundos, aunque el
//! máster pese un giga) y a partir de ahí cada seek es O(log n): muestra →
//! keyframe → offset exacto en el mdat. Las muestras del mdat ya están en
//! AVCC (longitud + payload), listas para VideoToolbox/MediaFoundation sin
//! reempaquetar. Nada de ffprobe ni procesos en el camino interactivo.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug)]
pub enum Codec {
    /// hvcC: parameter sets VPS/SPS/PPS + tamaño del prefijo de longitud
    Hevc { vps: Vec<u8>, sps: Vec<u8>, pps: Vec<u8>, nal_len: u8 },
    /// avcC: SPS/PPS + tamaño del prefijo
    H264 { sps: Vec<u8>, pps: Vec<u8>, nal_len: u8 },
}

#[derive(Clone, Copy, Debug)]
pub struct Muestra {
    pub off: u64,
    pub tam: u32,
    /// presentación, en unidades del timescale
    pub pts: i64,
    pub key: bool,
}

pub struct Indice {
    /// las dimensiones **tal y como están guardadas**: es lo que entrega el
    /// decodificador y lo que hay que darle al conform
    pub w: u32,
    pub h: u32,
    pub fps: f64,
    pub dur: f64,
    pub timescale: u32,
    pub codec: Codec,
    /// LA ORIENTACIÓN DEL FICHERO, en cuartos de vuelta a derechas (0..3).
    ///
    /// Los móviles graban SIEMPRE apaisado y meten en el `tkhd` una matriz que
    /// dice «esto va girado 90°». El taller la leía en `probe` (ffprobe) y la
    /// aplicaba el corte de ffmpeg, pero ese corte ya no existe: el visor y el
    /// motor de bobina leen el contenedor por aquí, así que cualquier vídeo de
    /// móvil salía tumbado en la preview **y en el máster** (PENDIENTE §1.5).
    /// No daba la cara porque el material de prueba es de una Insta360, que no
    /// lleva rotación.
    pub cuartos: u8,
    /// en orden de DECODE (el orden del fichero)
    pub muestras: Vec<Muestra>,
    /// índices de `muestras` ordenados por pts (el orden de PANTALLA)
    pub orden_pts: Vec<u32>,
    /// el mapa AL REVÉS: de índice de decodificación a posición de pantalla.
    /// Buscarlo con `position()` en cada salto era O(n) —y en un máster de
    /// veinte mil muestras, un salto por corte—, pero sobre todo invitaba a
    /// mezclar los dos órdenes, que es de donde salía el tirón en las juntas.
    pub pos_pantalla: Vec<u32>,
}

impl Indice {
    pub fn abre(ruta: &Path) -> Result<Indice> {
        let mut f = File::open(ruta).with_context(|| format!("abrir {}", ruta.display()))?;
        let total = f.metadata()?.len();
        // localizar el moov saltando de caja en caja (el mdat se salta entero)
        let mut off = 0u64;
        let mut moov: Option<Vec<u8>> = None;
        let mut hdr = [0u8; 16];
        while off + 8 <= total {
            f.seek(SeekFrom::Start(off))?;
            f.read_exact(&mut hdr[..8])?;
            let mut n = u32::from_be_bytes(hdr[..4].try_into().unwrap()) as u64;
            let typ: [u8; 4] = hdr[4..8].try_into().unwrap();
            let mut cuerpo = off + 8;
            if n == 1 {
                f.read_exact(&mut hdr[8..16])?;
                n = u64::from_be_bytes(hdr[8..16].try_into().unwrap());
                cuerpo = off + 16;
            } else if n == 0 {
                n = total - off; // hasta el final
            }
            if &typ == b"moov" {
                let mut b = vec![0u8; (n - (cuerpo - off)) as usize];
                f.seek(SeekFrom::Start(cuerpo))?;
                f.read_exact(&mut b)?;
                moov = Some(b);
                break;
            }
            if n < 8 { bail!("caja corrupta en {off}"); }
            off += n;
        }
        let moov = moov.context("sin moov")?;
        Self::desde_moov(&moov)
    }

    fn desde_moov(moov: &[u8]) -> Result<Indice> {
        // el primer trak de vídeo
        for (typ, cuerpo) in cajas(moov) {
            if typ != *b"trak" { continue; }
            if let Some(ind) = trak_video(cuerpo)? { return Ok(ind); }
        }
        bail!("sin pista de vídeo MP4");
    }

    /// índice (en orden decode) de la muestra cuyo pts cubre `t` segundos
    pub fn muestra_en(&self, t: f64) -> usize {
        let objetivo = (t.max(0.0) * self.timescale as f64) as i64;
        // búsqueda binaria sobre el orden de pantalla
        let k = self.orden_pts.partition_point(|&i| self.muestras[i as usize].pts <= objetivo);
        let vis = k.saturating_sub(1).min(self.orden_pts.len().saturating_sub(1));
        self.orden_pts.get(vis).map(|&i| i as usize).unwrap_or(0)
    }

    /// keyframe (orden decode) desde el que hay que decodificar para llegar a `i`
    pub fn keyframe_para(&self, i: usize) -> usize {
        let mut k = i.min(self.muestras.len().saturating_sub(1));
        while k > 0 && !self.muestras[k].key { k -= 1; }
        k
    }

    pub fn pts_s(&self, i: usize) -> f64 {
        self.muestras.get(i).map(|m| m.pts as f64 / self.timescale as f64).unwrap_or(0.0)
    }

    /// en qué posición de PANTALLA cae una muestra (por su índice de decode)
    pub fn pantalla_de(&self, i: usize) -> usize {
        self.pos_pantalla.get(i).map(|&x| x as usize).unwrap_or(0)
    }

    /// las dimensiones **como se ven**: con la rotación del contenedor puesta
    pub fn visible(&self) -> (u32, u32) {
        if self.cuartos % 2 == 1 { (self.h, self.w) } else { (self.w, self.h) }
    }
}

/// sondeo nativo (w, h, fps, dur) — sustituye a ffprobe para MP4/MOV.
/// Las dimensiones son las **visibles**: un vídeo de móvil grabado de lado se
/// declara 1920×1080 y se ve 1080×1920, y eso es lo que quiere saber quien
/// dibuja una miniatura o calcula la proporción de la bobina.
pub fn sondea(ruta: &Path) -> Result<(u32, u32, f64, f64)> {
    let i = Indice::abre(ruta)?;
    let (w, h) = i.visible();
    Ok((w, h, i.fps, i.dur))
}

/// sondeo con la orientación: (ancho y alto GUARDADOS, fps, dur, cuartos).
/// Es lo que necesita el encuadre, que conforma con las dimensiones del
/// fichero y aplica los cuartos de vuelta él mismo.
pub fn sondea_orientado(ruta: &Path) -> Result<(u32, u32, f64, f64, u8)> {
    let i = Indice::abre(ruta)?;
    Ok((i.w, i.h, i.fps, i.dur, i.cuartos))
}

/// itera las cajas hijas de un cuerpo
fn cajas(b: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut off = 0usize;
    std::iter::from_fn(move || {
        if off + 8 > b.len() { return None; }
        let n = u32::from_be_bytes(b[off..off + 4].try_into().unwrap()) as usize;
        let typ: [u8; 4] = b[off + 4..off + 8].try_into().unwrap();
        if n < 8 || off + n > b.len() { return None; }
        let cuerpo = &b[off + 8..off + n];
        off += n;
        Some((typ, cuerpo))
    })
}

fn caja<'a>(b: &'a [u8], camino: &[&[u8; 4]]) -> Option<&'a [u8]> {
    let mut cur = b;
    for objetivo in camino {
        cur = cajas(cur).find(|(t, _)| t == *objetivo)?.1;
    }
    Some(cur)
}

fn u16at(b: &[u8], o: usize) -> u32 { u16::from_be_bytes(b[o..o + 2].try_into().unwrap()) as u32 }
fn u32at(b: &[u8], o: usize) -> u32 { u32::from_be_bytes(b[o..o + 4].try_into().unwrap()) }
fn u64at(b: &[u8], o: usize) -> u64 { u64::from_be_bytes(b[o..o + 8].try_into().unwrap()) }

/// LOS CUARTOS DE VUELTA que declara el `tkhd`.
///
/// Son 36 bytes en punto fijo 16.16 (los dos últimos de cada fila, en 2.30) y
/// el índice ya está dentro del `moov`, así que leerlo no cuesta nada. La
/// matriz lleva coordenadas guardadas a coordenadas de pantalla:
/// `(x', y') = (a·x + c·y + tx, b·x + d·y + ty)`. Solo interesan los cuatro
/// primeros, y solo los cuatro giros rectos: un ángulo cualquiera en el
/// contenedor no lo respeta nadie.
fn cuartos_del_tkhd(trak: &[u8]) -> u8 {
    let Some(tkhd) = caja(trak, &[b"tkhd"]) else { return 0 };
    let base = if tkhd.first() == Some(&1) { 52 } else { 40 };
    if tkhd.len() < base + 36 { return 0; }
    let fijo = |o: usize| -> f32 {
        i32::from_be_bytes(tkhd[base + o..base + o + 4].try_into().unwrap()) as f32 / 65536.0
    };
    let (a, b, c, d) = (fijo(0), fijo(4), fijo(12), fijo(16));
    let uno = |v: f32, q: f32| (v - q).abs() < 0.01;
    if uno(a, 0.0) && uno(b, 1.0) && uno(c, -1.0) && uno(d, 0.0) { return 1; }
    if uno(a, -1.0) && uno(b, 0.0) && uno(c, 0.0) && uno(d, -1.0) { return 2; }
    if uno(a, 0.0) && uno(b, -1.0) && uno(c, 1.0) && uno(d, 0.0) { return 3; }
    0
}

fn trak_video(trak: &[u8]) -> Result<Option<Indice>> {
    let Some(mdia) = caja(trak, &[b"mdia"]) else { return Ok(None) };
    let Some(hdlr) = caja(mdia, &[b"hdlr"]) else { return Ok(None) };
    if hdlr.len() < 12 || &hdlr[8..12] != b"vide" { return Ok(None); }
    let mdhd = caja(mdia, &[b"mdhd"]).context("sin mdhd")?;
    let (timescale, dur_ts) = if mdhd[0] == 1 {
        (u32at(mdhd, 20), u64at(mdhd, 24))
    } else {
        (u32at(mdhd, 12), u32at(mdhd, 16) as u64)
    };
    let stbl = caja(mdia, &[b"minf", b"stbl"]).context("sin stbl")?;
    let stsd = caja(stbl, &[b"stsd"]).context("sin stsd")?;

    // primera entrada del stsd: avc1 / hvc1 / hev1
    let entrada = cajas(&stsd[8..]).next().context("stsd vacío")?;
    let (etyp, ecuerpo) = entrada;
    let w = u16at(ecuerpo, 24);
    let h = u16at(ecuerpo, 26);
    // las cajas hijas del sample entry empiezan tras los 78 bytes fijos
    let hijas = &ecuerpo[78.min(ecuerpo.len())..];
    let codec = match &etyp {
        b"avc1" | b"avc3" => {
            let c = cajas(hijas).find(|(t, _)| t == b"avcC").context("sin avcC")?.1;
            parse_avcc(c)?
        }
        b"hvc1" | b"hev1" => {
            let c = cajas(hijas).find(|(t, _)| t == b"hvcC").context("sin hvcC")?.1;
            parse_hvcc(c)?
        }
        _ => bail!("códec MP4 no soportado: {}", String::from_utf8_lossy(&etyp)),
    };

    // ── stts: deltas de dts ──
    let stts = caja(stbl, &[b"stts"]).context("sin stts")?;
    let n_stts = u32at(stts, 4) as usize;
    let mut deltas: Vec<(u32, u32)> = Vec::with_capacity(n_stts); // (count, delta)
    for i in 0..n_stts {
        deltas.push((u32at(stts, 8 + i * 8), u32at(stts, 12 + i * 8)));
    }
    // ── stsz: tamaños ──
    let stsz = caja(stbl, &[b"stsz"]).context("sin stsz")?;
    let tam_fijo = u32at(stsz, 4);
    let n = u32at(stsz, 8) as usize;
    let tam = |i: usize| -> u32 {
        if tam_fijo != 0 { tam_fijo } else { u32at(stsz, 12 + i * 4) }
    };
    // ── stco/co64 + stsc: offsets por chunk ──
    let (chunk_offs, co64): (Vec<u64>, bool) = if let Some(co) = caja(stbl, &[b"co64"]) {
        let m = u32at(co, 4) as usize;
        ((0..m).map(|i| u64at(co, 8 + i * 8)).collect(), true)
    } else {
        let co = caja(stbl, &[b"stco"]).context("sin stco")?;
        let m = u32at(co, 4) as usize;
        ((0..m).map(|i| u32at(co, 8 + i * 4) as u64).collect(), false)
    };
    let _ = co64;
    let stsc = caja(stbl, &[b"stsc"]).context("sin stsc")?;
    let n_stsc = u32at(stsc, 4) as usize;
    let mut stsc_e: Vec<(u32, u32)> = Vec::with_capacity(n_stsc); // (first_chunk 1-based, samples/chunk)
    for i in 0..n_stsc {
        stsc_e.push((u32at(stsc, 8 + i * 12), u32at(stsc, 12 + i * 12)));
    }
    // ── stss: keyframes ──
    let claves: Option<Vec<u32>> = caja(stbl, &[b"stss"]).map(|s| {
        let m = u32at(s, 4) as usize;
        (0..m).map(|i| u32at(s, 8 + i * 4)).collect()
    });
    // ── ctts: offset de composición ──
    let ctts: Option<Vec<(u32, i64)>> = caja(stbl, &[b"ctts"]).map(|c| {
        let m = u32at(c, 4) as usize;
        (0..m).map(|i| {
            let cnt = u32at(c, 8 + i * 8);
            let raw = u32at(c, 12 + i * 8);
            // versión 1 lleva signed; versión 0 en la práctica también aparece negativo
            let v = if c[0] == 1 { raw as i32 as i64 } else { raw as i64 };
            (cnt, v)
        }).collect()
    });

    // ── montar las muestras ──
    let mut muestras: Vec<Muestra> = Vec::with_capacity(n);
    let mut dts: i64 = 0;
    let mut it_delta = deltas.iter().flat_map(|&(c, d)| std::iter::repeat(d).take(c as usize));
    let mut it_ctts = ctts.as_ref().map(|v| {
        v.iter().flat_map(|&(c, o)| std::iter::repeat(o).take(c as usize)).collect::<Vec<_>>()
    });
    let es_clave = |i: usize| -> bool {
        match &claves {
            None => true, // sin stss = todo son sync samples (all-intra)
            Some(v) => v.binary_search(&((i + 1) as u32)).is_ok(),
        }
    };
    // recorrer chunks según stsc
    let mut idx = 0usize;
    'fuera: for (e, &(primero, por_chunk)) in stsc_e.iter().enumerate() {
        let ultimo = stsc_e.get(e + 1).map(|x| x.0 - 1).unwrap_or(chunk_offs.len() as u32);
        for ch in primero..=ultimo {
            let Some(&base) = chunk_offs.get((ch - 1) as usize) else { break 'fuera };
            let mut off = base;
            for _ in 0..por_chunk {
                if idx >= n { break 'fuera; }
                let t = tam(idx);
                let d = it_delta.next().unwrap_or(0) as i64;
                let comp = it_ctts.as_mut().and_then(|v| v.get(idx).copied()).unwrap_or(0);
                muestras.push(Muestra { off, tam: t, pts: dts + comp, key: es_clave(idx) });
                off += t as u64;
                dts += d;
                idx += 1;
            }
        }
    }
    if muestras.is_empty() { bail!("pista sin muestras"); }
    // normalizar: el primer pts de pantalla = 0
    let min_pts = muestras.iter().map(|m| m.pts).min().unwrap_or(0);
    for m in &mut muestras { m.pts -= min_pts; }
    let mut orden_pts: Vec<u32> = (0..muestras.len() as u32).collect();
    orden_pts.sort_by_key(|&i| muestras[i as usize].pts);
    let mut pos_pantalla = vec![0u32; muestras.len()];
    for (pos, &i) in orden_pts.iter().enumerate() { pos_pantalla[i as usize] = pos as u32; }

    // fps: el delta dominante del stts
    let delta_dom = deltas.iter().max_by_key(|&&(c, _)| c).map(|&(_, d)| d).unwrap_or(0);
    let fps = if delta_dom > 0 { timescale as f64 / delta_dom as f64 } else { 25.0 };
    let dur = if dur_ts > 0 { dur_ts as f64 / timescale as f64 }
              else { dts as f64 / timescale as f64 };

    Ok(Some(Indice { w, h, fps, dur, timescale, codec,
                     cuartos: cuartos_del_tkhd(trak), muestras, orden_pts, pos_pantalla }))
}

#[cfg(test)]
mod pruebas_tkhd {
    use super::cuartos_del_tkhd;

    /// un `trak` de mentira con un `tkhd` v0 y la matriz que se le pida
    fn trak(matriz: [f32; 4]) -> Vec<u8> {
        let mut cuerpo = vec![0u8; 84];
        cuerpo[0] = 0; // versión 0
        let mut pon = |o: usize, v: f32| {
            let n = (v * 65536.0) as i32;
            cuerpo[o..o + 4].copy_from_slice(&n.to_be_bytes());
        };
        pon(40, matriz[0]);        // a
        pon(44, matriz[1]);        // b
        pon(52, matriz[2]);        // c
        pon(56, matriz[3]);        // d
        let mut caja = Vec::new();
        caja.extend_from_slice(&((cuerpo.len() + 8) as u32).to_be_bytes());
        caja.extend_from_slice(b"tkhd");
        caja.extend_from_slice(&cuerpo);
        caja
    }

    #[test]
    fn lee_los_cuatro_giros_rectos() {
        assert_eq!(cuartos_del_tkhd(&trak([1.0, 0.0, 0.0, 1.0])), 0);
        assert_eq!(cuartos_del_tkhd(&trak([0.0, 1.0, -1.0, 0.0])), 1);
        assert_eq!(cuartos_del_tkhd(&trak([-1.0, 0.0, 0.0, -1.0])), 2);
        assert_eq!(cuartos_del_tkhd(&trak([0.0, -1.0, 1.0, 0.0])), 3);
    }

    #[test]
    fn sin_tkhd_no_hay_giro() {
        assert_eq!(cuartos_del_tkhd(&[]), 0);
        assert_eq!(cuartos_del_tkhd(&[0, 0, 0, 8, b'f', b'r', b'e', b'e']), 0);
    }
}

fn parse_avcc(c: &[u8]) -> Result<Codec> {
    anyhow::ensure!(c.len() > 8, "avcC corto");
    let nal_len = (c[4] & 0x3) + 1;
    let n_sps = (c[5] & 0x1f) as usize;
    let mut o = 6usize;
    let mut sps = Vec::new();
    for k in 0..n_sps {
        let l = u16at(c, o) as usize;
        if k == 0 { sps = c[o + 2..o + 2 + l].to_vec(); }
        o += 2 + l;
    }
    let n_pps = c[o] as usize;
    o += 1;
    let mut pps = Vec::new();
    for k in 0..n_pps {
        let l = u16at(c, o) as usize;
        if k == 0 { pps = c[o + 2..o + 2 + l].to_vec(); }
        o += 2 + l;
    }
    anyhow::ensure!(!sps.is_empty() && !pps.is_empty(), "avcC sin SPS/PPS");
    Ok(Codec::H264 { sps, pps, nal_len })
}

fn parse_hvcc(c: &[u8]) -> Result<Codec> {
    anyhow::ensure!(c.len() > 23, "hvcC corto");
    let nal_len = (c[21] & 0x3) + 1;
    let n_arrays = c[22] as usize;
    let (mut vps, mut sps, mut pps) = (Vec::new(), Vec::new(), Vec::new());
    let mut o = 23usize;
    for _ in 0..n_arrays {
        let nal_type = c[o] & 0x3f;
        let cnt = u16at(c, o + 1) as usize;
        o += 3;
        for k in 0..cnt {
            let l = u16at(c, o) as usize;
            let d = c[o + 2..o + 2 + l].to_vec();
            if k == 0 {
                match nal_type { 32 => vps = d, 33 => sps = d, 34 => pps = d, _ => {} }
            }
            o += 2 + l;
        }
    }
    anyhow::ensure!(!vps.is_empty() && !sps.is_empty() && !pps.is_empty(), "hvcC sin VPS/SPS/PPS");
    Ok(Codec::Hevc { vps, sps, pps, nal_len })
}
