# VELOCIDAD — cómo el editor nativo se volvió instantáneo

> Informe técnico del trabajo de 2026-07-31 (Hito 0 + rondas 2–4). Documenta
> QUÉ se construyó, POR QUÉ cada decisión, los NÚMEROS medidos y las TRAMPAS
> encontradas. Compañero de `TECHNICAL_REPORT.md` (el motor de render) y de
> `TRASPASO.md` (la historia y el estado). Todo lo de aquí está verificado
> con la app real, GPU real y cronómetros (`FL_CRONO=1`), en las dos
> máquinas: MacBook M4 Max (Metal) y GPD Win Max 2 · Radeon 890M (Vulkan).

---

## 0. El punto de partida y el resultado

El visor nativo original lanzaba **un proceso ffmpeg por cada apertura o
seek** y leía fotogramas 4K crudos (12 MB) por una tubería. Resultado:
~5 segundos desde pulsar espacio hasta ver movimiento, y scrub imposible.

| Operación | Antes | Ahora (Mac) | Ahora (GPD) |
|---|---|---|---|
| Espacio → imagen en movimiento | ~5000 ms | **2–4 ms** (caché) | **2–16 ms** |
| Scrub, por fotograma, CON proxy | imposible | 0,7–2,8 ms | ~2 ms |
| Scrub, por fotograma, SIN proxy (máster 4K HEVC) | imposible | **5–19 ms** | 15–60 ms |
| Pausa → máster 4K a resolución completa | — | ~80–260 ms (asíncrono) | ~100–400 ms |
| Secuencia de decode, proxy / máster | — | 1650 fps / 225 fps | ~large / 59+ fps |
| Miniaturas de bobina y latas | no existían | 1–3 ms/fotograma | keyframes ≈ gratis |

La regla del proyecto: **cualquier espera perceptible (>100 ms) es un fallo**.
Todo lo que sigue existe para cumplirla — con y sin proxies.

---

## 1. Los cuatro principios

1. **Cero procesos en el camino interactivo.** Nada de ffmpeg/ffprobe entre
   el gesto y el píxel. Decode por hardware EN PROCESO: VideoToolbox en Mac,
   Media Foundation en Windows.
2. **Una copia por fotograma es el contrato universal** (GENERALIZACION.md).
   El zero-copy es un fast path futuro; la frontera (planos YUV listos para
   `write_texture`) no cambia. Y esa única copia se aprovecha: decima,
   convierte profundidad y desentrelaza croma en la misma pasada.
3. **La última orden gana; el trabajo de fondo NUNCA compite.** Un seek
   nuevo aborta lo que hubiera. El precalentado y los refinados jamás roban
   el turno a un gesto del usuario.
4. **El fotograma correcto ya está en pantalla.** Las políticas (scrub
   síncrono, caché del último frame, refinado diferido) están diseñadas para
   que en el momento del gesto no haya que esperar a nadie.

---

## 2. Arquitectura de hilos

```
hilo de eventos (winit)                      GPU (wgpu)
│  escrutinio: decoders de SCRUB             cadena fílmica completa
│  (frame síncrono en el propio evento)      (grade→shutter→pirámide→comp)
│
├── cabina (hilo): reproducción + refinados + precalentado
│     órdenes por canal, «la última real gana», cola con contrapresión (3)
├── miniaturas (hilo): keyframes → RGBA → atlas 2048² con LRU
└── sonido (hilo + stream cpal): AAC del máster → anillo → altavoz
```

- **Escrutinio** (`visor::busca_sincrono`): el proxy all-intra (o el máster
  vía keyframe) decodifica en 1–19 ms, así que el fotograma del seek se
  decodifica y sube a textura **dentro del propio evento de ratón**: la
  aguja y la imagen se pintan en el MISMO frame presentado. Coherencia
  perfecta — no hay "la aguja va por delante de la imagen".
- **Cabina** (`nativa/src/cabina.rs`): órdenes `Frame` (un fotograma),
  `Toca` (secuencia [t0,t1)), `Precalienta` (lista de fondo). Entre
  fotograma y fotograma de una secuencia se mira si hay una orden más nueva
  y se abandona. La salida es un `sync_channel(3)`: contrapresión natural
  (el decoder nunca corre más de 3 frames por delante del visor).
- **Miniaturas** (`nativa/src/miniaturas.rs`): peticiones por clave
  `(cinta, t)`; falladas → reintento con enfriamiento de 5 s. Nada bloquea.
