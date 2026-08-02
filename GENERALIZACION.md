# GENERALIZACIÓN — Laboratorios Saorín

> Checklist viva. Origen: investigación del 2026-07-31 (cuatro barridos: UI del
> studio, motor/CLI, listón Shotcut/Kdenlive, portabilidad de hardware).
> Alcance acordado: **una pista de vídeo + una de audio**.
> **2026-07-31 tarde: gran sprint de implementación** — P0 completo, audio
> completo (reproducción+pista+mezcla), proyecto/aspectos/rejilla/encuadre,
> herramientas de edición, export incremental con caché por hash, multi-
> proyecto, ProRes máster, cancelar/ETA/caffeinate, media offline, chuleta.
> Invariante verificado tras el sprint: **413.2 fps e2e** en el clip de
> referencia (ráfaga 1920 frames, vt-submit 0.01 ms). OJO: con el disco al
> 96% el clip completo baja a ~150 fps por I/O sostenida — no es el motor.
> **Sprint 2 (misma tarde)**: fotos fijas, huecos + lift/extract, velocidad
> por clip (setpts+atempo), banda elástica de volumen, loudnorm, slip,
> multi-selección, rango I/O con export parcial, tier compat (lut3d por
> software: revela SIN motor nativo), HDR→709 (tonemap) y full-range→tv,
> relink recursivo, drag&drop (upload), LUTs de usuario, backups rotatorios
> + validación + versión de formato, prefetch de decoder en juntas, loop,
> zoom-fit, lupa fina.
> **Sprint 4 — instantaneidad y ampliadora nativa**: decodificador por ÍNDICE
> con Range (máster de 1 GB abierto en 33 ms / 2.1 MB — mp4box nextParsePosition
> salta el mdat y aterriza en el moov de la cola; los GOP se piden bajo demanda);
> servidor multi-hilo (8 obreros); miniaturas/ondas desde el PROXY; caché HTTP
> de derivados; proyección por proxy con catch-up por seek y pintado
> secuenciado; piezas en paralelo (Mac 3 / Win 2 — medido en la 890M: 82 s → 45 s);
> sidecars eager al importar. **Ampliadora NATIVA** (`core/src/bin/preview.rs
> --ipc`): ventana wgpu propia a resolución COMPLETA alimentada por stdin desde
> `/api/preview` — rompe el techo de WebGL; verificada visualmente con el grade
> real. **Drag&drop nativo** por `on_webview_event`/DragDropEvent → registro POR
> REFERENCIA (la webview no ve los eventos HTML5) + `/api/media-version` para
> que la estantería se refresque sola. Diálogo de importación: se fuerza al
> frente y avisa si no entra nada. Loopback medido: 1.5 GB/s — el servidor NO
> es el cuello.
> Deliberadamente pendiente: i18n (decisión de diseño:
> la metáfora), preview nativa bajo webview (necesita ventana real), cola de
> renders, vúmetros, atajos editables, grade por clip, J/L cuts, piezas en
> paralelo, probe OBS-style, ducking, títulos (pendiente de decidir tipografía
> del rótulo — la casa dibuja con Pillow/canvas, no drawtext).
> **Sprint 3 (misma tarde) — EL BUG GRAVE**: `ClipDecoder.open()` descargaba
> el máster ENTERO (1 GB, moov en la cola de los MP4 de cámara) bloqueando la
> vista con "enhebrando…" — y la manivela con él. Nueva política: NADA
> bloquea jamás — póster (miniatura) al instante → proxy como caballo de
> batalla (scrub/reproducción/manivela) → máster enhebrado en segundo plano
> solo para el frame exacto en pausa. Proxies con +faststart. Además:
> pantalla de bienvenida (portada con bobinas), insertar-en-la-aguja desde la
> fuente (⏎; ⇧⏎ = al final), contador de proxies. Desplegado en Mac y GPD.

## Invariantes (no negociables)

- **El M4 Max y la 890M no pueden bajar**: clip de referencia → ≥400 fps e2e en
  el M4 Max (ProRes 422 HQ), ≥85 fps en la 890M. Convertir en tests de
  regresión de rendimiento; cualquier capa de generalización que los rompa se
  revierte.
- **Preview = export** (mismo motor y mismos parámetros de look). El drift
  WYSIWYG es el pecado mortal de Kdenlive; nuestro diseño de motor único lo
  previene — mantenerlo.
- **Zero-copy es un fast path negociado en el probe; una copia por frame es el
  contrato mínimo universal.** La abstracción vive en la frontera del frame
  (handles de textura GPU), jamás dentro del bucle de píxeles.
- **Über-shader**: los tiers de hardware cambian parámetros (divisor de
  resolución, profundidad de pirámide, LUT horneado), nunca la arquitectura de
  passes. Nada de graph de efectos genérico estilo MLT.

---

## P0 — Bugs y arreglos inmediatos

### Texto seleccionable (el bug señalado)

- [x] `studio/css/lab.css:49` — el `* { user-select: none }` global no lleva
      `-webkit-user-select` (en WKWebView < Safari 17 es un no-op → TODO
      seleccionable). Añadir el prefijo y una allowlist:
      `input, textarea, [contenteditable] { user-select: text }` +
      `#render-log, #rev-step, #contacto .c-datos, #tc, .colofon`.
      Ojo: el input del nombre de bobina (`studio/index.html:169`) hoy queda
      bloqueado por el blanket `none` en WebKit.
