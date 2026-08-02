# filmlook-metal — Informe técnico
### Emulación fílmica 4K de calidad máster a 413–464 fps en Apple M4 Max (30-07-2026)

## Resumen ejecutivo

`filmlook-metal` es el renderizador nativo (Rust + Metal + VideoToolbox, cero
copias CPU) del laboratorio de look fílmico. Aplica el look completo del autor
—dos LUTs 3D encadenadas más un modelo procedural de película (grano, halación,
bloom, color subtractivo, etapa de positivado, obturador, gate weave…)— a vídeo
4K 10-bit, y lo hace **entre 7 y 8 veces más rápido que el tiempo real**.

Estado verificado sobre el clip de referencia (92,9 s de 4K 59.94 HEVC 10-bit,
5568 frames, cadena fílmica completa + audio):

| Salida | Tiempo de reloj | fps e2e | Máster |
|---|---|---|---|
| **ProRes 422 HQ** (2 motores) | **14,7 s** | **413** (464 en ráfagas) | 19,8 GB · 1,68 Gbps · 10-bit 4:2:2 |
| ProRes 4444 (2 motores) | ~17 s | ~440 (ráfaga 1920 fr) | ~2,2 Gbps · 12-bit 4:4:4:4 |
| HEVC 10-bit 40 Mbps | 29,5 s | 189 | 476 MB · Main 10 · techo del motor HW |

Como referencia de contexto: las cadenas profesionales de emulación fílmica
sobre las que se modela este look (DaVinci Resolve con grado + OFX de emulación
tipo Dehancer/FilmBox/Filmbox) suelen renderizar 4K **a tiempo real o por
debajo** en esta misma clase de hardware — decenas de fps. Este renderizador
ejecuta un modelo comparable en riqueza a **413 fps e2e**, con decode, encode y
mux incluidos, sin tocar la CPU con un solo píxel. En la combinación
calidad-del-modelo × velocidad, esto es estado del arte.

Al inicio de esta sesión el binario producía vídeo negro (con audio). Se
corrigieron cinco bugs reales (§4) y se rediseñó el camino de encode (§6).

---

## 1. Arquitectura del pipeline

```
mp4 → ffmpeg (demux annex-B, sin decode) ──┐
                                           ▼
                      VTDecompressionSession (HEVC HW, ASÍNCRONO)
                                           ▼   CVPixelBuffer x420 (IOSurface)
                      CVMetalTextureCache → MTLTextures Y/CbCr  (0 copias)
                                           ▼
             Cadena Metal (grade 2×LUT3D → shutter IIR → pirámide down/blur
             → comp fílmico → pack YUV 10-bit)          ~0,5 ms GPU/frame
                                           ▼   render directo a CVPixelBuffer x420
                                               del pool del encoder (0 copias)
        hilo de encode: espera GPU + submit round-robin a 2 VTCompressionSession
        ProRes (los 2 motores ProRes del M4 Max en paralelo)
                                           ▼   CMSampleBuffers comprimidos
        reordenación por PTS → AVAssetWriter passthrough → .mov final
        (audio del origen bombeado en vivo por AVAssetReader passthrough)
```

Principio rector: **los píxeles nunca existen en RAM de CPU**. El frame nace en
el decoder hardware dentro de una IOSurface, la GPU lo lee y lo escribe ahí
mismo, y el encoder hardware lo consume de la misma memoria unificada. Las
únicas transferencias son punteros.

---

## 2. El modelo de emulación fílmica

El look es un modelo físicamente motivado de la cadena negativo → positivado,
ejecutado en 12 pases de GPU por frame. No es "un LUT con grano encima": cada
etapa modela un mecanismo real de la película, y el orden de las etapas es el
orden físico del proceso fotoquímico.

### 2.1 Revelado del color: grade (1 pase, 2 render targets)

- **YUV→RGB BT.709** desde los planos del decoder (video-range 10-bit exacto:
  `(y·1023−64)/876`, croma `(c·1023−512)/896`), muestreo bilineal del croma
  4:2:0.
- **Exposición y push/pull**: ganancia en stops (`exp2`), y un remapeo de
  respuesta que emula forzar el revelado — el push abre gamma, levanta la
  niebla (mezcla hacia blanco) y el pull comprime, como en el laboratorio.
