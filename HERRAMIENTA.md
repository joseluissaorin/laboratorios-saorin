# HERRAMIENTA — de juguete a editor de vídeo

> La sensación que describe el autor: «me faltan cosas para sentir que deja
> de ser un juguete y que puede ser una herramienta». Este documento la
> completa: es la **checklist de entregables** que equipara la app con lo
> que cualquier persona espera de un editor de vídeo — con iMovie/CapCut
> como suelo de INTUITIVIDAD (nada requiere manual) y Resolve/Premiere como
> listón de CAPACIDADES básicas, dentro del alcance de la casa (1 pista de
> vídeo + 1 de audio, motor único preview=export, estética Saorín).
>
> Muchas de estas capacidades EXISTEN en el webview (GENERALIZACION.md las
> marca [x]) pero el propio autor señala que ni ahí eran intuitivas
> (aspecto, proyectos, ajustes). Por eso cada entregable de aquí especifica
> también el LISTÓN DE UX: no basta con que se pueda — tiene que verse,
> entenderse y tocarse sin explicación. Complementa a HITO1.md (que es la
> paridad 1:1 con el webview): esto es lo que va MÁS ALLÁ o lo que hay que
> SUBIR DE RANGO en visibilidad.
>
> Formato: ✅ entregable verificable · «se siente herramienta cuando…» =
> el criterio de aceptación en términos de sensación, no de código.

---

## 1. Los proyectos son OBJETOS, no un fichero escondido

Hoy el proyecto es `~/filmlab/project.json` + `current.txt`: invisible,
innombrable, imposible de compartir. Un editor de verdad trata el proyecto
como un documento.

- [ ] **Nuevo proyecto** con nombre desde la portada: pide nombre y
      ajustes (o «tomar del primer clip») — dos campos, no un formulario.
- [ ] **Abrir / recientes** en la portada: tarjeta con miniatura del
      último fotograma, nombre, fecha, duración, res/fps/aspecto.
      Entrada rota en gris con «no encontrado, ¿quitar?».
- [ ] **Guardar como / duplicar / renombrar / borrar** (a la papelera).
- [ ] **Un proyecto = un fichero `.bobina`** (JSON versionado) que se
      puede mover, copiar y mandar. Doble clic en el Finder/Explorer lo
      abre en la app (asociación de fichero + icono propio).
- [ ] **Rutas portables**: media por referencia con rutas absolutas Y
      relativas-al-proyecto; si proyecto y media viajan juntos, abre
      limpio en otra máquina.
- [ ] **Archivar proyecto**: «empaquetar bobina» = copiar proyecto +
      media usada a una carpeta elegida, con rutas reescritas (el backup
      de verdad, y el «pásamelo al portátil»).
- [ ] Cambiar de proyecto sin cerrar la app (selector en el rótulo, ya
      especificado en H1.1) — con «¿guardar cambios?» si toca.
- [ ] **Título de ventana** = nombre del proyecto + punto de «sin
      guardar» (la señal universal de documento).

**Se siente herramienta cuando**: puedes tener «Boda de Marta» y «Pruebas
Luna» a la vez, volver a uno de hace un mes desde la portada, y mandarle
la bobina empaquetada a alguien.

## 2. El proyecto tiene FORMATO: resolución, fps y aspecto a la vista

Hoy el aspecto «pasa» (auto del primer clip) y no se ve por ningún sitio.
El formato del lienzo es una decisión creativa de primer orden.

- [ ] **Selector de formato en la creación** (y editable después en
      ajustes de proyecto), con presets POR DESTINO, no por número:
      · Apaisado 16:9 (YouTube / TV)
      · Vertical 9:16 (Reels / TikTok / Shorts)
      · Cuadrado 1:1 · Retrato 4:5 (feed)
      · Cine 2.39:1 · Clásico 4:3 · Personalizado (w×h)
- [ ] **fps de proyecto** explícito (23.976 / 24 / 25 / 29.97 / 30 /
      59.94 / 60) con «tomar del primer clip» como default asistido —
      el usuario que no sabe qué es 29.97 no tiene que saberlo.
- [ ] **El formato SE VE**: el vidrio del visor tiene la proporción del
      proyecto SIEMPRE (letterbox sobre el papel si el material no
      encaja); las tiras de la bobina y las miniaturas siguen el aspecto
      del proyecto (hoy asumen 16:9); el rótulo dice «1080p25 · 9:16».
- [ ] **Todo clip se conforma al proyecto**: fit (letterbox) por defecto,
      jamás estirado silencioso; fps distintos se remuestrean al del
      proyecto (el motor ya; hacerlo visible).
- [ ] **Cambiar el formato a mitad** re-encaja todo sin romper nada y
      avisa de lo que va a pasar.