- [x] `img { -webkit-user-drag: none; }` + `draggable="false"` — las imágenes
      de `.lata .tira-asoma` (mesa.js:82-86), `.colgado`, `#foto-lab` inician
      drags nativos que roban el gesto de click/arrastre.
- [x] `body { -webkit-tap-highlight-color: transparent; }`.
- [x] `#timeline` (`lab.css:307`): `touch-action: none` (los gestos de
      trackpad pueden cancelar un drag).
- [x] Cursor `grabbing` mientras se arrastra un clip (hoy solo en la manivela).

### Bugs funcionales encontrados en el barrido

- [x] **La GUI descarta el `fade`**: `studio/js/revelado.js:26-32` mapea
      `{file,in,out}` y tira el campo — los fundidos que el motor sí soporta
      son inalcanzables desde la app (solo por `saorin cli render`).
- [x] **Salida Mac clavada a 3840×2160 con estirado**: `metal/src/main.rs:123`
      crea el encoder a 4K fijo y los pack shaders reescalan con UVs
      normalizadas — 1080p se upscala, 9:16 se espachurra. La salida debe ser
      = resolución del proyecto, con letterbox/fit, nunca stretch.
- [x] **Éxito silencioso con 0 frames**: `metal/src/main.rs:373` hace
      `exit(0)` incondicional; si VT no decodifica nada (perfil no soportado,
      stream corrupto) sale 0 con un fichero degenerado y el shell lo concatena.
- [x] **Audio sobrevive al vídeo con `--max-frames`** en el camino HEVC (falta
      `-shortest`; `audio_limit_s` solo lo honra el pump de ProRes).
- [x] **Timescale `fps as i32`** (59.94 → 59) en las CMTime del camino
      ProRes/AVAssetWriter (`metal/src/main.rs:142`).
- [x] **Temporales sin limpiar**: `cut_*.mp4`/`piece_*.mp4` en `.tmp/` no se
      borran en el shell Rust (el server.py sí lo hacía).
- [x] **Undo gasta slot en clicks vacíos**: `snapshot()` en pointerdown aunque
      el gesto no cambie nada (`timeline.js:171-181`).
- [x] **Colisión de ids**: el split usa `Date.now()%1e9` (`timeline.js:109`)
      contra el contador `nextId` del resto (`state.js:72`).
- [x] **`parse_cube` hace panic** con un .cube malformado
      (`metal/src/main.rs:52`).
- [x] Guard de atajos solo exime `type="text"` (`app.js:101`) — un input
      number/range se comería los atajos.
- [x] Doble generación AAC cuando hay fundidos (corte → AAC, xfade → AAC otra
      vez). Mantener PCM intermedio o retrasar el AAC a la pasada final.

---

## P1 — Table stakes de edición (sin esto no es un editor general)

### Audio (el hueco más grande de todos)

- [x] **Reproducción de audio en la preview** — hoy se edita imagen MUDA (no
      hay AudioDecoder ni `<audio>` en todo studio/js; el botón "mute" solo
      silencia el foley de la UI). Sin oír no se puede cortar al habla ni a la
      música. Es el gap nº 1 del proyecto.
- [x] Scrub con audio (oír al arrastrar la aguja).
- [x] **Pista de audio propia** en el esquema (hoy el JSON de timeline no
      tiene dónde poner audio: solo `clips[].{file,in,out,fade}`): clips de
      audio con `in/out/start`, música bajo el vídeo.
- [x] Importar ficheros solo-audio (WAV/MP3/FLAC/AAC) — hoy `is_video()`
      solo acepta `mp4|mov|m4v|mkv` (`shell/src/server.rs:89`).
- [x] **Ganancia por clip** (control directo en el clip, no un "filtro"
      escondido — lección anti-Shotcut) + mute por clip.
- [x] **Asas de fundido de audio** en las esquinas del clip (fade in/out a
      silencio). Lo más usado por cualquier amateur.
- [x] Volumen con banda elástica (keyframes simples) — mínimo viable para
      bajar la música cuando entra la voz. Ducking automático: P2.
- [x] Separar audio del vídeo (detach) y sustituirlo.
- [x] Mezcla real en el render: hoy NO existe mezcla ninguna (cero hits de
      volume/amix/sidechain en todo el árbol); el render debe componer
      vídeo+audio de N fuentes con ganancias y fundidos.
- [x] Normalización de sonoridad a objetivo (una pasada loudnorm): barata y
      diferencial para "cualquiera puede editar".

### Modelo de timeline

- [x] **Rejilla de frames**: todo el modelo es float de segundos y el timecode
      está clavado a 25 fps (`viewer.js:435` y 5 sitios más; Shift+← = 25
      frames). Adoptar fps de proyecto + `round(t*fps)/fps` en todas las
      operaciones; timecode real del proyecto.
- [x] **Ajustes de proyecto** (resolución, fps, aspecto) con "tomar del primer
      clip" automático — el usuario no sabe qué es 1080p29.97.