- **Compresión de altas luces ("esponja Dmax")**: por encima de un umbral la
  señal entra en una compresión racional `over/(1+k·over/wp)` — la saturación
  progresiva del negativo en vez del clip digital.
- **Dos LUTs 3D encadenadas**, interpolación trilineal manual de 8 taps sobre
  texturas 3D `RGBA32Float`:
  - **LUT A** — transformación de entrada (Insta360 I-Log → Rec.709 BT.1886,
    la LUT oficial de cámara de 65³ puntos);
  - **LUT B** — el grade creativo del autor, horneado en Resolve vía HaldCLUT
    a 65³. Es la firma de color: todo lo que Resolve hacía, capturado
    bit a bit.
- El pase emite **dos salidas MRT**: la imagen gradeada y la imagen "raw"
  (sin LUTs) para el wipe de comparación A/B del laboratorio.

### 2.2 Obturador: acumulador IIR temporal (1 pase)

Motion blur de obturador largo como en cámara de cine: un acumulador
`out = mix(frame, historia, feedback)` con reset en el primer frame. Es el
equivalente exponencial de un obturador de 180°+ y es el único estado
temporal de toda la cadena (una textura ping-pong).

### 2.3 Pirámide de luz difusa (9 pases pequeños)

Para halación, bloom y suavidad se construye una pirámide half-float:
downsample ½, ¼ y ⅛ (box de 5 taps) y **blur gaussiano separable** (9 taps
por eje) en cada nivel, con radios que crecen con el nivel — el nivel ¼ es el
núcleo de la halación y el ⅛ su falda ancha. Coste total ≈ 0,2 ms porque casi
todos los píxeles procesados son de baja resolución.

### 2.4 El pase de composición fílmica (1 über-shader)

Un solo fragment shader aplica, en orden físico:

1. **Gate weave**: traslación subpíxel + micro-rotación del frame con dos
   senos inconmensurables — el arrastre mecánico de la película en la
   ventanilla.
2. **Aberración cromática radial** (separación R/B dependiente del radio).
3. **Crosstalk de capas**: cada capa de emulsión contamina a las vecinas
   (R←G,B; G←R; B←G) ponderado por luminancia — el "glow" interno del color
   negativo.
4. **Hue-skew acoplado a luminancia**: campanas gaussianas en el círculo de
   tono desplazan cian→azul, verde→amarillo y rojo→naranja en altas luces, y
   magenta→rojo, azul→cian en sombras, **dejando estable la línea de piel
   (amarillo-naranja)**. Es la firma de color de stock más reconocible y la
   parte más delicada del modelo.
5. **Saturación subtractiva**: la película satura restando densidad, no
   sumando señal. El croma se pondera con una campana centrada en medios
   (satura más), se desatura en altas luces (los pasteles de las highlights),
   y el exceso de croma **oscurece** (`darken ∝ ‖croma‖`) — el
   comportamiento subtractivo que los modelos RGB aditivos no tienen.
6. **Etapa de positivado (print 2383)**: curva S de contraste, D-min frío en
   los negros (el soporte nunca es negro puro: azul-verdoso 0.012/0.016/0.020),
   calidez en altas luces, y un techo de gamut racional que comprime el croma
   extremo como la copia.
7. **Acutancia**: máscara de nitidez de alta frecuencia (imagen − blur ½) —
   el halo de borde del revelado (efecto de adyacencia químico).
8. **Suavidad/difusión**: mezcla con el nivel ¼ (filtro de difusión).
9. **Halación de dos lóbulos**: la luz que atraviesa la emulsión, rebota en
   el soporte y reexpone en rojo. Lóbulo interno (¼, núcleo naranja) +
   lóbulo externo (⅛, falda roja) con umbral suave sobre luminancia, tinte
   HSV dependiente del radio y mezcla *screen* — nunca satura por suma.
10. **Bloom** (velo atmosférico): promedio ½+¼ con umbral y tinte cálido,
    también en *screen*.
11. **Flicker y respiración**: parpadeo rápido de exposición (hash temporal)
    y un paseo aleatorio lento suavizado (smoothstep entre hashes) que
    modula exposición **y deriva CMY por canal** — la inestabilidad
    cromática de la copia antigua.
