# MOTOR — el revelado es un compositor, no una tubería

> Segunda versión. La primera proponía «arreglar ffmpeg y darle un rango
> al motor». Era un mal plan: aceptaba que el revelado fuese una tubería
> de ficheros con el motor de invitado. Este documento propone lo
> contrario — que **winlab y filmlook-metal hagan la bobina entera**, y
> que lo hagan con la cabeza puesta en dónde está el límite real: el
> **ancho de banda de memoria**, no el cómputo.

---

## 0. La tesis

Ya tenemos un compositor de timeline en GPU que hace todo esto a 50 fps
con material 4K: **es la preview**. Conforma al lienzo, encuadra por
clip, aplica una receta distinta a cada clip, cambia de fuente en las
juntas y dibuja a pantalla. Lo único que no hace es escribir un fichero.

El revelado, en cambio, monta una cadena de cinco procesos que se pasan
ficheros comprimidos. El motor zero-copy está ahí dentro, pero rodeado:
delante un ffmpeg que **decodifica 4K por software** (13 fps medidos), y
detrás otro que **re-encodea el máster entero** para poner los fundidos.

> **El revelado debe ser la preview corriendo a toda máquina.** No una
> tubería que casualmente incluye un motor rápido.

---

## 1. Dónde está el límite de verdad (números, no intuición)

Los intermedios de la cadena son `RGBA16Float` = **8 bytes por píxel**.
En 4K (8,29 Mpx) eso son **66 MB por buffer**. Contando lo que cada pase
lee y escribe por fotograma:

| pase | lee | escribe | tráfico |
|------|-----|---------|---------|
| grade | YUV 10-bit (~25 MB) + 2 LUTs | graded + raw (2 × 66) | ~158 MB |
| pirámide (down/blur ×3 niveles) | ~30 MB | ~30 MB | ~60 MB |
| comp | graded + raw + 3 blurs + grano (~180 MB) | máster (66) | ~246 MB |
| | | **total** | **≈ 465 MB/fotograma** |

A 90 fps eso es **≈ 42 GB/s solo en el look**. La 890M (LPDDR5X, bus de
128 bits) ronda los 120 GB/s teóricos y ~85 útiles, **compartidos con el
decodificador, el codificador y el sistema**. El M4 Max va sobrado; la
890M está justo en el filo.

**Conclusión que gobierna todo el diseño: no somos compute-bound, somos
bandwidth-bound.** Cada byte que no cruce la memoria es velocidad. Las
palancas, por orden de rentabilidad:

1. **menos bytes por píxel** (formato),
2. **menos pases** (fusión),
3. **menos píxeles** (resolución de trabajo donde no se note),
4. **no leer dos veces lo mismo** (tiles / memoria compartida).

---

## 2. Palanca 1 — el formato: la mitad del tráfico, gratis

`RGBA16Float` gasta 8 bytes; el alfa **no se usa** en ningún intermedio
salvo ProRes 4444.

- **`Rg11b10Float` (4 bytes)** para `graded`, `raw` y los niveles de la
  pirámide: mitad de tráfico, y su precisión (6 bits de mantisa, rango
  flotante completo) sobra para señal ya conformada en 0–1 con
  headroom. **Ahorro directo: ~45% del tráfico total.**
- La única salvedad: si un día hay canal alfa (títulos con
  transparencia, ProRes 4444), esos targets concretos vuelven a
  `Rgba16Float`. Es un `if` en la creación de la textura.
- El grano y el ruido pueden vivir en `R8Unorm` (1 byte).

Solo con esto, los ~465 MB/fotograma bajan a **~260 MB** y los 42 GB/s a
**~23 GB/s**. Es el cambio más barato del documento y el que más da.

## 3. Palanca 2 — fusión de pases: de cinco a dos

Hoy: `grade → down → blur → down → blur → down → blur → accum → comp`.
Cada flecha es un viaje completo a memoria.

**Diseño nuevo, dos dispatches de compute:**

**Pase A — «revelar y reducir» (compute, tiles de 16×16):**
lee YUV una vez, hace en registros: conform + encuadre + LUT de entrada
+ curva de revelado + color del stock + LUT de color. Escribe `graded`
(4 bytes) **y a la vez** emite el nivel 1/2 de la pirámide por reducción
en memoria compartida del grupo (`workgroupUniform`), sin releer nada.
Los niveles 1/4 y 1/8 salen de dispatches encadenados sobre datos ya
pequeños (baratos: 1/4 y 1/16 del tráfico).

**Pase B — «componer» (compute, tiles con halo):**
lee `graded` + los tres niveles de blur + la placa de grano y escribe el
fotograma final en el formato del encoder (P010). Halación, bloom,
viñeta, aberración, weave, dust, flicker y el frame se resuelven aquí,
en registros, sin intermedios.

> **Corrección al plan (comprobada en el código).** Aquí escribí que el
> grano se haría procedural «porque ya es un hash». Es falso:
> `tools/make_grain.py` sintetiza la placa **por FFT con fases
> aleatorias**, y su docstring dice por qué — periódica por construcción
> e **isótropa**, «sin banding axial/diagonal, el defecto de los hashes
> procedurales». Cambiarla por un hash sería empeorar el look para
> ahorrar 2 MB de una textura que vive en caché y no cruza la memoria en
> cada fotograma. **La placa se queda.** Lo que sí se hace es bajarla a
> `R8Unorm` (1 MB, mejor residencia en caché) si la comparación píxel a
> píxel lo permite.

De **~9 viajes a memoria a 2**. Combinado con el formato: **≈ 150 MB por
fotograma**, ~13 GB/s a 90 fps. Margen de sobra en la 890M.

> Nota de ingeniería: la pirámide con halo en LDS es lo único delicado
> (la halación tiene radio grande). Se resuelve con el patrón clásico de
> *separable blur en compute con halo de 8 píxeles por lado*, que a 1/4
> de resolución cabe holgado en 32 KB de memoria compartida.

## 4. Palanca 3 — trabajar en el espacio del vídeo

El decodificador entrega **P010 (4:2:0, 10 bits)**: la crominancia ya
viene a la mitad de resolución. Hoy la expandimos a RGB full-res y la
procesamos entera.

- **Grano, halación, bloom, viñeta y weave son fenómenos de luminancia.**
  Se pueden calcular en el plano Y a resolución completa y aplicar a
  croma con una corrección barata a media resolución. **Un tercio menos
  de píxeles procesados** en los pases caros.
- La salida al encoder es P010 de nuevo: si el pase B escribe
  directamente Y y CbCr en sus dos texturas, **desaparece la conversión
  final** (un pase entero menos).

## 5. Palanca 4 — el plan de bobina compilado

La CPU no debe decidir nada por fotograma. Antes de arrancar, se compila
la bobina a una **tabla plana** que se sube una vez como storage buffer:

```wgsl
struct Renglon {            // un fotograma de salida
  fuente_a: u32,            // índice de decoder
  fuente_b: u32,            // el otro lado del fundido (0xFFFF = ninguno)
  peso_b: f32,              // 0..1 — encadenado, a negro o a blanco
  receta: u32,              // índice en el array de recetas
  lut_in: u32, lut_col: u32,// índices en el ARRAY de LUTs 3D
  m0: vec4<f32>, m1: vec4<f32>, // matriz UV: conform + encuadre + flip
}
```

Consecuencias:

- **Las LUTs dejan de subirse por clip.** Todas las del proyecto viven
  en un `texture_3d_array` residente; cambiar de receta es cambiar un
  índice. (En la preview esto costaba 750 ms por fotograma cuando se
  recompilaba — el bug de ayer. Aquí, cero.)
- **Las recetas son un array de uniforms**, no un rebind.
- **Cero ramas por clip** en el shader: todo es indexación.
- El fundido no es una fase: es `mix(a, b, peso_b)` dentro del pase A.
- Fotos, títulos y huecos son *fuentes sintéticas*: una textura
  residente o un color constante. No pasan por decode ni por ffmpeg —
  y de paso muere el bug de las piezas mudas.