- [x] **Relaciones de aspecto de proyecto como ciudadanas de primera**
      (petición explícita): 16:9, 9:16, 1:1, 4:5, 2.39:1… con presets por
      destino (YouTube, Reels/TikTok, cine); el visor, las tiras de la
      timeline (hoy asumen 16:9 en `timeline.js:404` y `lab.css:679`), las
      miniaturas y el encaje fit/fill deben seguir el aspecto del proyecto.
- [ ] Clips con `start` propio (hoy posición ≡ índice del array, huecos
      imposibles): necesario para insert vs overwrite, huecos y la pista de
      audio. Mantener el ripple actual como comportamiento por defecto
      (modo "Normal" de Kdenlive: sin solapes destructivos).
- [x] Insert vs overwrite al soltar; lift (dejar hueco) vs extract (ripple) —
      hoy solo existe borrado ripple.
- [x] **Multi-selección** (`state.sel` es un único id) + mover en bloque.
- [x] **Copiar / pegar / duplicar** clips.
- [x] **Marcas persistentes** con nombre, navegables (hoy solo existe el lápiz
      graso transitorio del empalme) + rangos exportables (P2).
- [x] **Renombrar**: nombre lógico de cinta en el bin y de clip en la bobina
      (media.json ya lo permite; falta la UI).
- [x] Snapping también al arrastrar/recortar clips (hoy solo imanta la aguja).
- [x] Menú contextual en clip (cortar/copiar/borrar/propiedades).
- [x] Navegación por cortes (↑/↓), End al final (hoy sin atajo), J/K/L.
- [x] Auto-scroll de la timeline siguiendo la aguja en reproducción; clamp
      del scroll.
- [x] Insertar desde el monitor de fuente EN la aguja (hoy solo añade al
      final de la bobina); overwrite de 3 puntos: P2.

### Transformación por clip (petición explícita)

- [x] **Escalar / rotar / posicionar / recortar por clip** — enderezar un
      plano, punch-in, encajar un vertical. Implementación natural: matriz UV
      en el pase de grade del motor (cero passes extra) + `transform` por clip
      en el esquema. Con esto la rotación de metadatos (abajo) sale gratis.
- [x] Encaje de aspecto: fit (letterbox) / fill / stretch por clip, default
      fit. Nunca el estirado silencioso actual.
- [x] Velocidad por clip (retime simple sin interpolación sofisticada): P1.5 —
      no existe ni `speed` en el esquema.

### Transiciones (por junta, decisión explícita)

- [x] Cada junta = **corte seco por defecto**; fundido opcional por junta (ya
      existe: click en la cinta de empalme cicla 0/0.5/1/2 s) — añadir
      duración arbitraria (arrastre o entrada numérica), no solo 4 presets.
- [x] **Fundido desde/a negro** en cabeza y cola de la bobina — hoy
      inexpresable (el bucle de xfade empieza en el clip 1).
- [x] Dip-to-black entre clips y 2-3 wipes como mucho — `xfade` ya los trae;
      hoy `transition=fade` está hard-coded (`server.rs:474`).
- [ ] Audio crossfade ya va ligado al fundido de vídeo (bien); permitir
      desacoplarlo (J/L cuts) es P2.
- [x] Mensaje remediador si no hay chicha para el fundido (lección
      anti-Kdenlive: no error seco, ofrecer acortar).

### Robustez de formatos de entrada

- [x] **VFR**: detectar en importación y ofrecer "convertir a apto para
      edición" (modelo Shotcut, diálogo al importar — no aviso en el render
      como Kdenlive). Hoy se toma un fps escalar y los PTS se sintetizan →
      drift silencioso.
- [x] **Rotación de metadatos**: hoy se descarta (el round-trip Annex-B la
      destruye) → los verticales de móvil salen tumbados. Leer displaymatrix
      en el probe y hornearla vía la transform por clip.
- [x] **Mezcla de resoluciones/fps en una timeline**: conformar todo a los
      ajustes de proyecto (fit + fps del proyecto). Hoy: en Mac estira a 4K;
      en Windows cada pieza sale a su tamaño y el concat -c copy produce un
      fichero roto.
- [ ] H.264/ProRes/AV1 de entrada: ya entran de rebote (el paso de corte
      transcodifica todo a HEVC 10-bit), pero es un normalizador accidental —
      formalizarlo como etapa de conform con caché, y saltárselo cuando la
      fuente ya es apta (hoy TODO clip paga un re-encode a 120 Mbps).
- [x] **Fotogramas fijos** (JPEG/PNG/HEIC) con duración por defecto.
- [x] Full-range (el uniform existe pero está clavado a 0 → negros aplastados
      con material full-range) y 8-bit explícito.
- [x] HDR/HLG/PQ: mínimo detectar y tone-mapear a 709 en el conform (hoy se
      clipa duro sin avisar).
- [ ] Audio multi-pista de origen: hoy solo se mapea la pista 0.
- [x] `.webm` y demás contenedores que el lab legacy ya aceptaba.

### Export

- [x] **Presets por objetivo, no por códec** (<10 visibles + cajón avanzado):
      "YouTube 1080p/4K (H.264)", HEVC, ProRes máster, solo-audio. Hoy la GUI
      no ofrece NADA (HEVC hard-coded; el lab legacy tenía selector de códec y
      fps — restaurarlo como plantilla).
