# HITO 1 — la aplicación completa (paridad 1:1 con el webview)

> Desglose en profundidad del Hito 1. NO es «una cuestión de interfaz»: es
> portar TODAS las funciones que la versión web (`studio/`, 5563 líneas: 12
> módulos JS + CSS + HTML) ya tenía, sobre la base instantánea del Hito 0.
> **`studio/` es la ESPECIFICACIÓN**: pantalla a pantalla, gesto a gesto,
> hasta que sea indistinguible — con la ventaja nativa (resolución completa,
> instantaneidad, sin servidor). Ver TRASPASO.md §1 (lo que el autor quiere)
> y VELOCIDAD.md (la base sobre la que se construye).
>
> Regla heredada del traspaso: **portar, no reinventar**. Cada vez que haya
> una duda de comportamiento, la respuesta está en el código de studio/.

---

## 0bis. ESTADO (tras NORTE.md S1–S6, rondas 35–40 — ver TRASPASO)

H1.0 ✅✅ (atlas de doodles horneado, papel PROCEDURAL, trazo a pulso,
foley) · H1.1 ✅ (LAS TRES SALAS de verdad con transiciones — pliegue
v1 sin RTT) · H1.2 ~85% (latas con miniatura + cinta de 6; faltan
baldas §2bis, badges, media offline) · H1.3 ✅ (+cinta de 6 navegable) ·
H1.4 ✅ (falta mover-en-bloque/caja elástica/grapadora) · H1.5 ✅- (LA
MANIVELA con rueda e inercia de reproducción; faltan J/K/L reales,
lupa cuentahílos, vúmetros) · H1.6 ✅ (sala propia: 37 galvanómetros /
6 baterías, baños, stocks, cajón de gelatinas, receta calcada, tira de
contactos, A/B) · H1.7 ✅✅ (cuerda-galería + tira-progreso + cola +
sello REVELADA; bug de piezas mudas del shell arreglado) · H1.8 ~70%
(+palancas de silencio por banda y por clip; faltan vúmetros/ducking/
scrub audible) · H1.9 parcial (archivador rotatorio cada 5 min).
La FORMA la manda **NORTE.md** (papel/tinta/trazo/objetos/salas).

## 0. Estado de partida (lo que la app nativa YA tiene)

Del Hito 0 y las rondas 2–4:

- Motor: cadena fílmica completa por clip (grade 2×LUT → shutter →
  pirámide → comp), decode nativo VT/MF, scrub/play/pausa instantáneos con
  y sin proxies, refinado a máster 4K, audio AAC sincronizado, precalentado.
- UI esqueleto: cabecera con timecode, estantería (lista con miniatura),
  visor, inspector con 8 mandos, bobina con fotogramas reales +
  perforaciones + cinta de empalme + selección + aguja, atlas de miniaturas.
- Edición mínima: añadir cinta, cortar (B), quitar (⌫), mover, recortar
  bordes, guardar (S) en el MISMO project.json, revelar (R) vía CLI del
  shell, herencia del cuarto oscuro del proyecto en clips nuevos.
- Infra: `ui::Atlas` (quads texturizados), `ui::Tipos` (glyphon con caché),
  hilos cabina/miniaturas/sonido, `FL_CRONO` para medir.

Lo que sigue es TODO lo que falta, organizado en sub-hitos con orden de
dependencia, referencia a la especificación y criterio de aceptación.

---

## H1.0 — Fundamentos de UI nativa (bloquea casi todo lo demás)

La deuda estructural del esqueleto: sin esto, cada pantalla nueva sale
"de juguete" y hay que rehacerla.