12. **Viñeta cos⁴**: la ley física del objetivo (`cos⁴θ`), con control de
    tamaño, redondez (mezcla círculo↔rectángulo) y centro.
13. **El grano** (§2.5).
14. **Polvo y rayas**: motas claras/oscuras y rayas verticales con vida por
    épocas (hash por época de medio segundo, densidad controlada).
15. **Ventanilla (film gate)**: SDF de rectángulo redondeado cuyo borde
    ondula con value-noise en ángulo polar — el fotograma con bordes
    imperfectos.

### 2.5 El modelo de grano (dentro del über-shader)

El grano es la parte más cara de imitar bien y la más visible de fallar:

- **Placa FFT tileable** de 1024² (f16, precomputada offline con espectro
  correcto) muestreada con offsets aleatorios por frame → estructura de
  "clumping" de baja frecuencia, sin patrones repetidos visibles.
- **Capa de células "cuánticas"**: `read()` (texelFetch) de la placa a paso
  entero con realce de contraste `sign(g)·|g|^0.65` → los cristales duros y
  nítidos de plata. La mezcla clump↔célula la controla la "rugosidad".
- **Asimetría negativo/positivo**: dos campos de grano distintos — el del
  negativo (fino, denso, modula medios y altas) y el del positivado (suave,
  modula sombras) — porque en la copia real el grano de sombras viene de la
  copia y el de luces del negativo.
- **Respuesta tonal por campanas**: tres campanas de luminancia (sombras
  0.12, medios 0.42, altas 0.85) con pesos independientes — el grano vive en
  los medios, casi desaparece en negros profundos y blancos quemados.
- **Grano por canal**: pesos R/B independientes (el azul siempre más
  granulado, como en el stock real) y un control de croma que mezcla grano
  monocromo ↔ RGB decorrelacionado.
- **Defocus del grano**: el LOD de mip de la placa se eleva con el control
  de desenfoque — grano suave de óptica difusa sin coste (el hardware de
  mips hace el trabajo; los mips se generan una vez al cargar).
- **Resolución de película**: el grano arrastra consigo una pérdida de
  resolución acoplada (mezcla con el nivel ½) — más grano ⇒ menos nitidez,
  como manda la química.
- Normalización por tamaño de célula (`rsqrt`) para que el control de tamaño
  no cambie la energía total.

### 2.6 Cuantización de salida

Pack RGB→YCbCr BT.709 video-range con las constantes exactas de 10 bits
(64+876·E, 512+896·C sobre 1023) escrito directamente en los dos planos
`x420` del encoder; el subsampling 4:2:0 del croma lo hace el filtrado
bilineal del render target a media resolución. Sin dither: el error de
cuantización a 10 bits queda muy por debajo del piso de grano.

Todo el estado artístico llega en un JSON de prefs — el mismo esquema que el
laboratorio Tauri/web, que sigue siendo la referencia visual con la que se
compara el render.

---

## 3. Por qué va tan rápido

La velocidad no viene de un truco sino de una disciplina: **medir cada etapa
por separado y no dejar que nada espere a nada**.

### 3.1 En la GPU (~0,5 ms de shader por frame 4K)

- **Über-shader**: todo el modelo fílmico (etapas 1–15) es *un* pase de
  fragmento. Un frame 4K toca la VRAM una vez para leer y una para escribir;
  la alternativa por nodos (una pasada por efecto, estilo editor) multiplica
  el tráfico de memoria por el número de efectos — en un chip de memoria
  unificada el ancho de banda ES el recurso escaso.
- **La luz difusa se computa donde vive**: halación/bloom/softness solo
  necesitan bajas frecuencias → pirámide ½/¼/⅛ con blurs separables. El 97 %
  de los píxeles de la pirámide son de baja resolución; el coste total de los
  9 pases es ~0,2 ms.
- **Half-float en toda la cadena intermedia** (`RGBA16Float`): la mitad de
  ancho de banda que float32 con margen dinámico de sobra para el modelo.
- **El grano no genera ruido, lo muestrea**: la placa FFT precomputada +
  offsets por frame convierten "generar grano correlacionado espacialmente"
  (caro) en dos-cuatro taps de textura (gratis). El defocus del grano usa el
  hardware de mips en vez de un blur.