- [ ] Resolución/fps de salida distintos del proyecto (rescale al exportar).
- [x] Control de calidad simple (slider ↔ CRF/bitrate).
- [x] Exportar solo un rango marcado.
- [x] **Cancelar un render** (hoy solo hay guard de "ocupado") + progreso con
      % y tiempo restante (ya hay % — añadir ETA).
- [ ] Cola de renders en segundo plano mientras se sigue editando: P2.
- [x] x264/x265 software como fallback universal de export (tier patata).

### Proyecto y persistencia

- [x] **Proyectos con nombre** (hoy un único `~/filmlab/project.json` global):
      nuevo/abrir/guardar-como/recientes. Petición explícita del autor
      (2026-07-31): multi-proyecto es prioridad, no un extra — pantalla de
      inicio con proyectos recientes, doble click en el fichero de proyecto
      abre la app, cada proyecto con sus propios ajustes (resolución, fps,
      aspecto).
- [x] Autosave ya existe (800 ms) — añadir backups rotatorios al estilo
      Kdenlive (cada guardado manual, 20/hora…) y recuperación tras crash
      *probada*.
- [x] **Relink de media perdida**: diálogo al abrir con "buscar en carpeta
      recursivamente y re-enlazar el resto" (copiar el de Kdenlive, no el de
      Shotcut). Hoy los rotos se ocultan en silencio (`server.rs:824-826`).
- [ ] Archivar proyecto (copiar media + proyecto con rutas reescritas) o al
      menos rutas relativas si la media vive junto al proyecto.
- [x] Validar el esquema del project.json al cargar (hoy se escribe el body
      crudo sin validación).
- [ ] Workspace configurable (hoy `~/filmlab` fijo, solo FL_MEDIA lo mueve).

### Color

- [ ] **Perfil de entrada por clip/cinta** con detección: hoy la LUT I-Log de
      la Luna es el default en 3 sitios y el CLI no puede expresar "sin LUT de
      entrada" (`lut_pick` cae al I-Log si no encuentra el nombre) — material
      Rec.709 normal sale lavado. Opciones: `none` explícito + identity, y
      auto-detección por metadata/cámara (auto-davinci ya sabe hacerlo).
- [x] Importar LUTs del usuario desde la UI (el lab legacy podía, incluso
      Hald; el studio solo lee las dos embebidas).
- [ ] Grade por clip (prefs/LUT por clip además del global): P2 — el global
      por proyecto es defendible como identidad del producto.

---

## P2 — Deseables (después de lo anterior)

- [ ] Títulos/texto simple (clip de texto: fuente, tamaño, color, sombra,
      posiciones preset). En 1 pista: clip sobre negro o quemado como overlay.
- [x] Rangos con export por rango; subtítulos SRT (sube hacia TS para público
      YouTube).
- [ ] Ducking automático música/voz (sidechain — auto-davinci ya lo tiene en
      audio.py como referencia).
- [x] Slip/slide/roll; 3-point editing completo.
- [ ] Atajos editables + búsqueda de acciones; mostrar atajos en la UI.
- [x] Drag&drop de ficheros del Finder a la ventana (hoy solo diálogo nativo).
- [x] Zoom-to-fit, loop de reproducción, pantalla completa del visor.
- [ ] i18n: hoy no hay capa de strings (todo literal español, incluida la
      metáfora latas/bobina/revelado — decidir si la metáfora se traduce o se
      versiona por idioma).
- [x] Vista previa a resolución completa opcional (hoy siempre media).

---

## Proxies transparentes (petición explícita)

Hoy los proxies son invisibles: se generan solos en `.proxies/`, son mudos, y
el usuario no sabe cuándo está viendo proxy o máster. Convenciones a adoptar
(estilo Resolve/Premiere):

- [x] Badge por clip: proxy listo / generándose / sin proxy.
- [x] Progreso global visible ("generando proxies 3/10") sin bloquear.
- [ ] Conmutador global proxy/máster en el visor (y auto: proxy al scrub,
      máster en pausa — ya se hace, pero hacerlo VISIBLE).
- [ ] Preferencias: resolución de proxy elegible, ubicación de la caché,
      tamaño ocupado + "vaciar caché", regenerar proxy de un clip.
- [x] Los proxies deben llevar audio (hoy se generan con `-an` —
      imprescindible cuando exista reproducción de audio).

## Selector de motor de render (petición explícita)

- [x] Preferencia de motor visible: **"Máximo rendimiento (zero-copy
      nativo)"** — el camino VT/Metal en el M4 Max y MF/D3D12/AMF en la 890M —
      vs "Automático (probe)" vs "Compatibilidad (ffmpeg/software)". El probe
      elige por defecto, pero el usuario puede fijar el motor y ver CUÁL está
      activo y a qué velocidad rinde (fps medidos en su máquina).
- [ ] Para cada chip nuevo bien soportado, el objetivo es una implementación
      zero-copy "similar" a las dos existentes (mismo contrato de frontera de
      frame); mientras no exista, el tier ffmpeg cubre.

## Motor multi-hardware ("que corra en una patata")

Plan validado por la investigación (OBS/HandBrake/mpv como modelo):

- [ ] **Trait de códec con granularidad de frame** (`decode → handle GPU`,
      `handle GPU → encode`): los caminos VT/Metal y MF/AMF actuales se
      convierten en dos backends sin tocarse. Dispatch por frame = coste cero
      real frente a 2,2 ms/frame.