- **Sonido** (`nativa/src/sonido.rs`): symphonia (isomp4+aac) decodifica el
  audio DEL MÁSTER desde el t exacto; anillo de ~1 s; cpal lo vacía.
  Cada orden nueva resincroniza.

---

## 3. El índice del contenedor (`core/src/indice.rs`)

Parser MP4/MOV propio (~300 líneas, cero dependencias): localiza el `moov`
saltando cajas (el `mdat` de 1 GB se salta entero — en el máster de
referencia el moov son **19 KB en la cola**), y construye por muestra:
`{offset, tamaño, pts, keyframe}` desde `stts/ctts/stss/stsc/stsz/stco/co64`.

- **Seek O(log n)**: búsqueda binaria por pts sobre el orden de pantalla →
  muestra objetivo → keyframe anterior por `stss`. Sin parsear nada más.
- Las muestras del `mdat` ya están en **AVCC** (longitud + payload): se
  alimentan al decoder TAL CUAL, sin reempaquetar ni Annex-B.
- `indice::sondea()` (w, h, fps, duración) sustituye a ffprobe en TODOS los
  caminos interactivos: la estantería ya no lanza un proceso por cinta.
- fps = delta dominante del `stts`; pts normalizados a primer-pts-0;
  reordenación B-frames resuelta con el orden de pantalla precalculado.

## 4. El proyector en proceso (`core/src/cine.rs`)

Un solo tipo, `Cine`, con tres backends tras la misma API
(`frame_en`, `frame_clave`, `frame_scrub`, `arranca_en`, `siguiente`):

### 4.1 macOS — VideoToolbox (FFI C pura, sin crates ObjC)

- `CMVideoFormatDescriptionCreateFromHEVCParameterSets` /
  `...H264ParameterSets` a partir del hvcC/avcC del índice.
- Sesión **síncrona** (flag 0): el callback dispara antes de volver, el
  orden es determinista y no hay colas fantasma.
- El pts real NO viene en el callback (el CMSampleBuffer va sin timing):
  viaja como `source_frame_refcon` (el índice de muestra).
- Se pide EXPLÍCITAMENTE `x420` (10-bit biplanar) o `420v` (NV12): sin eso
  VT emite `p420` empaquetado y la importación lee basura (trampa heredada
  del motor de render).
- Salida: `CVPixelBufferLockBaseAddress` → copia de planos.

### 4.2 Windows — Media Foundation

- `IMFSourceReader` + `MFCreateDXGIDeviceManager` sobre un dispositivo
  D3D11 propio (`D3D11_CREATE_DEVICE_VIDEO_SUPPORT` + multithread
  protected): el decode ocurre en la VCN por hardware; la lectura a CPU la
  hace MF al bloquear el buffer (`Lock2DSize`, con pitch real y altura
  empadronada del buffer para encontrar el plano UV).
- Seek: `SetCurrentPosition` (aterriza en el keyframe anterior) + descarte
  hasta el objetivo — mismo esquema que VT.
- P010 si el stream es 10-bit; NV12 universal si no.
- `CoInitializeEx(MTA)` al abrir: el Source Resolver usa COM y los hilos de
  trabajo no lo traen inicializado — sin esto, abrir/leer FALLA EN SILENCIO
  fuera del hilo principal (así murieron las miniaturas y el vídeo en la
  primera versión de Windows).

### 4.3 La conversión: una copia que hace cuatro trabajos

`a_planos`/`convierte` recorre el pixel buffer UNA vez y en esa pasada:
1. copia (quita el stride/padding),
2. desentrelaza el croma NV12/P010 → planos U y V,
3. normaliza profundidad a los códigos de 10 bits que espera la cadena
   (`yuv_norm=1023`): P010 MSB-aligned `>>6`, 8-bit `<<2`,
4. **decima 2× si `mitad`** (fuentes >2200 px): la preview no necesita 4K
   — 4× menos bytes que mover y subir. El refinado en pausa va SIN decimar.

---

## 5. Las políticas del instante

### 5.1 Scrub: exacto si es barato, keyframe si no (`frame_scrub`)

Con proxy all-intra, todo fotograma es keyframe: exacto siempre, 1–3 ms.
Sin proxy, decodificar el frame exacto puede costar un GOP entero
(60–120 fotogramas 4K). Política:

- **catch-up corto** (mismo GOP y ≤10 muestras en Mac; lectura hacia
  delante <0,25 s en Windows) → frame EXACTO;