- [ ] El render sale EXACTAMENTE al formato del proyecto (la trampa del
      4K clavado del Mac ya está arreglada en el shell — verificar la
      cadena entera desde la nativa).

**Se siente herramienta cuando**: creas «vertical para Reels», sueltas
clips apaisados de la Luna, y el visor te enseña desde el primer segundo
exactamente lo que va a salir.

## 3. Encuadrar POR CLIP: la transformación

Petición explícita del GENERALIZACION y lo primero que se echa en falta
con material vertical/mixto. Implementación natural: matriz UV en el pase
de grade (cero pases extra).

- [ ] **Escala / posición / rotación / recorte por clip** con manejadores
      SOBRE el vidrio (arrastrar para reposicionar, esquinas para
      escalar, rueda para zoom) + valores finos en el inspector.
- [ ] **Fit / fill / stretch por clip** (default fit) — un botón, no un
      menú enterrado.
- [ ] **Punch-in rápido**: doble función del zoom — reencuadrar un plano
      sin salir de la mesa (el 2× de toda entrevista).
- [ ] Enderezar: rotación fina ±5° con guía de horizonte.
- [ ] **Rotación de metadatos** aplicada de serie (los verticales de
      móvil entran DERECHOS, no tumbados).
- [ ] Reset por clip (alt-clic) y «copiar encuadre a…».

**Se siente herramienta cuando**: un vertical de móvil dentro de un
proyecto 16:9 se arregla en cinco segundos arrastrando sobre el vidrio.

## 4. Traer material sin sorpresas (robustez de importación)

- [ ] Acepta lo que la cámara y el móvil escupen: HEVC/H.264 10/8-bit,
      ProRes, **AV1** (vía conform), .mp4/.mov/.m4v/.mkv/.webm; audio
      WAV/MP3/FLAC/AAC; fotos JPEG/PNG/HEIC (con duración por defecto
      configurable).
- [ ] **VFR detectado al importar** con diálogo «convertir a apto para
      edición» (modelo Shotcut) — nunca drift silencioso.
- [ ] **HDR/HLG/PQ → tone-map a 709** en el conform, avisando (nunca
      clipado duro sin decir nada).
- [ ] Full-range detectado y respetado (negros aplastados = roto).
- [ ] Un fichero corrupto en un lote de 100 NO aborta el lote: error con
      nombre y motivo, el resto entra.
- [ ] Perfil de color de ENTRADA por cinta con autodetección de cámara
      (la Luna → I-Log automático; material 709 → sin LUT de entrada —
      hoy el I-Log es default y lava el material normal). `none`
      expresable. auto-davinci ya sabe detectar cámara: portar la firma.
- [ ] Audio multi-pista de origen: elegir pista (hoy solo la 0).

**Se siente herramienta cuando**: arrastras la tarjeta entera de la
cámara + tres notas de voz del móvil + dos fotos y TODO entra, derecho,
con el color correcto, y lo roto te lo dice por su nombre.

## 5. Ajustes y preferencias (la app también se configura)

Separación clara y estándar (Cmd+, / Ctrl+,):

- [ ] **Preferencias de APP**: carpeta del taller (workspace — hoy
      `~/filmlab` fijo), ubicación y tamaño de la caché + «vaciar»,
      resolución de proxy, intervalo de autosave, duración por defecto de
      transición y de foto, dispositivo de salida de audio, sonido del
      taller on/off, escala de UI, idioma (es/en al menos), tema del
      papel.
- [ ] **Ajustes de PROYECTO** (otra pestaña, otra cosa): resolución, fps,
      aspecto, LUTs por defecto, carpeta de renders del proyecto.
- [ ] **Selector de motor** visible (petición explícita): «Máximo
      rendimiento (nativo)» / «Automático» / «Compatibilidad (ffmpeg)» —
      con el motor ACTIVO y sus fps medidos a la vista.
- [ ] **Proxies transparentes**: conmutador global proxy/máster en el
      visor + indicador de qué se está viendo + regenerar proxy de un
      clip + progreso global visible.
- [ ] **Editor de atajos** con detección de conflictos y presets
      («estilo Premiere», «estilo FCP» — palanca de adopción) + búsqueda
      de acciones.
- [ ] Restaurar por defecto; las preferencias sobreviven a las
      actualizaciones.

**Se siente herramienta cuando**: abres Cmd+, y está TODO donde esperas,
dividido en «la app» y «este proyecto».

## 6. Montar con vocabulario completo (lo que falta sobre HITO1.4)

HITO1.4 ya lista la bobina completa. Esto es lo que un editor espera
ADEMÁS, como vocabulario estándar:

- [ ] **Insert vs overwrite** como modos visibles al soltar (no solo
      ripple implícito).
- [ ] **Transiciones por junta con menú**: corte / fundido cruzado /
      dip-to-black / 2-3 wipes — duración arrastrable en la propia cinta
      de empalme.