- **LUTs como texturas 3D** con interpolación trilineal manual: 8 lecturas
  de una textura de 65³ que cabe entera en caché L2.
- **Cero pases de copia**: los planos del decoder y del encoder se envuelven
  como `MTLTexture` vía `CVMetalTextureCache` (IOSurface compartida). El
  primer y el último pase de la cadena leen/escriben directamente la memoria
  de los códecs hardware.

### 3.2 En el sistema (de 48 a 464 fps sin tocar el shader)

El shader ya costaba 0,5 ms cuando el e2e era de 48 fps: el problema era todo
lo demás. La arquitectura final es una **tubería de tres relojes
independientes** — decoder HW, GPU, encoders HW — donde la CPU solo dirige:

- **Decode asíncrono** (`kVTDecodeFrame_EnableAsynchronousDecompression`):
  el flag síncrono por defecto bloqueaba el hilo 3,6 ms/frame. Ahora el
  decoder corre por delante y el bucle consume de su cola.
- **Encoder en modo offline**: `RealTime=false` +
  `PrioritizeEncodingSpeedOverQuality=true` + `MaximizePowerEfficiency=false`
  cuadruplicaron el throughput HEVC (VT en modo realtime limita su cola
  interna para latencia — veneno para batch).
- **Hilo de encode dedicado** con canal acotado (8 frames ≈ 200 MB en
  vuelo): la espera de GPU y el submit a VT salen del bucle principal; el
  canal lleno ES la contrapresión, sin sincronización explícita.
- **Los dos motores ProRes en paralelo**: ProRes es all-intra ⇒ round-robin
  por frame entre dos `VTCompressionSession` sin restricciones de GOP, con
  reordenación por PTS en el callback. (Con HEVC esto NO funciona: medimos
  que el M4 Max solo expone un motor HEVC — dos sesiones se reparten los
  mismos ~250 fps.)
- **Mux sin recompresión y sin remux**: AVAssetWriter en passthrough recibe
  los CMSampleBuffers comprimidos tal cual, y el audio del origen entra en
  vivo por un AVAssetReader passthrough en su propio hilo. La versión
  anterior remuxaba el archivo terminado (copiar 20 GB) — eliminado.
- **Decisiones guiadas por medición**: cada cambio se aceptó o revirtió
  contra el desglose por etapa (`iter / decode-out / render / backpressure /
  gpu-wait / vt-submit`). Los dos hallazgos que definieron la estrategia
  (un solo motor HEVC; dos motores ProRes) salieron de un experimento de
  10 minutos con dos procesos en paralelo.

Presupuesto final por frame (422 HQ, clip completo):

| Etapa | ms/frame | |
|---|---|---|
| decode-out (espera cola del decoder) | 0,7 | solapado |
| render (submit Metal) | 0,1 | GPU real ~2,1 ms, pipelined |
| alloc pool + import planos | 0,01 | |
| contrapresión al hilo de encode | 1,3–1,6 | = ritmo del consumidor |
| hilo encode: gpu-wait / vt-submit | 2,1 / 0,02 | GPU es el cuello actual |
| **e2e** | **2,2–2,4** | **413–464 fps** |

---

## 4. Los cinco bugs que producían el vídeo negro (y demás corrupción)

### 4.1 El pase del obturador no dibujaba (→ NEGRO TOTAL)
`Renderer::render()` montaba el pase de acumulación temporal (shutter IIR):
pipeline, uniforms, texturas, sampler… y **nunca llamaba a `draw_primitives`**.
Con `shutter > 0` en las prefs (0.143 en las del autor), la cadena entera
componía desde `h_a`, una textura privada jamás escrita → negro puro
(Y=16, UV=128 exactos en el encoder: negro válido de video-range, por eso el
archivo "funcionaba" y tenía audio). *Detección*: `MTL_DEBUG_LAYER=1` →
`endEncoding without draw`.