## 5bis. Cortes, juntas y transiciones (la duda que ordena el diseño)

Esta es la parte que en la tubería de hoy cuesta **una pasada entera del
máster** (`xfade` sobre todas las piezas). En el compositor cuesta,
literalmente, un `mix`. Merece detalle porque decide dónde va el `mix`.

### El corte seco no existe como operación

En el plan compilado, un corte es que el renglón `t` apunte a la fuente
A y el renglón `t+1` apunte a la B. **Cero coste**: ni pase extra, ni
re-encode, ni concat, ni fichero intermedio. La bobina no se «pega»
nunca porque nunca se despedaza.

### Dónde se mezcla: antes del comp, no después

Un encadenado necesita las dos imágenes con **su propia receta** (cada
clip tiene su cuarto oscuro). La tentación es revelar A y B enteros y
mezclar al final: eso duplica la cadena completa en cada junta.

La respuesta buena sale de mirar cómo se hace en un laboratorio de
verdad: **un encadenado óptico es una doble exposición sobre el mismo
negativo, y el grano y la halación aparecen DESPUÉS, al revelar la
copia mezclada.** Así que el `mix` va justo tras el grade:

```
   pase A (barato):   grade(A, receta_A) ─┐
                      grade(B, receta_B) ─┴─► mix(peso) ──► graded
   pase B (caro):     pirámide + halación + bloom + grano + óptica  ← UNA vez
```

- El sobrecoste de una junta es **solo el pase A de la segunda fuente**
  (conform + encuadre + dos LUTs: el barato de los dos).
- El pase caro —el que mueve la pirámide y el grano— **se hace una sola
  vez** sobre la imagen ya mezclada.
- Y es **más fiel**: el grano no se duplica en la transición (que es lo
  que pasa al mezclar dos imágenes ya granuladas, y se nota como un
  «hervor» en mitad del encadenado).

Optimización y verdad fotográfica coinciden. Cuando eso pasa, la
decisión está tomada.

### Fundido a negro y a blanco: coste cero

No necesitan segunda fuente ni segundo decodificador: son `mix` contra
una constante. El renglón lleva `fuente_b = NINGUNA` y un color.

### Otras transiciones, gratis

Cualquier transición es el mismo `mix` con un peso **por píxel** en vez
de constante: barrido (peso = función de x), cortinilla, iris, mancha de
revelador. Añadir una transición nueva es escribir una función de peso,
no una fase de pipeline. (No entran ahora — pero el diseño no las cierra.)

### El pico de las juntas, medido y acotado

Durante un encadenado hay **dos decodificadores vivos**. Con el decode a
~96 fps por fuente y un consumo de 90 fps, en esa ventana el decode es
el límite: la bobina baja a ~48–60 fps *durante el fundido*.

Hagamos el cálculo honesto: una bobina de 5 minutos con 10 encadenados
de 1 s son **10 s a media velocidad** de 300 s totales. El e2e cae de 90
a **≈87 fps**. Es ruido.

Aun así, dos mitigaciones baratas:

1. **Precarga del entrante** ~16 fotogramas antes de la junta (≈200 MB
   en 4K P010). Cubre el arranque del GOP —que es el coste real de
   empezar un clip— con tiempo que de todos modos sobra.
2. **El arranque del GOP no se procesa**: los fotogramas entre el
   keyframe anterior y el punto de entrada se decodifican y se tiran
   sin tocar la GPU. Solapados con el clip saliente, salen gratis.

No hay que hacer nada más: perseguir ese pico con precargas de 1 GB
sería gastar memoria para ganar 3 fps.

### Frame-exactitud sin re-encode

El plan se compila **en la rejilla de fotogramas del proyecto**: cada
renglón sabe el PTS exacto que le toca a cada fuente. El decodificador
busca el keyframe anterior y descarta por PTS. No hay deriva acumulada
—que era justo el motivo por el que existía el corte con re-encode.

## 6. Palanca 5 — I/O como productor/consumidor, no como turnos

- **Decoders concurrentes**: un decoder por fuente viva (2 durante un
  fundido) + precarga del siguiente clip N fotogramas antes de la junta.
  MF y VideoToolbox admiten varias sesiones sin problema.
- **Frame-exactitud sin re-encode**: seek al keyframe anterior y
  descartar por PTS — los frames descartados **no se procesan** (se
  liberan sin tocar la GPU). Es lo que ya hace `cine.rs` en la preview.
- **Triple buffering explícito**: mientras el encoder mastica el
  fotograma N, la GPU compone el N+1 y los decoders sacan el N+2. Un
  anillo de texturas persistente (nada de allocar por fotograma).
- **Encode con cola profunda** (ya es async): que el codificador nunca
  espere.
- **Zero-copy de verdad**: samplear la textura del decodificador
  directamente (SRV sobre el plano NV12/P010 en D3D11; `MTLTexture` de
  la `CVPixelBuffer` en Metal). Cero blits de entrada.

## 7. Palanca 6 — atajos que valen oro

- **Identidad detectada**: si la receta es «FX off», no hay conform, no
  hay encuadre y el códec de salida coincide con el de entrada →
  **remux sin tocar píxeles**. Revelar es copiar.
- **Solo-corte**: sin fundidos ni transform, con la misma receta →
  concatenación por GOP; solo se re-codifican los GOP de los bordes.
- **Caché por (clip, receta, rango)**: hoy existe por pieza; con el
  plan compilado se puede afinar a nivel de GOP. Cambiar el grade de un
  clip no debería recalcular la bobina entera.
- **Revelar solo el tramo marcado**: recortar la tabla, nada más.

## 8. La palanca que lo cambia todo — un solo look

Hoy la cadena fílmica está escrita **tres veces**: `core/src/shaders/*.wgsl`
(preview), `winlab/src/chain.rs` (HLSL/D3D11) y `metal/src/shaders/chain.metal`.
Tres sitios donde arreglar el mismo bug, tres sitios donde divergir. Y
la promesa de la casa es *preview = export*.

**Propuesta: la cadena vive UNA vez, en WGSL, y corre en los tres sitios
sobre wgpu** (que traduce a Metal, D3D12 y Vulkan). winlab y
filmlook-metal dejan de ser motores y pasan a ser lo que de verdad
aportan: **backends de entrada/salida por hardware** (MF/AMF, VT).

```
   decode HW  ──►  interop  ──►  [ cadena WGSL única ]  ──►  interop  ──►  encode HW
   (MF / VT)      (textura       core/src/shaders/*        (textura       (AMF / VT)
                   compartida)                              compartida)
```

- La interop es el único punto delicado: en Windows, textura compartida
  D3D11↔D3D12 con `KeyedMutex` (winlab **ya usa `shared_tex`**, o sea
  que el patrón está probado en casa); en Mac, `MTLTexture` sobre
  `IOSurface`, que es lo que VideoToolbox entrega de forma natural.
- Beneficio inmediato: cualquier mejora del look (o de estas palancas)
  aparece a la vez en la preview y en el máster, en las dos plataformas.
- Y el revelado hereda gratis todo lo que la preview ya sabe hacer.

**Si la interop diera guerra**, el plan B es barato: mantener los tres
backends pero **generar** el HLSL y el Metal desde el WGSL con `naga`
(que ya es una dependencia de wgpu) en el build. Sigue habiendo una
sola fuente de verdad.

---

## 8bis. Los presets: solo lo que vuela (y en cada máquina, el suyo)

Hoy la sala ofrece cuatro presets y tres de ellos son el mismo códec con
matices. Peor: ofrecen caminos que en una de las dos máquinas **no están
acelerados** y por tanto son trampas de velocidad.

Lo que de verdad tiene motor de hardware:

| camino | Mac (M4 Max) | Windows (890M) |
|--------|--------------|----------------|
| HEVC 10-bit | VideoToolbox ✔ | AMF ✔ |
| ProRes 422 HQ / 4444 | VideoToolbox ✔ (**dos motores en paralelo**) | ✗ **software** (10× más lento) |
| H.264 | ✔ pero 8 bits: pierde el 10-bit del look | ✔ mismo problema |
| AV1 | ✗ | ✔ pero sin ventaja aquí |

**La regla: si en esta máquina no lo hace el chip, no aparece en la
sala.** Un menú que ofrece un camino lento es un menú que miente.

### La sala queda así

- **REVELAR** — HEVC 10-bit por hardware, al formato de la bobina. Es el
  camino medido y el único que existe en las dos máquinas. *Este es el
  botón.*
- **ARCHIVO (ProRes HQ)** — **solo visible en el Mac**, donde tiene dos
  motores dedicados y va aún más rápido que el HEVC. En Windows este
  preset no se dibuja.
- Una casilla: **normalizar el sonido** (loudnorm). No toca el vídeo, no
  cuesta velocidad.

Y desaparecen, de momento:

- «Compartir rápido» y «Solo revelar» (eran el mismo HEVC con otro
  nombre y otra normalización);
- **cualquier elección de resolución o formato de salida**: el máster
  sale al lienzo de la bobina y punto. La resolución se decide al crear
  la bobina, que es donde tiene sentido.

Cuando el motor esté terminado y medido, añadir un camino nuevo será
trivial: es un códec de salida, no una tubería distinta. **Primero
velocidad demostrada, después catálogo.**

### 8ter. EL CAJÓN DEL MÁSTER (1-ago-2026): la promesa, cumplida

El motor está terminado y medido, así que toca abrir el catálogo. Pero **la
regla vieja se queda a medias a propósito**: no vuelve el menú que ofrece
caminos lentos como si fueran gratis. Vuelve en dos alturas.

- **Los sellos de la sala** siguen siendo lo que vuela en ESTA máquina, y
  siguen siendo *el botón*. En Windows sigue sin haber sello de ProRes.
- **El cajón** (`prefs.json` → `master`) es para cuando manda el destino y da
  igual lo que tarde. Se abre a mano, dice lo que cuesta cada cosa y **lo que
  va por software lo dice**. El autor decide si le compensa esperar; lo que no
  puede pasar es que espere sin saber por qué.

**Y VAN SEPARADOS, que es lo importante.** Hay TRES sellos, y solo el tercero
mira el cajón:

    REVELAR   hevc 10 bits · el motor del chip · LO MÁS RÁPIDO
    ARCHIVO   prores hq · dos motores en paralelo          (solo en el Mac)
    A MANO    lo que diga el cajón · puede tardar mucho más

Los dos primeros **no leen el cajón para nada**: al lienzo de la bobina, sin
escalar, con el motor del chip. Pase lo que pase.

No es un detalle de interfaz, es la trampa que se evita: los ajustes del cajón
**persisten**. Si el camino rápido y el ajuste raro compartieran botón, un día
se quedaría puesto un «8K ×2» y el botón de siempre tardaría diez veces más sin
haber avisado de nada — y encima el autor pensaría que el motor ha empeorado.

Se comprueba en un minuto: con el cajón guardado en 8K ×2, tocar REVELAR saca
**1080×1920** y el parte de la sala dice «el lienzo de la bobina · directo, sin
escalar». El botón del cajón, mientras tanto, se lee **«EL CAJÓN (sin usar)»**
en tinta, no en rojo. Solo cuando el sello es A MANO se pone rojo y dice
**«(en uso)»**.

Y tocar cualquier fila del cajón elige A MANO sola: si cambias un ajuste, es
que lo quieres usar.

Dentro del cajón hay cinco cosas y **dos números que lo explican todo**:

| | |
|---|---|
| **sale a** | del lienzo · 720p · 1080p · 1440p · 4K · 8K — el alto; el ancho sale de la **proporción de la bobina**, que no se toca nunca (el formato es la decisión creativa y se tomó al cortarla) |
| **se revela a** | ×0,5 · ×1 · ×1,5 · ×2 — a qué escala corre la CADENA respecto a lo que sale |
| **códec** | HEVC 10 · H.264 8 · ProRes 422 HQ · ProRes 4444 · HEVC x265 (software) |
| **caudal** | 20 · 40 · 60 · 150 · 400 Mb/s, entendidos **para 1080** |
| **al escalar** | el que toque · nítido (lanczos) · suave (area) |

Las cuatro combinaciones que importan, y lo que hace cada una:

    sale a       se revela a    qué pasa
    del lienzo   ×1             el camino de siempre. CERO pases de más.
    8K           ×1             el look se calcula A 8K y se codifica a 8K.
                                Sin escalado y sin generación extra: es el
                                mejor 8K. Medido: 21 fps en el M4 Max.
    8K           ×0,5           se revela a 4K y se agranda. El grano del 4K,
                                más gordo — el que mejor aguanta el
                                recompresor de la plataforma.
    1080p        ×2             SUPERMUESTREO: la cadena corre a 4K y se
                                reduce con promedio de caja. Bordes y grano
                                sin escalones.

**Por qué un 8K de crudos 4K no es un capricho.** Las plataformas reparten el
caudal por escalón de resolución: un 8K recibe varias veces más bits que un
4K del mismo metraje. El grano y la halación son ruido de alta frecuencia —
justo lo primero que se come un recompresor—, así que subir el mismo plano en
8K es la forma barata de que el look llegue entero al otro lado.

**Lo que cuesta, dicho en la sala.** El panel enseña «el revelado mueve 3,6×
los píxeles: tardará ~3,6× más» en rojo. Y el caudal **escala con los
píxeles** (`base × (px/1080p)^0,8`): con un caudal fijo, un máster de 8K
saldría peor que el de 1080, que es justo lo contrario de para lo que se pide.

**El escalado no cuesta una generación de más** cuando ya había que tocar el
máster: la pasada que mezcla el sonido es la misma que escala. Solo se paga
sola cuando no hay ni música ni normalización.

Medido en el M4 Max, un segundo de bobina 9:16:

    del lienzo ×1   1080×1920 ·  52 Mb/s ·  6,5 MB
    8K ×0,5         7680×4320 · 539 Mb/s · 67,4 MB   (se revela a 4K y se agranda)
    8K ×1           7680×4320 · 549 Mb/s            (la cadena entera a 8K, 21 fps)
    1080p ×2        1920×1080 ·  50 Mb/s ·  6,2 MB   (supermuestreo)

El tope es 8192 por lado: por encima no hay motor de hardware ni en
VideoToolbox ni en AMF. Se recorta y se avisa, en vez de fallar al final.

## 9. Plan de obra (cada hito entrega y se mide)

> **Estado real tras la sesión de obra**, medido en las dos máquinas.
> M4 Max, 20 s de 4K60: **13,5 s → 7,1 s** (0,7 s con caché).
> Radeon 890M, la bobina «prueba2» de 44 s: **50,9 s → 17,5 s** (1,3 s con
> caché). Con encadenado, dos clips: **26,4 s → 10,4 s**.

| hito | qué | estado |
|------|-----|--------|
| **H-1** | podar la sala | ✅ hecho y verificado en la app |
| **H0** | hwaccel + lienzo del proyecto + conform que no hace nada | ✅ |
| **H1** | dieta del formato | ✅ pero **con otro formato** del previsto (§9bis) |
| **H2** | fusión de pases | ◐ dos pases fuera; la fusión a compute **rechazada con datos de las dos máquinas** (§9bis): el look es 0,1–1,2 ms de 6–35; manda el codificador |
| **H3** | plan de bobina compilado | ✅ **las dos máquinas**, medido: Mac 1,9× · 890M 2,5–2,9× |
| **H4** | anillo y solape | ✅ Mac |
| **H5** | un solo look | ✅ **el autor eligió el del taller** (1-ago). `build.rs` traduce con naga los cinco shaders del WGSL a Metal; preview, máster del Mac y máster de Windows son ya la misma cadena. `FL_LOOK=msl` recupera la vieja |
| **H6** | atajos | ✅ caché de bobina, **caché fina por tramos** e identidad→remux |