- [ ] **Velocidad por clip** en el menú del clip (0.25×–4×, con el audio
      re-pitcheado o mudo a elección) + congelar fotograma (still del
      frame actual).
- [ ] **Slip / slide / roll** (el webview ya tenía slip): las tres
      herramientas de ajuste fino de un corte.
- [ ] **Exportar fotograma actual** a PNG (el botón de «me llevo este
      still»).
- [ ] **Duplicar clip** con alt+arrastrar.
- [ ] Selección de rango I/O de bobina con export parcial (ya en modelo)
      — visible como franja en la regla.

## 7. Rotular: títulos y texto (el mínimo digno)

Sin títulos no hay editor — ni siquiera de juguete. Alcance mínimo
coherente con 1 pista (clip de texto sobre negro o quemado como overlay):

- [ ] **Clip de título**: texto, fuente (las del zine + sistema), tamaño,
      color, sombra/borde, posiciones preset (tercio inferior, centrado,
      esquina), entrada/salida con fundido.
- [ ] **Overlay sobre vídeo** (quemado): el rótulo del lugar, el nombre
      del entrevistado.
- [ ] Estilo de la casa por defecto (la tipografía del zine, el ámbar
      Saorín) — que el título por defecto ya sea bonito.
- [ ] **Subtítulos SRT**: importar un .srt y quemarlo con el estilo
      elegido (la casa ya dibuja subtítulos con Pillow en auto-davinci;
      en nativo: el mismo estilo «filtración» como preset).

**Se siente herramienta cuando**: pones un tercio inferior con el nombre
de alguien en menos de un minuto y no da vergüenza enseñarlo.

## 8. Sonar como algo terminado (audio de entrega)

Sobre HITO1.8 (mezcla, banda elástica, fundidos):

- [ ] **Vúmetros** siempre visibles con peak-hold y clipping pegajoso.
- [ ] **Normalización de sonoridad** al exportar (−14 LUFS YouTube /
      −16 podcast / off) — un desplegable, no un filtro.
- [ ] **Ducking automático** música/voz (sidechain — la receta ya está
      en auto-davinci/audio.py): un interruptor «la música se aparta
      cuando hay voz».
- [ ] Silenciar/solo por pista; ganancia de clip visible en el clip.
- [ ] Grabar voz en off directo a la pista (micro del sistema) — el
      «scratch narration» de toda pieza.

## 9. Entregar con confianza (el export como acto claro)

- [ ] **Presets por OBJETIVO** arriba del todo: «YouTube 4K» / «YouTube
      1080p» / «Reels/TikTok» / «Máster ProRes» / «Solo audio» — y un
      cajón «avanzado» plegado (códec, bitrate/CRF con slider de
      calidad, res/fps de salida distintos del proyecto).
- [ ] **Nombre por defecto** = nombre del proyecto; carpeta recordada
      por proyecto; sin sobrescritura silenciosa (autoincremento).
- [ ] **Tamaño estimado en vivo** en el diálogo.
- [ ] Progreso con %, transcurrido, **ETA suavizada**, cancelar limpio
      (temp+rename), impedir que el sistema duerma, aviso si cierras con
      export en marcha.
- [ ] **Cola de renders**: seguir editando mientras revela (P2 del
      GENERALIZACION — aquí sube de rango: es lo que espera cualquiera
      que exporta dos versiones).
- [ ] Notificación del sistema al terminar + «mostrar en carpeta» +
      resumen (duración, tamaño, bitrate real, tiempo de revelado).
- [ ] Avisos PRE-export: media offline, huecos al final, timeline vacía.
- [ ] Fallo con motivo Y posición («falló en 00:12:04 — clip X»).

**Se siente herramienta cuando**: le das a «Revelar para YouTube» y todo
lo demás (nombre, sitio, sonoridad, no-dormir, aviso al acabar) ya está
pensado por ti.

## 10. No perder trabajo JAMÁS (la confianza)

- [ ] Autosave (ya, 800 ms en webview) + **backups rotatorios** + campo
      de versión de formato + migración con copia previa.
- [ ] **Recuperación tras crash PROBADA**: matar la app a medias de un
      gesto → reabrir → «¿recuperar la sesión?» → cero pérdida.
- [ ] Device-lost de GPU → reiniciar renderizador con el proyecto
      intacto.
- [ ] Historial de versiones simple («la bobina de ayer a las 18:03»)
      sobre los backups rotatorios — abrir en solo-lectura.
- [ ] Doble instancia detectada (lock → enfocar la existente).
- [ ] Undo TOTAL (toda mutación, 80 pasos, con nombre: «deshacer
      recorte») — ya en HITO1.4; aquí el listón: no existe acción sin
      undo.

## 11. Acompañar al que llega (onboarding y ayuda)