### 4.2 VideoToolbox decodificaba a 'p420' empaquetado, no P010/x420
Sin `destinationImageBufferAttributes`, VT elige su formato preferido: en
macOS actual es `'p420'` (10 bits *empaquetados*, bpr 5120 = 3840·10/8·(4/3)),
no el biplanar de 16 bits que asumía la importación. Leer eso como R16Unorm
produce la imagen solarizada y duplicada al 2/3 que se veía. *Fix*: pedir
explícitamente `kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange` (`'x420'`,
MSB-aligned, bpr 7680) + IOSurface + MetalCompatibility. Nota: la constante
del repo decía `'p010'`, un fourcc de DXGI que en CoreVideo no existe.

### 4.3 El plano de croma se importaba como monocanal
`import_planes` creaba la textura del plano CbCr como `R16Unorm` (el docstring
ya decía `RG16Unorm`). El shader leía `.g` (Cr) = 0 constante → V = −0,57 →
verde/azul solarizado en todo el frame.

### 4.4 Flip vertical en cada pase (→ fantasmas y orientaciones mezcladas)
El full-screen-triangle usaba `uv = p·0,5+0,5` (convención GL). En Metal, NDC
+y es ARRIBA pero la fila 0 de una textura es ARRIBA → **cada pase invertía la
imagen**. Con un número impar de pases la salida quedaba derecha "por suerte",
pero la pirámide de blurs quedaba con niveles en orientaciones opuestas (la
halación muestreaba una copia invertida → fantasmas gigantes) y el acumulador
del shutter alternaba la orientación de su historia cada frame. *Fix*:
`uv = (p.x, −p.y)·0,5+0,5` — todos los pases preservan orientación.

### 4.5 Subidas de texturas rotas
- LUT 3D: `replace_region` en una textura 3D **exige `bytesPerImage`**; se
  pasaba 0 → basura a partir del primer slice. *Fix*: `replace_region_in_slice`
  con `n·n·16`.
- Grano: la textura declaraba 5 niveles de mip y solo se subía el nivel 0; el
  shader muestrea con `level(lod)` (defocus) → leía mips sin inicializar.
  *Fix*: `generate_mipmaps` con un blit encoder tras la subida.
- `fs_pack_uv` tenía un swap Cr/Cb de debug olvidado (`return (v, u, …)`).

Además se eliminó un segundo `MTLDevice`/`CommandQueue` y una segunda
compilación del MSL que el `Renderer` creaba sin necesidad, y el binding del
render target `raw` como textura de entrada del mismo pase (conflicto
señalado por la validación de Metal).

## 5. Correcciones de calidad de señal

- **Salida 10-bit real**: el pack RGB→YUV escribía NV12 8-bit (`'420v'`). Ahora
  el pool del encoder es `x420` (10-bit biplanar), los pases de pack renderizan
  a `R16Unorm`/`RG16Unorm` con las constantes de video-range de 10 bits
  (64/876/512/896 sobre 1023, no las de 8 bits), y la sesión HEVC lleva
  `ProfileLevel = HEVC_Main10_AutoLevel`. Verificado: `Main 10, yuv420p10le`.
  Nota: `x420` alinea el código de 10 bits en los bits altos del u16, así que
  escribir `código/1023` como unorm16 cae exactamente en los bits correctos.
- El formato nativo del encoder se consultó con
  `VTCompressionSessionGetPixelBufferPool` → coincide con `x420`: **no hay
  conversión oculta de píxeles** en el submit.
- BT.709 señalizado en la sesión VT, en los attachments del pool y en el mux.

## 6. La caza de los 350 fps: medición → decisión

| Paso | Cambio | fps e2e | Cuello siguiente |
|---|---|---|---|
| 0 | Bugs corregidos | 48 | `vt-submit` 6,8 ms |
| 1 | Encoder offline (`RealTime=false` + prioridad velocidad) | 164 | decode síncrono 3,6 ms |
| 2 | Decode asíncrono + bucle guiado por frames | 164→ solapa | `vt-submit` 4,0 ms (motor HEVC saturado) |
| 3 | 2 sesiones HEVC por GOPs cerrados (IDR cada 60) | **108** ❌ | — |
| 4 | Test decisivo: 2 procesos → **HEVC solo tiene un motor** | — | — |
| 5 | ProRes 4444 (bench sin mux) | 309 | serialización del bucle |
| 6 | Hilo de encode dedicado (canal acotado) | 327 | motor ProRes único ~330 |
| 7 | Test 2 procesos ProRes → **los 2 motores ProRes paralelizan** | — | — |
| 8 | 2 sesiones ProRes round-robin por frame + reorden PTS | 430 pico | E/S + remux final |
| 9 | AVAssetWriter directo + audio en vivo (sin remux) | **413–464** ✅ | GPU ~2,1 ms |