El plan original, para contrastar:

| hito | qué | resultado esperado (890M, 4K60) |
|------|-----|-------------------------------|
| **H-1** · minutos | **podar la sala**: un solo camino por máquina (§8bis) | deja de ofrecerse lo que no vuela |
| **H0** · horas | `-hwaccel` en el corte + mandar `project{w,h,fps}` + saltar conform si ya coincide | 13 → **≥45 fps** |
| **H1** · 1–2 d | formato `Rg11b10Float` en los intermedios (la placa de grano se queda, ver §3) | **+40%** sobre H0 |
| **H2** · 2–3 d | fusión a dos dispatches de compute (A: revelar+reducir, B: componer) | el look deja de ser el límite |
| **H3** · 2–3 d | plan de bobina compilado + LUTs en array + fuentes sintéticas + **juntas y fundidos como `mix` tras el grade** (§5bis) | **desaparecen el corte Y la fase de fundidos** |
| **H4** · 2–3 d | decoders concurrentes, anillo de texturas, triple buffering, zero-copy de entrada | **≥90 fps e2e** |
| **H5** · 3–5 d | unificación en WGSL sobre wgpu (o naga → HLSL/Metal) | un solo look; preview = export por construcción |
| **H6** · 1–2 d | atajos: identidad→remux, solo-corte→GOP, caché por GOP | revelados triviales *instantáneos* |

**Contrato de medición.** El diario del revelado (ya existe) informa por
fase; se le añade el desglose que dice quién manda:

```
⟨ 29.4s⟩ MÁSTER: 2650 fotogramas en 29.4 s → 90 fps (4K60 · HEVC 10-bit)
         decode 96 · componer 140 · encode 94 fps   · GB/s 12.8
```

Ninguna etapa se da por buena sin medirla **en las dos máquinas**, y la
cifra queda en el repo.

---

## 9bis. LO MEDIDO (bitácora de la obra)

Todo en el MacBook M4 Max con `/tmp/taller/media/source.mp4` (HEVC 4K
10 bits, 59,94 fps). Las cifras de la 890M quedan pendientes: el GPD no
estaba accesible durante esta sesión y **ninguna afirmación sobre
Windows se da por buena sin medirla allí**.

### Dos fallos de corrección que aparecieron por el camino

Estaban ahí antes de tocar nada y salieron al montar el banco de
aceptación (revelar dos veces y comparar píxel a píxel — tenía que dar
infinito y daba 28 dB):

1. **El máster salía con fotogramas de otro sitio.** El motor soltaba el
   `CVPixelBuffer` de entrada nada más encolar el trabajo, sin esperar a
   que la GPU lo hubiese leído. VideoToolbox lo reciclaba para un
   fotograma posterior y el look revelaba encima. A partir del séptimo
   fotograma el máster divergía, **y distinto en cada revelado**. Ahora el
   fotograma de entrada viaja con el trabajo y se suelta cuando la GPU
   termina. (Windows ya lo hacía bien: copia al anillo con valla.)
2. **El sello de tiempo no llegaba a la muestra.** `CMSampleBufferCreateReady`
   se llamaba con cero entradas de tiempo, así que el `pts` del callback
   era inventado. Además cada NAL suelta —incluidas SEI y delimitadores—
   se mandaba como si fuera un fotograma. Corregido: unidades de acceso
   completas y sello real, más un búfer de reordenación.

Con eso, dos revelados idénticos dan **24/24 fotogramas iguales al bit**.

### H1 — la palanca del formato: el plan se equivocaba de formato

| intermedios | tráfico | contra la referencia |
|---|---|---|
| `RGBA16Float` (lo que había) | 8 B/px | — |
| `RG11B10Float` (lo que proponía §2) | 4 B/px | **43 dB** ✗ |
| ídem, solo en la pirámide | | **54 dB** ✗ |
| **`RGB10A2Unorm`** | 4 B/px | **59,7 dB** ✓ (~1 valor de código) |

`RG11B10Float` tiene 6 bits de mantisa: ~1,6 % de error relativo, unos 16
valores de código sobre 1023. No sirve ni para la pirámide. Después de
las LUT toda la señal vive en [0,1], así que **10 bits sin signo** es el
formato correcto — y es el que **winlab ya usaba**: los dos motores
llevaban tiempo produciendo másteres distintos. Igualarlos quita una
divergencia (§8) en vez de crearla.

**Lo que de verdad pagó en H1 no fue el formato**: el pase de revelado
escribía un segundo destino, `raw`, de resolución completa, que **solo lee
la cortinilla del comparador de la preview**. En el máster se escribía y
no lo leía nadie: 66 MB por fotograma en 4K, un 14 % del tráfico, y
quitarlo es exacto al bit.

### H3 — el motor lee la bobina

| | camino viejo | motor de bobina |
|---|---|---|
| procesos | 5 (corte · look · fundidos · concat · mezcla) | 2 (bobina · sonido) |
| compresiones del material | 3 | 1 |
| 20 s de 4K60, e2e | **13,5 s** | **8,6 s** |
| el revelado en sí | 89 fps | **169 fps** |

Verificado además: **corte frame-exacto** (pedido t=10,0 s → el motor
sirve el fotograma 599, que es exactamente el que cubre ese instante,
comprobado por correlación contra los once candidatos vecinos) y el
**encadenado** como doble exposición con el revelado aplicado una sola
vez encima.

Sigue cayendo al camino de siempre —sin perder nada— cuando la bobina
trae fotos fijas o rótulos, que aún no sabe fabricar por su cuenta, y en
Windows.

### La 890M, por fin medida (bobina «prueba2», 44,2 s de 4K)

El GPD volvió a estar accesible y se compiló y probó todo allí. La bobina es
la del taller real: un clip de Insta360 Luna Ultra, HEVC 4K 10 bits a 59,94,
revelado a un lienzo de **3840×2160 a 25 fps**.

| | e2e |
|---|---|
| camino viejo (ffmpeg corta, motor revela) | **50,9 s** |
| corte dentro del motor | **17,5 s** |
| con la pieza en caché | **1,3 s** |

**2,9× más rápido**, y la fase que desaparece es justo la que dolía: el corte
con ffmpeg iba a **35 fps** (decodificar 4K por software y re-codificarlo
entero antes de tocarlo).

A cadencia nativa (59,94 → 59,94, 2650 fotogramas) el motor sostiene
**73 fps e2e** con el look puesto. El techo es el codificador AMF
(`gpu-wait 16,5 ms/fotograma`), no la cadena fílmica — igual que en el Mac,
donde el techo es VideoToolbox. Los ~90 fps que esperábamos son alcanzables
en 4K solo si el codificador deja de ser el límite; el look ya no lo es en
ninguna de las dos máquinas.

### Y la bobina entera, también en Windows

Con el GPD delante ya no había excusa para dejarla fuera. `winlab bobina`
existe y hace lo mismo que el motor del Mac: recorre la tabla de renglones,
corta, conforma, convierte la cadencia, aplica la receta de cada clip y
resuelve las juntas con **un segundo dibujo encima** — el lado B se
decodifica en su propio carril de huecos y se dibuja con su peso sobre el
lienzo, y el pase caro corre una sola vez después, sobre la mezcla.

Medido con dos clips de 6 s y un encadenado de 1 s (4K → 25 fps):

| | e2e |
|---|---|
| camino viejo (corte · look · fundidos · concat) | **26,4 s** |
| bobina de un tirón | **10,4 s** |