**Especificación**: `studio/css/lab.css` (papel hueso #f2eee4, tinta
ultramar #2b3bc7, tipografías, texturas), `studio/index.html` (estructura).

1. **Atlas de texturas del taller**: papel con grano, latas metálicas,
   cinta de empalme, doodles a lápiz, pizarra de media offline, cubetas.
   Fuentes de los assets: `studio/` y `app/ui/assets/`. Cargarlas como
   segundo atlas (o ampliar el actual) con 9-slice para los marcos.
2. **Tipografías reales del zine**: Space Grotesk (rótulos), Courier
   (datos/timecode), Caveat (manuscritas — el rótulo de la bobina, los
   susurros). Hoy glyphon usa la sans del sistema. Cargar los TTF con
   fontdb en `Tipos` y exponer familia por texto.
3. **Sistema de widgets mínimo** (no un framework: lo justo):
   - hit-testing con rectángulos CON capa/z y estado hover/press/drag;
   - cursores por zona (mano, corchete de trim, grabbing, I-beam) — el
     cursor ES el indicador de modo (QoL del GENERALIZACION);
   - tooltips (con atajo) y "susurros" temporales (avisos);
   - foco de teclado: espacio JAMÁS escribe en un campo; Esc cancela el
     gesto en vuelo (ya es regla);
   - campos de texto mínimos (renombrar, valores numéricos, nombre de
     bobina) con edición estándar.
4. **El sonido del taller (foley)** — `studio/js/foley.js` (247 líneas):
   clics de manivela, empalme, latas, cubetas. Portar como samples + rodio
   o cpal mixer (el stream de cpal ya existe: mezclar foley + preview).
   Con su toggle en ajustes, como en el webview.
5. **Persistencia de ventana**: tamaño/posición restaurados, escala UI.

**Aceptación**: una captura de la mesa nativa y una del webview lado a lado
se confunden a primera vista (papel, tinta, tipos, latas).

---

## H1.1 — Salas, portada y navegación

**Especificación**: `studio/js/app.js` (726 líneas), `menu.js`, `index.html`.

1. **Tres salas**: *la mesa* (montaje), *el cuarto oscuro* (look), *el
   revelado* (render) — con el **pliegue de papel** al cambiar (transición
   propia del zine; en nativo: shader/animación del lienzo).
2. **Portada (pantalla de bienvenida)**: elegir bobina o crear una nueva —
   proyectos recientes con miniatura, fecha, res/fps; entradas obsoletas en
   gris con "¿quitar?" — nunca crash (QoL (E)).
3. **Multi-proyecto**: selector de bobinas en el rótulo; `current.txt` +
   `projects/*.json` ya existen y la nativa ya los LEE — falta crear,
   renombrar, cambiar y guardar-como desde la UI.
4. **Ventana de ajustes (⚙)**: lupa fina on/off, sonido del taller, motor
   y tier a la vista, resolución de proxy, puntero a la chuleta.
5. **Chuleta de atajos (`?`)** — sobreimpresa, con TODOS los gestos.
6. Menú contextual genérico (composición nativa: clip/lata/regla tienen
   menús DISTINTOS — QoL).

**Aceptación**: arrancar la app sin proyecto → portada; crear bobina;
cambiar entre 3 salas con el pliegue; `?` enseña la chuleta.

---

## H1.2 — La estantería completa (media pool)

**Especificación**: `studio/js/mesa.js` (266) + partes de app.js.
Estado nativo: lista plana con una miniatura. Falta CASI TODO:

1. **Latas de verdad**: lata metálica con TRES fotogramas asomando
   (miniaturas reales del atlas), duración y nombre; badge de proxy
   (verde listo / ámbar cociéndose) + contador global de proxies.
2. **Hoja de contactos** al pasar el ratón: 6 miniaturas + w×h×fps.
3. **Gestos**: un toque → la cinta al monitor de FUENTE; doble toque → a
   la bobina. Clic derecho → renombrar (nombre lógico, jamás el fichero) /
   quitar (a la papelera del sistema).
4. **Importación**:
   - diálogo nativo de ficheros (rfd) — forzado al frente, avisar si no
     entra nada (trampa conocida del webview);
   - **drag&drop del Finder/Explorer a la ventana** (winit lo da nativo) —
     al bin Y a la bobina; carpeta → recursivo;
   - registro POR REFERENCIA en `media.json` (regla de la casa: cero
     copias);
   - import masivo con progreso "34/120", cancelable, errores por lote con
     fichero Y motivo (QoL (E)).
5. **Generación de derivados al importar** (sidecars eager, como el shell):
   proxy 640p all-intra + poster + (H1.4) forma de onda. El generador vive
   en el shell — portar la receta a un hilo "cocina" nativo con ffmpeg
   como PROCESO DE FONDO (permitido: no es camino interactivo), o llamar
   al CLI del shell. Badges vivos mientras se cuecen.
6. **Latas de audio** (icono de onda) y de **fotos fijas** — `is_video()`
   hoy solo acepta vídeo; aceptar WAV/MP3/FLAC/AAC y JPEG/PNG/HEIC.
7. **Media offline**: pizarra (clapperboard) en la lata y en la tira,
   botón «buscar…» que re-enlaza RECURSIVAMENTE (localizar uno re-enlaza a
   sus hermanos — modelo Link Media de Premiere, ya implementado en el
   shell: portar la lógica). La estructura JAMÁS se pierde.

**Aceptación**: importar una carpeta arrastrándola; ver latas con
miniaturas y badges; hover → hoja de contactos; doble clic → a la bobina;
desconectar un disco → pizarras + relink al volver.

---

## H1.3 — El monitor de fuente y el montaje de 3 puntos

**Especificación**: viewer.js (modo fuente) + app.js.

1. Reproducir una cinta SIN pasar por la bobina (la cabina ya sabe:
   es una `Toca` sobre otra ruta; falta el estado "fuente" en el visor y
   el conmutador fuente/programa).
2. **Marcas I/O** sobre la cinta (i/o), con su franja visual.
3. **⏎ inserta** el tramo [I,O] en la aguja de la bobina; **⇧⏎ al final**.
4. Insert vs overwrite al soltar (la base del 3-point editing — P2 del
   GENERALIZACION pero el modelo de datos debe nacer preparado).

**Aceptación**: un toque en lata → fuente; marcar I/O; ⏎ → el tramo queda
en la bobina en la aguja.

---

## H1.4 — La bobina COMPLETA (la sala make-or-break)

**Especificación**: `studio/js/timeline.js` (887 líneas — el módulo más
grande) + `state.js` (462). Lo nativo tiene la tira con fotogramas y 4
gestos; faltan ~25 funciones:

### Modelo de datos (primero — bloquea el resto)
1. **Rejilla de frames**: todas las operaciones cuantizadas a
   `round(t·fps)/fps`; timecode real del proyecto (no 25 fps clavado).
   Frame-accurate en TODO: el número del tooltip es el frame exacto donde
   corta el motor (QoL (E) — "el parece-roto más profundo").
2. **Pista de audio propia** en el esquema: clips de audio con
   `{file, in, out, start, gain, fades, banda}` — música bajo el vídeo
   (el JSON del webview ya la tiene: mismo formato).
3. **Undo/redo por gesto, 80 pasos** (un gesto = UN paso; cubre TODA
   mutación; snapshot en pointerdown solo si el gesto muta — trampa P0 ya
   resuelta en el webview, no repetirla).
4. Huecos como clips (`gap`, ya en el formato), fotos fijas con duración,
   **velocidad por clip** (el render ya la soporta vía shell).
5. **Marcas persistentes** con nombre + **rango I/O** de bobina
   (`markers`/`range` ya están en el project.json del webview).

### Gestos y dibujo
6. **Onda de audio DENTRO de la tira** de vídeo + ondas en la pista de
   audio. Sidecar de picos formato audiowaveform (min/max por bucket,
   multi-resolución) generado por la cocina; dibujado como quads.
7. **Banda elástica de volumen** en los clips de audio (puntos con alt,
   arrastrables) — el mínimo viable para "bajar la música cuando entra la
   voz".
8. **Fundidos**: asas en las esquinas de los clips de audio; en las juntas
   de vídeo, la cinta de empalme cicla 0/0.5/1/2 s + duración arbitraria
   arrastrando (el campo `fade` ya existe y el render lo honra; la GUI
   webview lo tenía — restaurar). Fundido desde/a negro en cabeza y cola.
9. **Arrastre con fantasma** semitransparente + **línea de inserción**
   (insert vs overwrite visualmente distintos); imán a las juntas con
   FLASH; snapping también al recortar (toggle N).
10. **Trim con tooltip vivo** (+00:12, nueva duración) + cursor corchete +
    **tope de material visible** (borde rojo/resistencia — jamás recortar
    más allá del material en silencio).
11. **Empalmadora en dos gestos**: primera B marca con lápiz graso,
    segunda B corta (hoy corta a la primera).
12. **Multi-selección** + mover en bloque; caja elástica en zona vacía;
    copiar/cortar/pegar/duplicar; menú contextual completo del clip.
13. Lift (dejar hueco) vs extract (ripple) — hoy solo ripple.
14. **Navegación**: ↑/↓ corte anterior/siguiente, Home/End, J/K/L
    lanzadera; zoom con rueda centrado en aguja/puntero, Shift+Z
    zoom-to-fit, auto-scroll siguiendo la aguja SIN pelearse con el scroll
    manual.
15. **Aguja con banderita** + regla a lápiz + rótulo a mano (Caveat) con
    el nombre de la bobina.
16. Esc cancela cualquier gesto en vuelo (ya) — extender a TODOS los
    gestos nuevos.

**Aceptación**: montar una pieza de 10 clips con música — cortar al ritmo,
fundidos, bajar la música con la banda elástica, mover en bloque,
deshacer 20 pasos — sin tocar el ratón más de lo que se tocaría en el
webview, y con imagen viva en todo momento.

---

## H1.5 — El visor completo

**Especificación**: `studio/js/viewer.js` (829) + `engine-decode.js`.

1. **Manivela con inercia** (el gesto insignia del taller) + su foley.
2. **J/K/L** con velocidades (×1 ×2 ×4, reversa) — la cabina necesita
   `Toca` con velocidad y el sonido, pitch/mute según modo.
3. **Lupa fina** (resolución completa bajo el cursor — el refinado ya
   trae el 4K; falta el recorte con zoom).
4. **Tira de prueba A/B** (W) — ya hay wipe básico; falta el "antes"
   congelado real y el deslizador.
5. **Scrub audible** (opt-in): trocitos de audio al arrastrar (el anillo
   de sonido ya existe; alimentarlo en seeks).
6. Loop, play in→out, play-around del corte (pre/post-roll), pantalla
   completa (Esc sale), timecode clicable para teclear e ir.
7. **Vúmetros** con peak-hold + clipping pegajoso; mute/solo.
8. Indicador de frames perdidos y de tier (proxy/máster) — hacer VISIBLE
   la política (petición del GENERALIZACION: proxies transparentes).

**Aceptación**: se puede montar a oído y a manivela, como en el webview,
pero a resolución completa.

---

## H1.6 — El cuarto oscuro completo (por clip)

**Especificación**: `studio/js/darkroom.js` (266) + `presets.js` +
`luts.js`. El modelo por-clip YA existe en nativo; falta la sala:

1. **Los 48 parámetros en galvanómetros** agrupados como en el webview
   (revelado, grano [9 mandos], halación [6], bloom [3], óptica [5],
   color/stock [7], viñeta [5], gate [6], encuadre [3]…) — con
   scrub-drag fino, doble clic para teclear, alt-clic para reset
   (convención AE; QoL).
2. **5 baños (presets)**: «saorín · revelado» (default de la casa),
   La Chimera S16, La Chimera Bolex, CineStill 800T, FX off — de
   `presets.js`, idénticos.
3. **5 stocks** (capas parciales sobre el baño): Kodak 50D/250D/500T,
   Fuji Eterna, CineStill 800 — ídem.
4. **El cajón de gelatinas**: LUT de entrada y LUT de color POR CLIP,
   selector con las del taller + **importar LUTs del usuario** (.cube;
   el lab legacy aceptaba hasta Hald) — la búsqueda tolerante a NFD/NFC
   ya existe.
5. **A/B wipe** dentro de la sala (comparar con/sin).
6. Copiar el cuarto oscuro de un clip a otro / a todos (el flujo real de
   trabajo); indicador de "este clip difiere del proyecto".

**Aceptación**: reproducir la receta del proyecto Filtración de memoria:
elegir baño, apilar stock, ajustar 6 mandos, con la imagen respondiendo en
vivo y por clip.

---

## H1.7 — La sala de revelado (render)

**Especificación**: `studio/js/revelado.js` (254) + el shell
(`server.rs`: la orquestación de piezas ya EXISTE y funciona — 413 fps
e2e). Estrategia: la nativa ORQUESTA vía `saorin cli render` (proceso de
fondo legítimo) y pinta el progreso; portar el motor de composición al
proceso nativo es Hito 2.

1. **Códecs de máster**: HEVC alta/media, ProRes 422 HQ, ProRes 4444 —
   selector como plantillas por objetivo (no por códec).
2. Apertura/cierre (fundido negro), **normalización de sonoridad**
   (loudnorm), export SOLO del rango marcado.
3. **Progreso con cubetas animadas** + % + ETA suavizada + cancelar +
   log accesible; `caffeinate`/`SetThreadExecutionState` durante el
   export; temp+rename (nunca un fichero a medias con nombre final);
   sin sobrescritura silenciosa.
4. **La cuerda de secado**: bobinas reveladas colgadas (miniatura +
   nombre + fecha), "mostrar en Finder", resumen post-export (duración,
   tamaño, bitrate, tiempo).
5. **Export incremental**: la caché de piezas por hash + piezas en
   paralelo YA están en el shell — asegurar que el JSON que emite la
   nativa produce las mismas claves de hash (misma identidad de pieza).
6. Fallo de export con motivo Y posición («falló en 00:12:04 — clip X»).

**Aceptación**: revelar la bobina de prueba dos veces cambiando UN corte:
la segunda pasada solo re-revela la pieza tocada; cancelar a medias no
deja basura; la bobina cuelga de la cuerda.

---

## H1.8 — Sonido fino

1. Mezcla real en la preview: vídeo + pista de música con ganancias,
   banda elástica y fundidos (hoy solo suena el clip de vídeo activo).
2. **Sin hueco en juntas**: pre-roll del audio del clip siguiente
   (el equivalente sonoro del pre-roll de decoders).
3. Ganancia y mute por clip (control directo en el clip — lección
   anti-Shotcut).
4. Detach/sustituir el audio de un clip.
5. Scrub audible (H1.5) y vúmetros (H1.5) comparten esta base.

---

## H1.9 — Robustez y QoL transversal (la lista (E) del GENERALIZACION)

No es una sala: es lo que hace que TODO lo anterior parezca sólido.

1. **Guardado atómico** (temp+fsync+rename — el webview ya) + backups
   rotatorios + validación de esquema al cargar + campo de versión.
2. Indicador de cambios sin guardar (punto en cerrar) + confirmación al
   cerrar + Cmd+S siempre funciona.
3. Crash de GPU/device-lost: recuperar la superficie con el proyecto
   intacto (el swapchain jamás se lleva la bobina por delante).
4. Detección de doble instancia (lock → enfocar la existente).
5. Asociación de fichero: doble clic en un proyecto abre la app.
6. Errores accionables SIEMPRE (fichero + motivo + siguiente paso).
7. Nunca un modal durante reproducción o arrastre.
8. VFR: detectar al importar y ofrecer conformar (modelo Shotcut).
9. Rotación de metadatos (displaymatrix) — los verticales de móvil.
10. Tooltips con atajo en cada control; atajos visibles.

---

## Orden de ataque propuesto (dependencias reales)

```
H1.0 fundamentos ──┬─▶ H1.2 estantería ──▶ H1.3 fuente
                   ├─▶ H1.1 salas/portada
                   └─▶ H1.4 bobina (modelo primero: pista audio,
                          rejilla frames, undo) ──▶ H1.8 sonido fino
H1.4 modelo ──▶ H1.6 cuarto oscuro (la sala; el motor ya está)
H1.4 + H1.6 ──▶ H1.7 revelado
H1.5 visor: incremental, en paralelo con H1.4
H1.9 QoL: transversal, un poco en cada sub-hito + pasada final
```

Pesos relativos (estimación honesta, por complejidad y líneas de la
especificación): H1.4 la bobina ≈ 30 %, H1.0 fundamentos ≈ 15 %,
H1.2 estantería ≈ 12 %, H1.6 cuarto oscuro ≈ 10 %, H1.7 revelado ≈ 10 %,
H1.5 visor ≈ 8 %, H1.8 sonido ≈ 8 %, H1.1 salas ≈ 4 %, H1.3 fuente ≈ 3 %.

**El criterio de aceptación GLOBAL del Hito 1** (del TRASPASO, literal):
la interfaz nativa es **1:1** con la del webview — la MISMA estética y
LAS MISMAS FUNCIONES — verificada pantalla a pantalla contra `studio/`
en las dos máquinas, sin perder ni un milisegundo de lo conseguido en el
Hito 0 (VELOCIDAD.md es un contrato: cualquier sub-hito que lo rompa se
revierte).

## Los sub-hitos de HERRAMIENTA.md (más allá de la paridad)

El autor señaló que ni el webview se sentía herramienta en proyectos,
aspecto o ajustes: `HERRAMIENTA.md` es la checklist de entregables que
completa esa sensación (proyectos como documentos, formato visible,
transform por clip, títulos, export por objetivo, preferencias,
confianza, onboarding). Sus §1–§2 se funden con H1.1 y el modelo de
H1.4; §9 con H1.7; §8 con H1.8; y añade H1.10 transform, H1.11 títulos,
H1.12 preferencias y H1.13 confianza. Leerlo JUNTO a este documento al
planificar cada sub-hito.

## Después (Hito 2, para no perderlo de vista)

- Retirar el webview y el servidor HTTP (queda `saorin cli` para agentes).
- Portar la composición del render al motor nativo (adiós a la pasada
  xfade; export de una sola generación).
- Zero-copy fast path, pre-roll de juntas, GOP-paralelo (VELOCIDAD §9).
- Tier «patata» y probe al primer arranque (GENERALIZACION).