- **salto grande** → el FOTOGRAMA CLAVE anterior (`frame_clave`): un solo
  decode, 5–19 ms. El frame exacto, a resolución completa, llega solo con
  el refinado cuando la aguja se asienta.

Es la política de los NLE profesionales (keyframes al arrastrar, exacto al
soltar) — pero con el refinado automático no hay que soltar nada.

### 5.2 El refinado a máster (y su debounce)

En pausa, tras un asentamiento, la cabina decodifica el frame EXACTO del
máster SIN decimar y sustituye la imagen: la pausa siempre acaba en 4K
completo (se lee la matrícula del coche del clip de referencia).

- Debounce de **400 ms tras un scrub** (un máster a destiempo bloquea la
  cabina y le roba el instante al arrastre — medido: picos de 75–330 ms en
  mitad del scrub antes del debounce).
- Debounce de **~80 ms tras una pausa** (no hay arrastre del que
  protegerse; el 4K llega casi al momento).

### 5.3 La caché del último fotograma (pausa→play gratis)

`Cine` recuerda el último fotograma servido por un seek. Si se pide el
mismo punto y el decoder sigue posicionado justo después (mac:
`pos == i+1`; win: `pts == ultimo_pts`), se sirve el clon: **0 decodes**.
Efecto: pulsar espacio tras una pausa (el caso universal) arranca la
reproducción en **1,9–3,5 ms** — antes repagaba el GOP entero (120–423 ms).

### 5.4 Al pausar, NO pedir nada

La imagen en pantalla en el momento de pausar YA es el fotograma correcto
(el último que el ritmo pintó). La primera versión pedía un frame al
decoder de scrub — que estaba parado GOPs atrás — y enseñaba un keyframe
~1 s viejo hasta el refinado. Regla: pausar = congelar + refinar. Nada más.

### 5.5 Precalentado que jamás compite (`Orden::Precalienta`)

Al abrir un proyecto, la cabina calienta en los ratos muertos
índice+sesión+primer GOP de todos los proxies y másters (un ítem por
vuelta del bucle, mirando entre ítem e ítem si llegó una orden real).

**La trampa que rompió Windows**: el buzón «la última orden gana» trataba
`Precalienta` como una orden más — la `Frame`/`Toca` anterior en cola se
DESCARTABA. En Mac no se notaba (el scrub síncrono ya había pintado); en
Windows dejaba el vídeo estático con el audio sonando. El trabajo de fondo
va SIEMPRE a su propia lista, fuera del buzón.

### 5.6 El ritmo de reproducción

- El visor consume la cola por pts: en cada redraw pinta el último
  fotograma cuyo `pts ≤ src_t + ½ frame`.
- **El reloj espera al fotograma**: si la cola se seca >150 ms, el reloj se
  rebasa (la imagen nunca se queda atrás del tiempo — la regla anti-webview
  del TRASPASO).
- Junta entre clips: nueva generación + nueva `Toca` + audio resincronizado.
  Primer frame del clip entrante: 12–16 ms (sesión caliente por el
  precalentado).
- Cada orden lleva **generación**: un frame de una generación vieja se
  descarta al drenar. Los canales nunca se "purgan": se ignoran por gen.

---

## 6. Miniaturas instantáneas

- Hilo propio con sus decoders (jaula de 4) — nunca toca los del scrub ni
  los de la cabina.
- **Solo keyframes** (`frame_clave`): el truco Jellyfin — decodificar un
  keyframe es un decode sin catch-up, da igual que el máster pese 1 GB.
  Para proxies all-intra, keyframe == frame exacto.
- Conversión YUV(10-bit)→RGBA BT.709 limited + reescalado nearest a
  160×90, en el hilo.
- **Atlas 2048² RGBA** (264 huecos) con desalojo LRU; quads texturizados
  con su propio pipeline, pintados entre el lienzo y la tipografía (los
  fotogramas viven ENTRE las perforaciones de la tira, como en una película
  de verdad).
- La UI pide por clave `(cinta, t·100)` cada frame; lo que no está se
  encarga UNA vez; las claves falladas reintentan a los 5 s (sin esto, un
  fallo se quedaba atascado para siempre).

## 7. La interfaz que no estorba

- **Texto**: glyphon re-shapeaba TODA la interfaz cada frame (3,7 ms).
  Caché por (texto, tamaño): solo se shapea lo nuevo — el timecode. 0,3 ms.