**2,5×**, y ahí sí desaparecen las cuatro fases de golpe. Verificado
visualmente contra un corte seco en los mismos fotogramas: con `fade=0` los
tres fotogramas de la junta son idénticos (todos del clip que se va) y con
`fade=1` se ve la disolvencia progresiva — la silueta de la roca
desapareciendo mientras entra el agua.

Lo que **no** hace todavía en Windows: bobinas que mezclen tamaños de
fuente (el anillo de entrada se dimensiona con la primera y las demás tienen
que coincidir). Si pasa, el motor lo dice y el taller cae al camino de
siempre.

**Y un fallo que solo se ve con la máquina delante**: la conversión de
cadencia. Con `--desde/--cuantos` el motor leía **de corrido**, así que una
bobina a 25 con material a 59,94 salía estampada a 59,94 y con solo los
primeros 18 s del clip. Ahora el paso del proyecto viaja en `--fps` y, si no
coincide con el de la fuente, el motor sirve **el fotograma que cubre cada
instante del máster** (que es la conversión de cadencia) en vez de leer
seguido. Verificado: 1105 fotogramas, 25/1, 44,22 s, con sonido.

Otros tres que cazó el GPD y no el Mac:

- **El uniforme del revelado son 128 bytes en Windows, no 112.** `GradeU`
  lleva allí una fila más (`zoom/enc_cx/enc_cy`) que la del Mac. Con 112
  wgpu rechaza el pipeline entero y el motor no arranca. Es exactamente la
  clase de divergencia que §8 quiere quitar: dos structs con el mismo
  nombre y distinto tamaño en cada máquina.
- **ffmpeg es posicional.** El recorte del sonido tiene que ir pegado a su
  `-i`; puesto detrás de `-c:a aac -b:a 256k`, ffmpeg toma el bitrate como
  opción de *esa entrada* y aborta el mux.
- **`CompU` son 240 bytes, no 236.** WGSL redondea el tamaño de la
  estructura a múltiplo de 16 y exige que el búfer llegue ahí. Con 59 `f32`
  wgpu rechaza el grupo de enlace («Binding size 236 … less than minimum
  240»). Hacen falta 60 exactos.

En los dos casos **el taller cayó solo al camino de siempre** y el revelado
terminó igualmente, que era justo para lo que estaba puesta la caída.

### H2 — el look no era el límite (y eso cambia el plan)

El documento entero se apoyaba en §1: «somos bandwidth-bound, el look es el
cuello». **Medido en el M4 Max, es falso.** Con la cadena en solitario
(`--bench`, sin codificador):

```
✅ 400 fotogramas en 0,7 s = 541 fps
   decodificar 0,9 · REVELAR 0,1 · contrapresión 0,9 ms/fotograma
```

El look cuesta **0,1 ms de 6**. En la bobina completa: esperando a las
fuentes 2,2 · **componer 0,26** · esperando al codificador 2,9. Los
límites son los códecs del chip, no la cadena fílmica.

Así que la fusión completa a dos *dispatches* de compute —una reescritura
grande del look, con riesgo de cambiarlo— **no se hace a ciegas**: sería
optimizar el 4 % del fotograma sin poder medir la máquina donde
supuestamente importa. Queda pendiente de las cifras de la 890M.

Lo que sí se hizo, porque es barato y además **acerca los dos motores**:

- **`raw` fuera** (§H1): un destino de resolución completa por fotograma.
- **El obturador, fundido en el revelado.** Era un pase aparte que leía
  `graded` y escribía la historia: otros 66 MB de ida y vuelta en 4K para
  una cuenta de una línea. Windows ya lo hacía así. Ahora los dos igual.
  Y encaja con las juntas sin cuidado especial porque el filtro es lineal:
  `mix(mix(A,h,k), mix(B,h,k), p) == mix(mix(A,B,p), h, k)`.
  Medido contra la referencia: **62 dB**, y es *más* preciso que antes
  (la mezcla ocurre ahora antes de cuantizar a 10 bits, no después).

De los ~9 viajes a memoria por fotograma, **quedan 7**. Los otros dos se
quitaron sin tocar una línea del look.

### H4 — el anillo, dimensionado por bytes

Decodificar y componer se turnaban. Con un hilo propio para las fuentes y
una cola acotada, medido sobre la misma bobina 4K con encadenado:

| fotogramas por delante | 6 | 12 | 24 | 32 | 48 | 64 |
|---|---|---|---|---|---|---|
| fps | 157 | 155 | 162 | 174 | 176 | 176 |

Cuanto más hondo, menos espera al codificador… y más espera a las fuentes:
a partir de 48 el decodificador es el único límite. Pero un fotograma 4K en
x420 son 25 MB, así que 48 serían más de un giga. El anillo se fija por
**presupuesto de memoria** (1,5 GB) y salen los fotogramas que quepan: en
1080p sale hondo gratis, en 4K sale prudente (20).

Y una cifra que valida el §5bis: en la bobina con encadenado, los fps caen
de 163 a 138 **exactamente en la junta** y se recuperan después. El pico de
los dos decodificadores existe, es local y es pequeño — como estaba escrito.

**Dos motores de HEVC son más lentos que uno.** El troceado por segmentos
existía para ProRes (que sí tiene dos motores dedicados) y parecía que HEVC
podría repartirse igual: 1 → 153 fps, 2 → 110, 3 → 106. Las sesiones se
pelean por el mismo bloque del chip y obligan al mux a guardar segmentos
desordenados.

### H6 — la caché, fina

El camino viejo cacheaba PIEZAS; el motor de bobina no tiene piezas, así que
**se había perdido la caché**. Se recuperó en dos niveles:

**La bobina entera**, por su plan: revelar dos veces lo mismo (tras un
cierre, una cancelación, o para probar otro sonido) es instantáneo —
7,1 s → 0,7 s.

**Y por TRAMOS**, que es lo que pedía §7. La bobina se trocea en un tramo por
clip (con su junta incluida) y cada uno se cachea **por su contenido**: qué
fuente, en qué segundo, con qué receta y con qué encuadre. Medido con tres
clips y cortes secos, tocando el grade del tercero:

```
   primera vez     3 tramo(s): 3 revelados, 0 del cajón
   tocando el 3º   3 tramo(s): 1 revelado,  2 del cajón
```

Igual en las dos máquinas. Dos detalles que lo hacen funcionar:

- **La clave no lleva la posición en la bobina**, solo el contenido. Así un
  tramo sigue valiendo aunque lo de delante haya cambiado de duración.
- **Carrerilla**: cada tramo se revela con unos fotogramas de más por
  delante que no se escriben, para que el obturador llegue a su primera
  imagen con el arrastre ya formado. Sin eso se vería el escalón en cada
  juntura. En Windows además hay que descontarlos de la valla del anillo del
  codificador, o se espera un testigo que no llega.
- Los tramos se pegan con `concat -c copy` porque cada uno empieza con
  fotograma clave. Verificado que la juntura no deja costura.

Si algo no cuadra —un tramo que no sale, un pegado que falla, o una bobina
que se trocearía en más de 40— se cae al revelado de un tirón.

### Lo anterior: la caché de la bobina

El camino viejo cacheaba PIEZAS; el motor de bobina no tiene piezas, así
que **se había perdido la caché**. Ahora se cachea la bobina entera por su
plan (clips, receta, gelatinas, lienzo, códec y firma de cada fuente):

```
sin caché  7,1 s        con caché  0,7 s
```

Lo que aún no hace es afinar por clip —cambiar el grade de un solo clip
sigue recalculando la bobina—; eso pide caché por GOP, que sigue en pie.

### H5 — la divergencia, MEDIDA (y el mecanismo, construido)

Faltaba el dato que justifica todo el hito. Aquí está: se revelan los mismos
24 fotogramas por la cadena **WGSL** (`filmlook-core render`, que es la que
usan la preview y Windows) y por la cadena **MSL** (el motor del Mac), y se
comparan a resolución completa:

```
   WGSL contra MSL:  47,0 dB de media · 46,1 el peor
```

Unos 4,5 valores de código sobre 1023. O sea que **la promesa de la casa —lo
que ves es lo que sale— no se cumplía**, y no por poco. No es higiene: es un
fallo, y estaba escondido porque nadie había puesto las dos imágenes juntas.

De paso salieron dos cosas peores:

- **El CLI de `filmlook-core` llevaba tiempo sin arrancar.** Tenía un `GradeU`
  local de 64 bytes mientras `grade.wgsl` pedía 80. Nadie se enteró porque el
  trabajo pasa por la app. Ahora usa el del taller (y el `CompU` también).
- **Había TRES structs de grade distintos** (64, 80 y 128 bytes) para el mismo
  shader. Ya no.

**El mecanismo del plan B está construido y funcionando.** `metal/build.rs`
traduce con `naga` el `comp.wgsl` del taller a Metal en cada compilación, con
el reparto de bindings que el motor ya espera; se compila junto al
`chain.metal` de siempre y se elige con `FL_LOOK=wgsl`:

```
🧪 look ÚNICO: el comp sale del WGSL del taller
✅ 24 frames en 0.1s = 187 fps
```

A la misma velocidad, y el camino por defecto sigue dando **24/24 al bit**.

Está detrás de un interruptor **a propósito**: encenderlo mueve el máster del
Mac 53 dB. Cuál de las dos cadenas es la buena es decisión del autor mirando
las dos imágenes, no del compilador — así que el camino viejo manda hasta que
él lo diga.

Lo que falta para el hito entero: extender la traducción al *grade* y a la
pirámide (mecánico ahora que la tubería existe; el grade además tiene la
variante biplanar) y esa decisión sobre cuál es el look canónico.

### H5 — qué comparten ya los dos motores

No hace falta reescribir nada para converger, y ya se ha converjido en casi
todo. Hoy los dos motores comparten:

- **el plan de bobina** (`core/src/plan.rs`, incluido por ruta en los dos —
  el crate entero arrastra wgpu y winit, que en el motor de Mac no pintan
  nada), así que la bobina se compila a los mismos renglones en las dos
  máquinas;
- **la matriz del encuadre** (`plan::matriz`), que usa hasta el propio
  taller para llamar al motor de Windows;
- **el formato de los intermedios** (10 bits sin signo) — antes no;
- **el obturador fundido en el revelado** — antes no;
- **el fundido como mezcla por alfa** en el pase de revelado;
- `down`, `blur`, `accum` y `comp` en WGSL: **winlab ya los incluye desde
  `core/src/shaders/`**. El único que se escribe dos veces es el revelado
  (biplanar), por la forma de la entrada.

Falta el motor de Mac, que lleva su propio `chain.metal`. El plan B del §8
está **probado**: `naga` traduce `comp.wgsl` a MSL válido (comprobado en
esta sesión). Lo que queda es acoplar los índices de binding y el reparto
de uniformes, y verificar con el banco de aceptación que el look no se
mueve ni un LSB. Es trabajo mecánico, pero no ciego: hay que hacerlo con
las dos máquinas delante.

---

### El empalme (1-ago-2026) — tres fallos con la misma cara

El autor dijo que en un corte seco el **máster** daba un tirón, o se veía un
fotograma o dos del plano anterior, y que el sonido también daba un salto. Eran
tres cosas distintas y las tres se midieron antes y después.

**1 · El arrastre del obturador cruzaba el empalme.** El obturador es un IIR:
cada fotograma es `mix(nuevo, historia, shutter)`, y el reset (`pad1`) solo se
ponía en el PRIMER fotograma del tramo. Con el valor de la casa (0,143), el
primer fotograma del plano entrante llevaba encima un **14 % del que se iba** —
que es exactamente «ver el plano anterior después de haber cortado»—. Y peor:
como el reset sí caía al empezar cada tramo, **el máster salía distinto según
dónde troceara la caché**. Ahora el plan marca `Renglon::corte` (fuente nueva
que no viene de una junta) y los dos motores arrancan el acumulador ahí. En un
encadenado NO se marca: allí las dos imágenes conviven de verdad.

    primer fotograma tras el corte, contra el mismo revelado limpio
    antes  26,4 dB      ahora  46,0 dB   (46 dB = ruido de recompresión)

**2 · El motor del Mac elegía fotograma en el orden equivocado.** `Fuente::en`
comparaba `c.muestra >= objetivo`: dos índices de DECODIFICACIÓN mientras se
recorre la PANTALLA. Con fotogramas B los dos órdenes no coinciden. El de
Windows ya comparaba por PTS; el del Mac era el que se salía. Ahora compara
pts, y la cuenta de «¿hay que volver a buscar?» va en posiciones de pantalla
(`Indice::pos_pantalla`, el mapa al revés, que además hace el salto O(1)).

**3 · Y el salto tiraba fotogramas que hacían falta.** `busca()` soltaba TODO
lo decodificado en el camino («el arranque del GOP, a la basura»). En un IBBBP
el P se decodifica **antes** que las tres B que van delante suyo en pantalla:
al saltar a una B se soltaban las que venían después, el motor se quedaba
esperando un fotograma que ya no volvía y el máster salía con **repetidos y
negros**. Ahora se suelta solo lo que cae antes en pantalla, así que lo que
queda es la profundidad de reordenación (dos o tres), no el GOP. El mismo
fallo estaba en el decodificador de la preview (`cine.rs`), que además
convertía solo el objetivo.

    rampa de 100 fotogramas, la misma con y sin fotogramas B
    antes  repetidos y negros      ahora  las dos secuencias, IDÉNTICAS

**4 · Y el sonido, pegado a hueso.** La banda de voces recortaba en segundos
(`atrim=0:3.0400`, que casi nunca cae en muestra entera: medio error por clip,
sumando corte a corte) y concatenaba las ondas por donde cayeran. Ahora el
recorte es **en muestras** y cada empalme a hueso lleva tres milisegundos de
fundido a cada lado — el equivalente sonoro del empalme de la moviola, que
tampoco es a hueso.

    escalón de una muestra en la junta
    antes  0,0699  (nivel −0,049 → +0,021)      ahora  0,0059  (0,0002 → 0,0005)
    el escalón típico de la señal es 0,0010: antes era 70 veces mayor, ahora está dentro

**La trampa de fondo, otra vez la misma**: el material de la casa es de una
Insta360 y **no lleva fotogramas B** (`has_b_frames = 0`), igual que tampoco
lleva rotación. Dos fallos graves invisibles con el material de prueba. Para
medir estos hubo que fabricarse una rampa de luma con `-bf 3`: cada fotograma
un valor distinto, y así el orden se lee del propio vídeo.

---

## 9ter. LA CADENCIA (1-ago-2026) — el tirón, medido y quitado

El autor: «cuando se renderiza hay un poco de lag que es fruto de un
remuestreo mal hecho». Tenía razón, y se puede enseñar en una tabla.

**Cómo se mide.** Una fuente sintética de 59,94 fps en la que una barra
blanca avanza **10 píxeles exactos por fotograma**. En el máster se saca el
centroide de la barra en cada fotograma: la diferencia entre uno y el
siguiente dice cuántos fotogramas de origen se han consumido. Si esa
diferencia es constante, no hay tirón; su **desviación típica ES el tirón**,
en fotogramas de origen.

| máster | avance ideal | antes | ahora |
|--------|--------------|-------|-------|
| 59,94 → 59,94 | 1,0000 | 0,000 | **0,000** |
| 59,94 → 30    | 1,9980 | 0,000 | **0,001** |
| 59,94 → 25    | 2,3976 | 0,490 | **0,124** |
| 59,94 → 24    | 2,4975 | 0,500 | **0,106** |