Datos del clip completo (5568 frames): 422 HQ **413 fps** · HEVC 10-bit
**189 fps** (vt-submit 4,98 ms/frame = motor saturado; el resto del pipeline
espera — es el techo físico de esa salida, no de este código).

Lecciones de hardware (M4 Max, medidas, no folleto):

- **Motor HEVC**: ~250 fps 4K10 con independencia del bitrate (5→40 Mbps
  idéntico). Dos sesiones o dos procesos se reparten esos 250.
- **Motores ProRes**: dos reales; con round-robin por frame dejan de ser el
  cuello (~440 fps conjuntos en 4444, >500 en 422 HQ).
- **El disco importa**: 4444 con grano ≈ 4,7 MB/frame; a 430 fps son ~2 GB/s
  sostenidos. El remux final (copiar 9–20 GB) era un impuesto del 30 % — fuera.
- **`RealTime=true` es veneno para offline.**
- **El decode síncrono era invisible** hasta desglosar el bucle.

## 7. Diseño del encode ProRes (lo no obvio)

- **Round-robin por frame** entre N=2 `VTCompressionSession` ProRes (all-intra
  ⇒ cualquier reparto es válido, sin trucos de GOP).
- Callbacks desordenados (dos sesiones) → **reordenación por PTS** (`BTreeMap`
  + cursor) antes de `appendSampleBuffer`.
- **AVAssetWriter en passthrough** (`outputSettings: nil` + `sourceFormatHint`
  del primer sample): muxa los CMSampleBuffers comprimidos sin recomprimir.
  Writer perezoso (se crea en el primer packet). `startSessionAtSourceTime:`
  recibe `CMTime` por valor → llamada cruda a `objc_msgSend` transmutada.
- **Audio en vivo**: `AVAssetReader` passthrough del AAC del origen → segundo
  `AVAssetWriterInput` en su hilo (con límite de duración si `--max-frames`).
- HEVC conserva su camino: segmentos-GOP cerrados reordenados → pipe
  `ffmpeg -f hevc` (+ audio).

## 8. Uso

```bash
cd film-look-lab/metal && cargo build --release

# máster ProRes 422 HQ (recomendado: >400 fps)
./target/release/filmlook-metal render "<in.mp4>" -o out.mov \
  --codec prores422hq \
  --lut-in "Luna_I-Log_to_Rec709_BT1886_s65_v2.cube" \
  --lut "pre 709 conversion 65 puntos - Cube_1.hald.cube" \
  --prefs ~/Downloads/filmlook-prefs.json

# --codec prores4444 · --codec hevc (Main10, --bitrate N)
# --max-frames N (recorta y limita el audio) · --bench (sin encoder)
# FL_DEBUG_DUMP=1 vuelca y/uv/graded/out del frame 2 a /tmp/fl_dbg_*
```

## 9. Resultados de validación

- Clip real 92,9 s 4K59.94 → **14,7 s** (413 fps e2e), máster 422 HQ 10-bit
  de 19,8 GB (~1,68 Gbps), audio AAC, **5568/5568 frames** verificados por
  decodificación completa.
- Mismo clip en HEVC 10-bit 40 Mbps → **29,5 s** (189 fps e2e), 476 MB,
  `Main 10 / yuv420p10le` verificado.
- Frames inspeccionados a 0 s / 3 s / 30 s: grade correcto (dos LUTs),
  halación/bloom/grano activos, orientación correcta, sin fantasmas del
  acumulador, motion blur del shutter coherente con el paneo.
- e2e reproducible: 1920 frames → 464 fps; 5568 frames → 413 fps (la
  diferencia es la E/S sostenida de ~1,7 Gbps y arranque/cierre amortizados).

## 10. Trabajo restante razonable

1. **GPU a <1 ms/frame** (hoy ~2,1 ms, el cuello actual): fusionar los pases
   de pack en el de comp (Y/CbCr como attachments MRT), revisar formatos
   intermedios (`RG11B10Float` donde el rango lo permita). Con eso el techo
   pasa a ser el decoder (~600 fps estimado).