- **`desired_maximum_frame_latency`**: con 1, `get_current_texture`
  serializa el bucle al vsync (~18 ms bloqueado, pierde 1 de cada 5
  frames). Con el scrub ya síncrono no aporta nada: **2 es lo correcto**.
- La cabina se drena también en `about_to_wait`, no solo en el redraw: un
  fotograma recién decodificado no espera una vuelta extra de vsync.
- Medidor de fps honesto (ventana rodante de fotogramas PINTADOS, no EMA).
- Trampa de contexto: la pantalla del Mac del autor está a **50 Hz** (modo
  PAL): el rótulo diciendo «50 fps» durante la reproducción es el techo
  físico del monitor, no la app (el GPD marca 58–60).

---

## 8. Trampas descubiertas (rondas 1–4, TODAS verificadas)

1. `CMBlockBufferCreateWithMemoryBlock` con blockAllocator NULL **libera él
   la memoria** → doble free con Vecs de Rust (SIGABRT). Pasar
   `kCFAllocatorNull`. El patrón late en `metal/src/decode_vt.rs` — no
   tocar sin medir.
2. La cadena espera códigos de 10 bits (`yuv_norm=1023`): P010 `>>6`,
   8-bit `<<2`. Con escala 16-bit todo sale blanco quemado.
3. El pts de VT viaja como refcon; el CMSampleBuffer va sin timing.
4. VT sin formato de salida explícito emite `p420` empaquetado (basura al
   importar como biplanar).
5. `CoInitializeEx(MTA)` obligatorio en hilos de trabajo con MF: sin él,
   el Source Resolver falla EN SILENCIO.
6. El buzón «última orden gana» no puede incluir el trabajo de fondo
   (la carrera del §5.5 — el bug de Windows).
7. `frame_latency=1` serializa el bucle al vsync (§7).
8. Un refinado a máster sin debounce roba el instante al scrub (§5.2).
9. Los proxies VIEJOS del webview cuelgan el SourceReader de MF: hay que
   regenerarlos con la receta nueva o borrarlos.
10. En el GPD por ssh: el `dir` de cmd miente sobre directorios que
    PowerShell sí ve; `taskkill` no siempre suelta el exe a la primera; la
    GUI no arranca en la sesión ssh (device lost) — `schtasks /it`.
11. glyphon `Buffer` no es clonable a la ligera: cachear por clave y
    referenciar, no reconstruir.

## 9. Los límites actuales y las siguientes palancas

- **Suelo de latencia visual**: ~1,5–2 vsyncs (el pipeline de
  presentación). A 120 Hz (ProMotion) baja solo. Posible: presentar fuera
  de vsync solo durante el scrub (tearing aceptable en arrastre).
- **Junta en frío sin proxy**: 12–16 ms hoy (precalentado). El pre-roll
  con segundo decoder (GENERALIZACION, pendiente) lo dejaría en 0.
- **Zero-copy real** (IOSurface→wgpu-hal / shared handle D3D): eliminaría
  la única copia por frame. Solo tiene sentido si la copia aparece en el
  perfil — hoy no es el cuello.
- **Decode GOP-paralelo** para el refinado y el export (TECHNICAL_REPORT
  §10).
- El camino ffmpeg queda SOLO como fallback de contenedores no-BMFF
  (mkv/webm) y plataformas sin backend nativo (Linux, hasta VAAPI).


---

## Addendum (1-ago): la receta del cuarto oscuro, cacheada

Medido con `FL_CRONO=1` al implementar la lanzadera J/K/L: cada
`Visor::busca()` marcaba `cuarto_pendiente`, y la cadena RE-SUBÍA las dos
LUTs 3D y recompilaba el grade **en cada fotograma**: `cadena 750 ms`.
En reproducción normal no se notaba (la cadena solo se toca al cambiar de
clip), pero cualquier gesto que buscara fotograma a fotograma (lanzadera,
scrub sostenido, rueda de la manivela) caía a **1,3 Hz**.

Arreglo: `Visor.receta_puesta` guarda la HUELLA de lo que ya está en la
GPU (`prefs.to_string()`, las dos rutas de LUT) y `aplica_cuarto` solo
corre si la huella cambia. Resultado en el mismo material (máster 4K sin
proxy): **cadena 0,8 ms · redraw 50 Hz** durante la lanzadera — 900×.

La regla que deja: *lo que se sube a la GPU se recuerda*. Marcar algo
como pendiente es barato; recompilarlo, no.