**Y lo que queda de ese 0,106, con lo que está probado y lo que no.** Se
persiguió a conciencia porque un cuarto del tirón original sigue siendo tirón:

- **El plan es exacto**: 0,00000 a 24, 25, 30 y 50 fps, y hay una prueba que
  lo fija (`el_plan_reparte_los_fotogramas_sin_tiron`). La posición aparente
  avanza 2,4975 clavado, renglón a renglón.
- **El motor aplica bien los pesos**: con una fuente cuya barra salta 60 px
  por fotograma —las dos muestras no se solapan y se pueden pesar por
  separado— el peso medido difiere del del plan en 0,007 de media, 0,013 como
  mucho. Eso son 0,013 fotogramas de desvío, un orden de magnitud menos que
  lo que se mide de punta a punta.
- **No es el códec ni el espacio de color.** ProRes 4444 con gelatinas
  neutras da 0,106 y HEVC con la LUT de la casa 0,111: la misma cifra. La
  hipótesis de «se mezcla en espacio no lineal» era mía y **la medición la
  desmintió**. Linealizar la lectura tampoco arregla nada: la empeora.
- **No es el umbral de pegado.** Bajarlo de 0,02 a 0,004 —lo que quita el
  sesgo de redondear al fotograma vecino— no movió la cifra.
- **Lo que queda sin aislar** es la composición de las dos muestras dentro del
  motor. El siguiente experimento, apuntado y sin hacer: revelar el mismo plan
  con el pase de composición desactivado. Los fotogramas mezclados llegan a
  ese pase con la mitad de contraste que los de una sola muestra, y cualquier
  cosa no lineal de ahí —acutancia, `filmRes`, `softness`— desplazaría a unos
  y a otros de forma distinta.

**Y una lección sobre la regla, que costó tres mediciones falsas.** Medir el
centroide de la barra con un umbral («quédate con lo que pase del 6 % del
pico») **borra la muestra tenue** cuando el peso es 0,99, y entonces el
centroide salta a la muestra brillante y aparece un tirón que no existe.
Medir sin umbral mete el ruido de los 1920 píxeles de la fila. Lo correcto es
una **ventana** alrededor del pico anterior. Con esa regla la fuente mide
0,0002 y el máster viejo 0,4999, que es como se sabe que la regla es buena.

Antes, a 24, el avance alternaba **3, 2, 3, 2**: un objeto recorría un 25 %
más en un fotograma que en el siguiente, sesenta veces por segundo. Eso es lo
que se veía. No era un fallo de cuentas: era la política, «el fotograma de
origen más cercano y punto».

**Qué se hace ahora.** Lo mismo que el filtro de reducción hace en el espacio
(§1.5), pero en el tiempo: el fotograma del máster cae ENTRE dos de la fuente
y **se toman los dos, pesados**. El centro de la imagen pasa entonces a
avanzar 2,4975 cada vez, sin alternar.

**Y sin tocar los motores.** Mezclar dos fuentes con un peso es exactamente lo
que ya hacían para un encadenado. Cada fuente que necesita interpolar estrena
una **gemela** en el plan —el mismo fichero, la misma receta, su propio
decodificador— y cada renglón la usa como lado B. Cero líneas nuevas en
`metal/` y en `winlab/`; todo vive en `core/src/plan.rs`.

Tres cosas que **no** hace, a propósito:

- si la cadencia del máster divide exacta a la de la fuente (60→30, 60→60,
  50→25) no interpola: el peso sale 0 y la imagen queda idéntica y nítida —
  el camino rápido de siempre no se toca;
- si el máster va **más rápido** que la fuente (30→60) tampoco: ahí mezclar
  sólo inventaría fantasmas donde antes había un fotograma repetido;
- dentro de un encadenado el lado B ya está ocupado por el otro plano; esos
  pocos fotogramas siguen yendo al vecino más cercano.

Lo que queda por encima de esto sería compensación de movimiento (flujo
óptico). Es lo único más fino que existe, y trae artefactos en las oclusiones:
para material de mano y grano de película no compensa.

**Lo que apareció por el camino** (y no estaba en la lista):

- una fuente de **8 bits** se revela partida: el motor pide 10 bits y la
  importación como R16 lee dos píxeles por téxel. Con el material de la casa
  —10 bits siempre— no se veía. Anotado, sin arreglar.
- `out_dir` del payload se usa para escribir pero el CLI **informa de otra
  ruta**. Cuesta una tarde de medidas creer que un fichero es el que no es.

---

## 9quater. EL REVELADO, GENERALIZADO (2-ago-2026)

Una revisión externa del shader señaló ocho cosas. Todas eran ciertas. Lo que
sigue es qué se hizo y qué se midió.

### La disciplina de recortes (lo gordo)

`raw` se recortaba a [0,1] al salir del muestreo, otra vez tras la ganancia y
otra dentro de cada rama del push/pull. Cuando la señal llegaba al compresor
ya estaba **plana en 1,0**: el hombro no tenía nada que doblar y se limitaba a
bajar lo que ya había. Y su techo era `thr + range/(1 + c·range/wp)`,
estrictamente menor que 1: **comprimir bajaba el blanco a gris**.

Ahora se recorta sólo por abajo (`max(x, 0)`) hasta el final, se deja correr
por encima de 1, y hay **un único** recorte por arriba justo antes de la
gelatina. El hombro es exponencial —tangente a la recta en el umbral— y está
normalizado para que el nivel `compress_wp` acabe exactamente en 1,0.

Y eso destapó algo que no estaba en la revisión: **con `compWP = 1` un hombro
no puede recortar nada**, porque no hay margen por encima del blanco; lo único
que puede hacer es levantar las luces altas para que el blanco siga siendo
blanco, que es un cambio de contraste disfrazado de compresión. Ahí se apaga y
se dice. `compWP` pasa a significar «el nivel de entrada que acaba en blanco»
y tiene su propio mando en el cuarto oscuro («margen del hombro»).

Medido con una rampa de 0 a 100 % revelada en ProRes 4444 y gelatinas neutras:

- antes, comprimir movía la curva hasta **91 milésimas** hacia abajo;
- ahora, con `compWP = 1`, comprimir cambia **0,000 milésimas** (está apagado);
- y con margen, el blanco de entrada cae exactamente en blanco.

### Lo demás

| lo señalado | qué se hizo |
|---|---|
| `Y = clamp(Y,0,1)` mata el superblanco | fuera: de 941 a 1023 hay material legal y es justo lo que el hombro recupera |
| limited range clavado a 10 bits | forma sin profundidad: `(y − 16/255)/(219/255)`, los mismos números a 8, 10 o 12 |
| croma: tamaño impar y sitio | `ceil(size)` en el tope del plano y un desplazamiento `croma_x/croma_y` (por defecto **sentado a la izquierda**, que es lo que traen H.264 y HEVC de cámara) |
| filtro de reducción topado en 4:1 | el del máster sube a 6×6 (8K→1080 es 7,4:1). Por encima sigue faltando y **la respuesta no es ensanchar la rejilla sino un pase de prefiltrado**: no está hecho |
| `lut3` con `n − 1u` en enteros sin signo | guarda para `n < 2`; sin ella una .cube ilegible daba 4 294 967 295 |
| trilineal | **tetraédrica**, que es la del oficio: respeta la diagonal de grises, que es donde el ojo mira |
| `encuadra` calculado dos veces | una |
| matriz clavada a BT.709 | `matriz`: 709 · 2020 · 601. El HDR de móvil es 2020 y salía torcido |
| sin tramado | triangular de ±½ escalón de 8 bits en el pase final, con ruido blanco (`hash`) y no con el ruido de valor interpolado, que se vería como patrón |

### EL FILTRO ND

Un ND no es gris del todo, y ensucia de dos maneras que piden dos curas:

1. **El tinte plano.** Un ND variable son dos polarizadores cruzados y su
   extinción depende de la longitud de onda: sale una dominante —casi siempre
   magenta— parecida en toda la escala.
2. **La contaminación de infrarrojos**, que es la fea. El cristal corta el
   visible y deja pasar el infrarrojo cercano (700–1000 nm); el sensor sí lo
   ve, y el filtro rojo de la matriz de color es el que más lo deja entrar.
   Como el término es **aditivo**, sólo se nota donde hay poca luz visible:
   los negros se van a granate y las telas oscuras sintéticas salen marrones.

La cura del (2) tiene que quitar el rojo **que sobra** sin tocar el que hay, y
por eso lleva dos guardas: **la de gris** (la contaminación tiñe lo que está
cerca del neutro; un rojo de verdad está muy saturado) y **la de sombras** (el
término es aditivo). Se aplica en espacio de cámara, antes de la gelatina de
entrada, que es donde ocurrió la suciedad.

Medido con cuatro parches:

| parche | R−G sin ND | R−G con ND | cambio |
|---|---|---|---|
| gris oscuro sucio | +0,0266 | +0,0248 | −0,0018 |
| gris medio sucio | −0,0122 | −0,0357 | **−0,0234** |
| rojo oscuro de verdad | +0,0657 | +0,0648 | −0,0009 |
| rojo claro de verdad | +0,4721 | +0,4721 | **0,0000** |

**La guarda hace exactamente lo prometido**: al rojo saturado no lo toca en
absoluto. Corrige el gris. Pero el parche oscuro se mueve mucho menos de lo
que predice la fórmula (−0,0018 frente a −0,013 esperado) y **no lo he
aislado**: la medida es a la salida de la cadena entera y el pase de
composición vuelve a mezclar canales. El experimento que falta es poder
revelar con ese pase apagado.

### Y una trampa de compilación que costó dos tardes

`metal/build.rs` declaraba `cargo:rerun-if-changed` sobre **la carpeta** de
shaders. En macOS, tocar un fichero NO cambia la fecha del directorio que lo
contiene: cargo daba por bueno el Metal traducido de la vez anterior y el
motor seguía revelando con el shader viejo, mientras yo medía y sacaba
conclusiones contrarias. Ahora se declara **fichero a fichero**.

La otra de la misma familia: el CLI informaba de una ruta de salida
reconstruida (`carpeta del taller + nombre`) en vez de la real. Con un
`out_dir` distinto —lo normal— el parte decía un fichero que no era, y medí
másteres viejos creyendo que eran nuevos **dos veces**. Ahora viaja la ruta
entera.

### El material de 8 bits salía roto en el Mac (3-ago-2026)

**Todo H.264 de 8 bits se revelaba doblado en horizontal y con los colores
podridos** en el máster del Mac, desde siempre. Nunca se vio porque todo el
material de prueba histórico —y el del autor— es HEVC 10 bits.

La causa: `Fuente::abre` le pide a VideoToolbox `x420` (planos de 16 bits)
para HEVC pero `420v` (NV12, planos de 8) para H.264… y `bobina::importa`
importaba los planos SIEMPRE como `R16Unorm`/`RG16Unorm`. Cada texel de 16
bits se comía dos píxeles de 8: media imagen por textura, muestreada dos
veces a lo ancho, y el croma leído a bytes cambiados.

El arreglo es de una pieza: el formato **se le pregunta al buffer**
(`CVPixelBufferGetPixelFormatType`) y NV12 se importa como `R8`/`RG8` con
las medidas reales de cada plano (`GetWidthOfPlane`, que además hace bien
los tamaños impares). Los dos formatos muestrean a [0,1], así que el shader
ni se entera. El revelado canta ahora una línea por formato distinto
(`planos '420v': 1280×720…`) para que esto no vuelva a esconderse.

Verificado con la prueba de ácido de 6 capas: testsrc2 H.264 8 bits de base
+ 2 PiP de vídeo + 4 rótulos PNG, fotograma extraído y mirado; y el mismo
contenido en HEVC 10 bits, idéntico antes y después.

---

## 12. LA COPIA — un fotograma en papel (3-ago-2026)

La ampliadora del cuarto oscuro: sacar EL fotograma que se está mirando, con
su receta, sus capas y su encuadre, revelado por el mismo motor que la bobina.

**Por qué no vale sacarlo del máster con ffmpeg.** Medido sobre el mismo
fotograma (testsrc2, baño de la casa, 1920×1080): la copia contra el
fotograma extraído del máster da **36,3 dB**. Y la misma copia pasada sólo
por `yuv420p` y de vuelta da **36,2 dB**. O sea: *toda* la diferencia es el
**croma a la mitad**; el códec a 60 Mb/s no aporta ni 0,2 dB. La copia se
salta las tres pérdidas del máster —submuestreo de croma, rango limitado y
códec— porque sale del lienzo del comp en RGB de 10 bits, antes de empaquetar.

**Cómo.** Si la salida del motor termina en `.png`, no hay codificador: se
revela igual y el ÚLTIMO renglón se lee de la GPU (`lee_rgb16`, un blit a
búfer y `v<<6 | v>>4`, que es la conversión exacta de 10 a 16 bits) y se
escribe en PNG de 16 bits. El motor no sabe hacer otra cosa: JPEG, PNG de 8
y el reducido del supermuestreo los saca el taller convirtiendo ESE fichero,
así que sólo hay un sitio donde se pueda perder calidad y está a la vista.

**La carrerilla no es un adorno.** Con obturador el arrastre se forma con los
fotogramas anteriores, así que la copia pide 12 renglones de carrerilla: sin
ellos saldría más limpia que el máster en ese mismo segundo, o sea mentiría.

**El índice del fotograma, arreglado de paso.** El grano y el vaivén de la
ventanilla se siembran con el NÚMERO de fotograma, y un tramo suelto empezaba
a contar desde cero: la copia (y cualquier tramo de la caché fina) traía otro
grano que la bobina entera. Ahora el primer renglón del tramo viaja en
`FL_INDICE0` y el comp recibe el índice absoluto. Medido: la copia contra el
máster pasó de 34,4 a 36,3 dB sólo con eso — los 1,9 dB eran grano y vaivén
distintos.

**En Windows** el lienzo del comp ya existía (`out_rgb`, el parche del comp
escribe RGB en el destino 0 y el plano Y en el 1), así que la copia se lee de
ahí igual que en el Mac; el codificador sigue su curso a un temporal que se
tira. Compila (comprobado en cruzado desde el Mac); falta verlo correr.

---

## 10. Riesgos, con su salida

| riesgo | salida |
|--------|--------|
| La interop wgpu↔D3D11/VT se atasca | plan B de §8: naga genera HLSL/Metal desde el WGSL único |
| El compute con halo se complica en la 890M | H2 se puede entregar a medias: fusionar solo `down+blur` ya quita 4 viajes |
| `Rg11b10Float` da banding en degradados | medir con carta de grises; si aparece, `Rgba16Float` solo en `graded` |
| Romper el máster durante la reforma | `FL_MOTOR=ffmpeg` conserva el camino viejo hasta H4; el nuevo se activa con `FL_MOTOR=bobina` |
| Builds de Windows de 20 min (LTO) | perfil `release-rapido` con `lto="thin"` para iterar |
| Divergencia de resultados preview/máster | test de aceptación: revelar 3 fotogramas y compararlos con la preview píxel a píxel (tolerancia 1 LSB) |

---

## 11. Lo que NO se toca

- El **look** en sí: la física está bien y es la firma de la casa. Se
  reorganiza cómo se calcula, no qué calcula.
- El **audio**: ffmpeg lo hace de sobra y es barato. Una sola pasada al
  final (voces + música + banda + ducking + loudnorm) y mux.
- La **preview**: ya cumple. El objetivo es que el máster la alcance.