- [ ] **Backend ffmpeg-hwcontext como paraguas universal** (VAAPI, QSV,
      NVDEC/NVENC, Vulkan, d3d12va, software): es lo que hace que "cualquier
      ordenador" sea verdad. Vulkan Video NO puede ser la base (no existe en
      macOS; encode Intel/AMD verde) — adoptarlo oportunista vía ffmpeg.
- [ ] **Camino portable wgpu**: la cadena ya existe en WGSL (`core/`); los 21
      fps son culpa de los pipes CPU, no del shader. Interop hal por
      plataforma: IOSurface (Mac), shared handle/ID3D12Resource (Win),
      dma-buf fd (Linux — ya en wgpu trunk). NV12/P010 multi-planar es el
      borde afilado en todos los backends.
- [ ] **Probe al primer arranque estilo OBS**: caps query + benchmark REAL de
      ~5 s (decode+shader+encode) → tier persistido (completo / hw-decode+
      sw-encode / solo-proxy) con override manual. Las caps queries mienten
      (la trampa de la 890M es la prueba).
- [x] **Tier patata**: proxies 1080p/540p generados una vez (hw decode + x264
      veryfast), preview con la cadena a media resolución, y para preview en
      GPUs prehistóricas **hornear todo el look en un LUT 3D combinado**
      (solo grano/halación no se hornean: aproximar o omitir en preview);
      export siempre cadena completa, x264 medium sin vigilar (se permite que
      tarde). Suelo de referencia Kdenlive: 4 núcleos/8 GB para HD.
- [ ] Datos de Macs viejos: HEVC 10-bit HW solo desde Kaby Lake (2017); M1
      base sin motores ProRes (empiezan en M1 Pro) → VT cae a CPU, funciona
      pero lento: el tier lo decide el probe, no el nombre del chip.
- [ ] `filmlook-core` tiene `--fps`/`--scale`/2 códecs y no está cableado al
      shell — es el esqueleto natural del tier portable.
- [ ] Rango parcial y resolución de preview en los motores nativos (hoy solo
      `--max-frames` desde el principio; sin `--start`, sin `--scale`).

## Arquitectura del render (deuda estructural)

Hoy: motor nativo = un clip entra/un clip sale; TODA la noción de timeline
vive en `shell/src/server.rs` como cadena ffmpeg (corte 120M → look → concat o
xfade). Camino evolutivo sin romper nada:

1. [ ] Extender el **JSON de timeline** (contrato agente/GUI/CLI único):
       pista de audio, `start`, `transform`, transición por junta con tipo y
       duración, fades cabeza/cola, `speed`, stills, perfil de color por
       cinta, ajustes de proyecto (res/fps).
2. [ ] El shell sigue orquestando (conform con caché → look por clip →
       composición final), pero la composición final deja de re-encodear dos
       veces el audio y aprende mezcla (ganancias, fades, loudnorm).
3. [ ] Más adelante: mover transiciones/transform al motor (un pase más en la
       GPU) para que la pasada xfade de 60 Mbps desaparezca y el export sea
       una sola generación.

## Rendimiento: mucho más lejos (investigado 2026-07-31)

Técnicas de los NLE maduros (Premiere smart render, Resolve render cache, FCPX
background render, Kdenlive chunks, Av1an, LosslessCut) mapeadas a nuestra
arquitectura. Ordenadas por palanca:

- [x] **Export incremental con caché de piezas por hash** (la palanca más
      grande): clave = `hash(identidad+mtime del fichero, in, out, prefs,
      LUTs, transform, versión del motor)` → la pieza renderizada se cachea;
      al re-exportar solo se re-renderizan las piezas cuyo hash cambió y el
      resto se empalma con `-c copy`. Cambiar un corte en una película de 20
      clips = re-renderizar ~1 clip. Los fundidos se cachean como segmentos
      de junta propios (solo se re-renderiza la ventana del solape). Es el
      patrón de hashing de Nuke + el smart render de Premiere, y nuestra
      arquitectura por piezas lo regala.
- [ ] **Intermedios all-intra, no HEVC long-GOP 120 Mbps**: el corte actual a
      HEVC long-GOP paga impuesto de GOP en cada trim/scrub, hace frágil el
      empalme y pierde generación. Veredicto de la práctica: ProRes 422 en
      Mac (el M4 Max tiene 2 motores HW — es gratis), DNxHR SQ o HEVC `-g 1`
      en Windows. HEVC long-GOP solo como formato de ENTREGA final.
- [ ] **Renders de piezas en paralelo**: los puntos de corte son fronteras de
      chunk naturales — K instancias del motor sobre subconjuntos disjuntos
      de piezas sucias (calidad CRF/CQ, no ABR entre chunks); el **audio se
      codifica UNA vez, entero, y se muxa al final** (audio por-chunk =
      drift). Compone perfecto con la caché: paralelismo solo sobre lo sucio.