2. **Decode GOP-paralelo** (2 sesiones VT sobre fronteras de GOP) si tras lo
   anterior el decoder pasa a ser el cuello.
3. Cola de trabajos + integración con el laboratorio (app Tauri) y con el
   pipeline de auto-davinci.
4. Windows/Linux: la cadena ya existe en WGSL (`core/`); faltaría D3D12/Vulkan
   video para el camino zero-copy equivalente.

## 11. Windows: el mismo look sobre una iGPU de bolsillo (Radeon 890M)

El motor Windows (`winlab/`) reproduce la cadena completa sobre wgpu/D3D12 con
decode y encode por hardware (VCN) vía Media Foundation y AMF. **Resultado:
86,7 fps e2e sostenidos** (clip completo de 5568 frames, 4K 59.94 → 4K AV1
10-bit CBR con audio) en un GPD Win Max 2 (Ryzen AI 9 HX 370, TDP ~28 W).
Partíamos de 23,8 fps con el camino de pipes: **3,6×**.

### El bug del vídeo verde: el driver descarta copias por plano

El síntoma: vídeo plano verde (Y=0, UV=0) con la cadena aparentemente sana.
La causa, cazada con lecturas de vuelta etapa a etapa: **el driver AMD D3D11
descarta EN SILENCIO cualquier `CopySubresourceRegion` entre un plano de P010
y una textura R16/RG16** — probado exhaustivamente: typed, typeless, shared,
plain, con caja parcial y sin caja, desde la textura del decoder y desde P010
propio. Ninguna combinación funciona y ninguna devuelve error.

Dos caminos que SÍ funcionan (ambos verificados bit a bit):
1. **Split/merge por pixel shader en D3D11**: un SRV `R16_UNORM` sobre la
   textura P010 expone el plano Y y uno `R16G16_UNORM` el UV (el mapeo por
   formato de los reproductores de vídeo); como RTV, lo mismo para escribir.
2. **Copias por plano en D3D12** (`CopyTextureRegion` con índice de
   subrecurso): otro camino de driver, y ahí sí son parte del contrato de
   copy-compatibility. Es el camino final: listas de comandos pregrabadas por
   slot (split entrada → cadena → merge salida) en la MISMA cola D3D12 de la
   cadena, con los devices D3D11 reducidos a un `CopyResource` (decoder) y al
   `SubmitInput` del AMF.

### Ingeniería del pipeline

- Interop triple: D3D11 (decoder MF con pool `BIND_SHADER_RESOURCE`) →
  D3D12/wgpu (cadena fílmica + copias planares) → D3D11 (AMF/VCN), cosido con
  fences compartidos NT; el hilo del encoder espera por CPU (a 8 frames de
  profundidad la espera es ~0).
- Los samples del decoder se retienen en un anillo hasta que su copia ha
  corrido en GPU (MF recicla las texturas del pool).
- Targets intermedios en `Rgb10a2Unorm` (post-LUT todo vive en display [0,1]
  y se entrega 10 bits), bind groups cacheados por slot/paridad.
- AV1 (VCN 5.0) es más rápido que HEVC (229 vs 197 fps el encoder solo);
  señalización BT.709 explícita en la cabecera AV1 (si no, sale PQ/BT.2020).

### El techo es térmico, no de software

Sin encoder el pipeline hace 150,6 fps; el encoder solo, 229. Juntos, 86,7:
con el VCN activo la cadencia de la cola 3D pasa de 6,6 a 11,7 ms/frame — el
TDP compartido y el timeslicing VCN↔3D del scheduler hardware. Se probó
profundidad 8, MFT asíncrono, CQP sin pre-análisis, tiles AV1 y device
compartido: todo converge a 85-90 fps. La pista restante sería AMF sobre
superficies DX12 nativas (menos colas que arbitrar).

---
*Medido en M4 Max, macOS, clip `VID_20260714_205527_099.mp4` (4K 59.94 HEVC
10-bit, 92,9 s, 5568 frames). Todas las cifras son e2e reales:
demux+decode+render+encode+mux, con el look completo activo (shutter, grano,
halación, bloom, hue-skew, subtractivo, print, weave, viñeta…).*