- [ ] **Estados vacíos que enseñan**: estantería vacía → «arrastra aquí
      tus vídeos»; bobina vacía → «doble toque en una lata»; dibujados a
      mano, en el tono del zine.
- [ ] Tooltip en CADA control con su atajo.
- [ ] La chuleta (`?`) + un **tour de 60 segundos** la primera vez
      (opcional, descartable): las tres salas, la manivela, B para
      cortar.
- [ ] Mensajes remediadores, no errores secos («no hay chicha para el
      fundido — ¿acorto el clip?» — lección anti-Kdenlive).
- [ ] About con versión, GPU, motor activo y «copiar diagnóstico».

## 12. Rendimiento como CONTRATO (lo conseguido no se negocia)

- [ ] Los números de VELOCIDAD.md convertidos en **tests de regresión**
      (crono_cine en CI manual antes de cada entrega): scrub <20 ms sin
      proxy, play <16 ms desde pausa, junta <30 ms.
- [ ] Indicador de frames perdidos (nunca stuttering silencioso).
- [ ] Apertura de proyecto instantánea: no tocar NINGÚN fichero de media
      al abrir (identidad tamaño+mtime, verificación perezosa).
- [ ] La app abre en <1 s hasta portada; un proyecto de 100 clips abre
      en <2 s con placeholders y se rellena solo.

---

## ESTADO (sesión de objetivo, rondas 5–34 del TRASPASO)

Completado y desplegado en ambas máquinas: §1 proyectos (portada,
crear/abrir/recientes, multi-proyecto en caliente) · §2 formato visible
(presets por destino, letterbox real, rótulo) · §3 encuadre por clip
(punch-in/pan vía UV) · §5 v1 preferencias persistentes (prefs.json +
ajustes conmutables) · §7 v1 títulos (rasterizados al formato, clip-foto,
preview=export) · §9 v1 revelar por objetivo (4 presets → codec/loudnorm
reales) + % real/ETA/cancelar/revelada al Finder · §6 parcial (velocidad,
fundidos por junta, duplicar) · §8 parcial (mezcla voz+música+foley,
ganancia, fundidos y banda elástica de música) · §10 parcial (undo total
de ambas pistas, guardado atómico) · §11 parcial (chuleta, avisos
remediadores). Pendiente el detalle restante de cada sección (ver
checkboxes) y §4 robustez de importación avanzada (VFR/HDR/rotación).

## ESTADO 2 (NORTE.md S1–S6, rondas 35–40)

La forma pasó a mandarla **NORTE.md** y se ejecutó entera: las tres
salas de verdad (cuarto oscuro con los 37 galvanómetros/baños/stocks/
gelatinas/receta; revelado con cubetas animadas, tira-progreso de
fotogramas reales, cuerda-galería, cola y sello), la mesa con FICHA DEL
CLIP (velocidad, palanca de sonido, croquis del encuadre, washi, receta,
contacto/duplicar/cubo), cubo de RECORTES rescatable, manivela con
rueda, bandas rotuladas con palancas, acetato de guías, ⌥⌫ lift, [/]
trim a la aguja, susurros contextuales, LA PARED en ajustes, archivador
rotatorio. Novedades de modelo: Clip.{mute,washi,nota} +
Proyecto.{mudo_voz,mudo_musica}. Bug real de render arreglado (piezas
mudas de foto/título rompían los fundidos). Pendientes gordos: §2bis
baldas, §4 VFR/HDR, vúmetros/ducking, grapadora, drag-out.

## Priorización: qué quita antes el olor a juguete

Orden por impacto en la sensación (opinión fundada en la queja concreta
del autor — aspecto, proyectos, ajustes — y en el uso real):

1. **§1 Proyectos** + **§2 Formato visible** — es LA diferencia entre
   demo y documento de trabajo. (Toca portada H1.1 y modelo de datos.)
2. **§9 Entregar** — sin un export claro no hay herramienta, hay motor.
3. **§3 Transform por clip** — el material real es mixto y vertical.
4. **§7 Títulos** — el mínimo digno.
5. **§5 Preferencias** + **§10 Confianza** — en paralelo con todo.
6. **§4 Import robusto**, **§6 vocabulario**, **§8 audio**, **§11
   onboarding** — incrementales sobre HITO1.

Encaje con HITO1.md: §1–§2 se funden con H1.1 (portada/salas) y el
modelo de datos de H1.4; §9 con H1.7; §8 con H1.8; el resto son
sub-hitos nuevos (H1.10 transform, H1.11 títulos, H1.12 preferencias,
H1.13 confianza) que se intercalan tras la primera pasada 1:1.

**El criterio final de este documento**: alguien que edita en CapCut o
iMovie se sienta delante, monta una pieza con música, título y export
para Reels, y en ningún momento pregunta «¿y esto cómo se hace?» ni
piensa «qué bonito el juguete». Piensa: qué herramienta.