- [x] **Preview nativa bajo el webview** (adiós al techo de WebGL a media
      resolución): UI en el webview, vídeo en una capa nativa debajo de una
      zona transparente — `CAMetalLayer`/IOSurface alimentada por el MISMO
      motor Metal en Mac (zero-copy desde VT, vsync por CVDisplayLink), child
      HWND wgpu bajo WebView2 en Windows. wgpu-sobre-webview transparente da
      guerra (tauri #9220/#8246) — el camino probado es la ventana hija.
      Esto reutiliza el motor de 413-464 fps para la preview a resolución
      completa.
- [ ] **Sidecars de importación en background** (patrón .pek/.cfa/.PFL):
      por fichero, un worker genera `{índice de keyframes, sprite sheet de
      miniaturas (decode SOLO keyframes: ~100× más rápido, truco Jellyfin),
      peaks de audio formato audiowaveform (min/max por bucket, 8-bit,
      multi-resolución), proxy, detección de cámara/log}` — direccionado por
      hash del fichero para sobrevivir a mudanzas. La UI nunca se bloquea:
      tira gris de placeholder mientras tanto.
- [x] **Pool de decoders con pre-roll en las juntas** (adiós al hipo del
      corte): 2-3 sesiones VT/AMF; al entrar en el último ~1 s del clip N, la
      sesión B hace seek al keyframe previo al in del clip N+1, quema hasta
      el in-frame y lo retiene en GPU; en el corte se conmuta de decoder.
      Con proxies all-intra el pre-roll es casi gratis (seek O(1)).
- [ ] **Cola de frames render-ahead** en reproducción (N frames compuestos
      por delante de la aguja) para absorber jitter de decode.
- [ ] **Apertura de proyecto instantánea**: no tocar NINGÚN fichero de media
      al abrir — confiar en identidad guardada (tamaño+mtime), verificar
      perezosamente al primer uso; todo lo derivado direccionado por hash
      (nada se recomputa); miniaturas/ondas cargadas bajo demanda según el
      viewport. (Las aperturas de 10-45 min de Premiere son el anti-ejemplo.)
- [ ] **Background render estilo FCPX** (opcional, tier débil): renderizar en
      idle (~0.3-5 s sin input) solo los tramos que no reproducen en tiempo
      real, con línea de estado en la regla (rojo/verde estilo Resolve/FCPX).
- [ ] Pendientes del motor ya identificados en TECHNICAL_REPORT §10: GPU a
      <1 ms/frame (fusionar pack en comp, MRT) → techo ~600 fps; decode
      GOP-paralelo si el decoder pasa a ser el cuello.

## QoL invisible (lo que se da por hecho en todo editor)

Enumeración por ciclo de vida (investigación 2026-07-31 sobre FCP/Premiere/
Resolve/Kdenlive/Shotcut/CapCut). **(E)** = su ausencia parece rotura;
**(P)** = pulido. El clúster de máxima palanca: fantasma de arrastre + línea
de inserción + Esc cancela + flash de imán + tooltips de trim + undo total +
placeholders de media offline + relink + guardado atómico + no sobrescribir
en silencio.

### Arranque y proyectos
- [x] (E) Pantalla de bienvenida con proyectos recientes (miniatura, fecha,
      res/fps); nuevo/abrir a un click; entrada obsoleta en gris con
      "no encontrado, ¿quitar?" — nunca crash.
- [ ] (E) Asociación de fichero: doble click en `.proyecto` abre la app con
      él (incluso ya abierta); reabrir el último proyecto al arrancar.
- [ ] (P) Arrastrar proyecto/vídeo al icono del dock; ventana/posición/layout
      restaurados de la última sesión; cheat-sheet de atajos.

### Import y organización
- [ ] (E) Import masivo: progreso "34/120", cancelable, UI viva; errores por
      lote que nombran fichero Y motivo; un corrupto no aborta el lote.
- [x] (E) Drag&drop del Finder al bin Y a la timeline; carpeta → recursivo.
- [x] (E) Miniaturas en background con placeholder (frame al ~10%, nunca
      negro); badge de media offline distinto de "generando".
- [ ] (P) Hover-scrub en miniaturas del bin (el "se siente premium" nº 1);
      ordenar/buscar/filtrar; etiquetas de color; renombrar in situ (F2,
      nombre lógico, jamás el fichero en disco); tooltip de metadata; "mostrar
      en Finder"; badge de "ya usado en la bobina"; multi-selección.

### Timeline (la etapa make-or-break)
- [x] (E) Fantasma semitransparente al arrastrar + línea de inserción que
      enseña dónde caerá (insert vs overwrite visualmente distintos).
- [x] (E) Cursor de corchete en bordes + tooltip vivo de trim (+00:12, nueva
      duración); tope de media visible (borde rojo/resistencia) — recortar
      más allá del material en silencio = roto.
- [ ] (E) Snapping con flash de imán visible, toggle (tecla N) y desactivación
      temporal con modificador.
- [x] (E) **Esc cancela cualquier drag/trim en vuelo** (top 1 de "parece
      roto"); caja de selección elástica en zona vacía; espacio = play SIEMPRE
      y jamás escribe en un campo oculto (reglas de foco de teclado).
- [x] (E) Un gesto = un paso de undo (no cinco); undo cubre TODA mutación.
- [x] (E) Rueda = scroll horizontal, Cmd+rueda/pinch = zoom centrado en aguja
      o puntero (zoom que salta al inicio = roto); Shift+Z = zoom-to-fit.
- [ ] (P) Alt+arrastrar duplica; auto-scroll en reproducción (página/suave/
      off); scroll manual no pelea con la reproducción; menú contextual en
      clip/hueco/regla/cabecera con items DISTINTOS; doble click con
      comportamiento en todo; tooltip de clip; alturas de pista ajustables;
      scrollbar-minimapa con densidad de clips; ↑/↓ = corte anterior/
      siguiente; marcas de color con nota (M).

### Reproducción y monitorado
- [ ] (E) Vúmetros con peak-hold e indicador de clipping que se queda rojo;
      mute/solo; pantalla completa (Esc sale); indicador de frames perdidos
      (nunca stuttering silencioso); timecode clicable para teclear e ir.
- [ ] (P) Salida a segundo monitor; rejillas/áreas seguras; dropdown de
      calidad de reproducción; loop y play in→out; play-around del corte con
      pre/post-roll; zoom del visor (fit/100%) con indicador; exportar
      fotograma actual; scrub de audio opt-in (no chirrido por defecto).

### Feedback y estado
- [x] (E) Indicador de cambios sin guardar (punto en el botón de cerrar);
      confirmación al cerrar; guardado atómico (temp+fsync+rename); si un
      guardado falla, decirlo A GRITOS y retener en memoria.
- [ ] (E) Ventana de tareas en background estilo FCPX (Cmd+9): cada tarea con
      progreso, pausa, cancelar; nada >1 s sin progreso, nada >5 s sin
      cancelar; la UI JAMÁS se congela.
- [ ] (E) "Deshacer recorte" / "Deshacer mover" — el menú dice QUÉ deshace;
      errores accionables (fichero + motivo + siguiente paso, nunca
      "Error -39").
- [ ] (P) Toasts no intrusivos; barra de estado (selección, zoom, disco
      libre); log accesible desde la UI; botón copiar en cada diálogo de
      error; "no volver a preguntar" donde sea seguro.

### Export
- [x] (E) Progreso con %, transcurrido, ETA suavizada, cancelar; cancelar no
      deja fichero a medias con el nombre final (temp + rename); sin
      sobrescritura silenciosa (autoincremento o confirmación); prohibido
      exportar encima de un fichero fuente; impedir que el sistema duerma
      durante el export; avisar al salir con export en marcha.
- [x] (E) Recordar últimos ajustes y carpeta de export por proyecto; fallo de
      export con motivo Y posición ("falló en 00:12:04 — clip X").
- [ ] (P) Tamaño estimado en vivo en el diálogo; nombre por defecto = nombre
      del proyecto; "mostrar el exportado"; notificación del sistema al
      terminar en background; resumen post-export (duración, tamaño, bitrate
      real, tiempo); avisos pre-export (media offline, timeline vacía);
      diálogo simple por defecto con avanzado plegado (modelo CapCut).

### Preferencias
- [ ] (E) Ventana de preferencias estándar (Cmd+,); separación clara ajustes
      de app (caché, tema, atajos) vs de proyecto (res, fps, aspecto);
      restaurar por defecto; sobreviven a las actualizaciones.
- [ ] (P) Ubicación y tamaño de caché + vaciar; intervalo de autosave;
      duración por defecto de transición y de foto fija; dispositivo de
      audio; idioma; tema; resolución de proxy; editor de atajos con
      detección de conflictos y presets ("estilo Premiere/FCP" — palanca de
      adopción enorme); formato de timecode.

### Resiliencia
- [x] (E) Disco desconectado en mitad de sesión: placeholders "Media
      Offline" (pizarra de color), estructura intacta, relink automático al
      volver el disco — crashear aquí mata la confianza para siempre.
- [x] (E) Diálogo único de ficheros perdidos al abrir (Localizar / Dejar
      offline / Buscar carpeta recursivo; localizar uno re-enlaza a sus
      hermanos — el modelo es el Link Media de Premiere); validar duración/
      fps del encontrado.
- [x] (E) Campo de versión en el fichero de proyecto DESDE EL DÍA UNO;
      migración silenciosa con copia de seguridad previa; "creado con
      versión más nueva" → aviso claro, nunca abrir-romper-guardar.
- [x] (E) Crash del GPU/device-lost: recuperar la superficie o estado
      "reiniciar renderizador" con el proyecto intacto — el swapchain jamás
      se lleva el proyecto por delante.
- [x] (P) Backups rodantes independientes del autosave; detección de doble
      instancia (lock → solo-lectura o enfocar la existente); rutas
      relativas + absolutas (mover proyecto+media juntos abre limpio).

### Pulido y accesibilidad
- [ ] (E) Tooltip en cada control CON su atajo; cursor coherente por acción
      (el cursor ES el indicador de modo); nitidez high-DPI (probar el
      webview en Windows con escalado fraccional — trampa conocida);
      comportamiento estándar de texto en todo campo (Cmd+A/C/V/X/Z, IME).
- [ ] (E) **Frame-accurate en todo**: el número que enseña el tooltip es el
      frame exacto donde corta el motor — un off-by-one entre UI y motor es
      el "parece roto" más profundo de todos.
- [ ] (E) Nunca un modal durante reproducción o arrastre; Cmd+S siempre
      funciona aunque haya autosave (memoria muscular jamás da error).
- [ ] (P) Menú completo de la app (búsqueda del menú Ayuda en macOS); focus
      rings y tab-order sano; escala de UI; respetar reduced-motion; paleta
      segura para daltónicos en etiquetas/línea de caché; About con versión,
      GPU y "copiar diagnóstico"; atajos visibles en los menús; scrub-drag
      en los números de los inspectores (convención AE) con doble click para
      teclear y alt-click para reset.

## El foso competitivo (recordatorio)

La queja nº 1 que expulsa a la gente de Shotcut/Kdenlive es la preview a
tirones y el render que no coincide con la preview. Nuestro motor da preview
fluida hasta en una iGPU de 28 W (60 fps SIN proxies) y un solo camino de
píxeles. Todo lo de esta lista debe construirse sin sacrificar eso.

---

## LA APP NATIVA (decidido 2026-07-31)

Decisión del autor: **toda la aplicación nativa, por defecto**. El webview es
un techo estructural (media resolución, decodificador JS, HTTP para píxeles que
la GPU ya tiene) y la "ampliadora" nativa era un parche. UI dibujada con la
MISMA pipeline wgpu del motor (opción elegida frente a egui: conserva la
estética Saorín exacta).

### Estado (paso 1 hecho)

- [x] Crate `nativa/` (`saorin-nativa`): ventana winit + wgpu (Metal verificado
      en M4 Max), bucle de eventos, escalado lógico/físico.
- [x] `proyecto.rs`: lee el MISMO `~/filmlab` que el resto (bobina abierta,
      media.json por referencia, LUTs, prefs, fps) — sin servidor.
- [x] `ui.rs`: lienzo 2D propio en wgpu (quads + líneas con la paleta del
      zine) y la bobina dibujada nativa: tiras, perforaciones, cintas de
      empalme, regla y aguja.
- [x] `visor.rs`: reloj de bobina, decodificador propio por clip, encaje del
      vidrio, transporte (espacio/flechas/Home) y arrastre de la aguja.
- [x] Arranca y pinta: verificado con captura de la ventana real.

### Paso 2 HECHO (misma sesión)

- [x] **Cadena fílmica enganchada**: grade 2×LUT → shutter (IIR) → pirámide
      down/blur → comp, todo dentro de la ventana; el comp entra por viewport
      en el rect del vidrio. Verificado con capturas (Mac Metal / GPD Vulkan).
- [x] **Cuarto oscuro POR CLIP** (petición explícita): cada clip lleva sus
      prefs (las del proyecto + las suyas encima) y sus gelatinas; el visor
      las aplica al cruzar cada junta, con caché de LUTs.
- [x] Tipografía con glyphon: rótulo, timecode, nombres, susurros, atajos.
- [x] **Interfaz completa**: estantería (clic → a la bobina), visor con
      play/pausa al clic, inspector del cuarto oscuro con 8 mandos vivos
      (arrastre horizontal, valor a la derecha), banco con regla, tiras con
      perforaciones, cinta de empalme, selección, aguja arrastrable, lupa con
      rueda, y línea de atajos.
- [x] Edición: añadir, cortar (B), quitar (⌫), mover clip (arrastre),
      recortar por los bordes, guardar (S) en el MISMO project.json del
      taller, revelar (R) lanzando el revelador.
- [x] **Trampa cazada**: los nombres de LUT con acento fallaban por
      normalización Unicode (macOS NFD vs JSON NFC) → búsqueda tolerante.

### Paso 3 — HITO 0 HECHO (2026-07-31 noche, ver TRASPASO §addendum)

- [x] **Decode por hardware EN PROCESO (VideoToolbox) en vez del pipe
      ffmpeg**: `core/src/indice.rs` (índice del moov, seek O(1)) +
      `core/src/cine.rs` (sesión VT síncrona, muestras AVCC directas del
      mdat). Proxy 0,7–2,8 ms/frame; máster 4K a 225 fps; espacio→frame
      ~40 ms; scrub arrastrando con imagen viva. MF en Windows pendiente
      (el código está en winlab/src/mf_decode.rs).
- [x] Política proxy/máster del webview portada: proxy all-intra = caballo de
      batalla; máster a resolución completa para el frame exacto en pausa
      (refinado con debounce 400 ms), `nativa/src/cabina.rs`.
- [x] **Audio nativo (cpal + symphonia)** sincronizado al transporte
      (reproducción; scrub audible y vúmetros: pendiente).
- [x] **Miniaturas instantáneas** (ronda 2): hilo de miniaturas con decoders
      de proxy (1–3 ms/frame) + atlas GPU 2048² con LRU → fotogramas reales
      en la tira de la bobina y en las latas. Scrub SÍNCRONO en el hilo de
      eventos (0,8–3,7 ms). Backend **Media Foundation verificado en el GPD**
      (seek máster 12,9 ms, 58–60 Hz). Instalado: .app en Mac + acceso
      directo en el GPD.
- [ ] Texturas del taller (papel, grano, latas, doodles) como atlas.
- [ ] Estantería, inspector, cuarto oscuro y sala de revelado nativos (1:1).
- [ ] Edición completa sobre el lienzo (cortes, arrastres, imanes, menús).
- [ ] Retirar el webview y el servidor HTTP (queda `saorin cli` para agentes).
