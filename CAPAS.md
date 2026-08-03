# CAPAS — pistas de vídeo, capas y bobinas anidadas

> El plan completo antes de tocar nada, y el estado de cada pieza al
> ejecutarlo. Regla de la casa: *verlo funcionando, no darlo por hecho*.

## 0 · Qué se pide y qué significa aquí

Tres cosas distintas que se tocan:

1. **Pistas** — más de un carril de material a la vez: vídeo base, una capa
   de vídeo encima, voz, y varios carriles de música.
2. **Capas** — que lo de arriba se COMPONGA sobre lo de abajo: títulos con
   su alfa, fotos con transparencia, vídeo sobre vídeo (PiP), cada capa con
   su encuadre, su receta y sus fundidos de entrada y salida.
3. **Anidadas** — una bobina dentro de otra: se inserta como un clip, se
   recorta como un clip, y por dentro sigue siendo la bobina hija (editarla
   y volver la refresca).

## 1 · La arquitectura, y por qué ésta

**La observación que lo abarata todo:** los motores ya saben dibujar N veces
sobre el mismo fotograma con un peso cada una — es lo que hace un encadenado
(lado A + lado B con `peso_b`). Una capa es EXACTAMENTE eso: un dibujo más,
con la novedad de que su alfa puede venir por píxel (títulos) y de que fuera
de su encuadre debe quedar TRANSPARENTE, no negro.

**Y la segunda:** una bobina anidada no necesita motor ninguno si se APLANA
al compilar. El encuadre es una afín 2×3 y las afines se componen: la matriz
del clip exterior (lienzo padre → lienzo hijo) por la del clip interior
(lienzo hijo → fichero) da una sola matriz (lienzo padre → fichero). El
motor recibe clips normales con una matriz explícita y no sabe que hubo
anidamiento.

Por tanto:

- el **renglón** gana un tercer hueco: `fuente_c` + `t_c` + `alfa_c`
  (la capa de ese fotograma, con su alfa global por fundidos);
- la **fuente** gana `capa: bool` (semántica de composición) y
  `mat: Option<[f32;6]>` (matriz explícita, para lo aplanado);
- los **shaders** ganan la semántica de capa: `src_mode` 2 = vídeo capa,
  3 = RGBA capa; fuera del encuadre → alfa 0; RGBA multiplica su alfa;
- los **motores** dibujan C después de A y B — el mismo pase, otro juego de
  texturas;
- la **anidada** se aplana en la app al armar el payload (profundidad ≤ 3,
  guarda de ciclos), componiendo matrices y arrastrando también las capas y
  la música de la hija.

## 2 · El modelo (nativa/proyecto.rs)

- `Capa { c: Clip, start: f64, fundido_in: f64, fundido_out: f64 }` —
  reutiliza `Clip` entero: encuadre, prefs, gelatinas, velocidad, mute. Se
  coloca LIBRE (con `start`), no en secuencia: una capa es un objeto puesto
  encima, no un eslabón.
- `Proyecto.capas: Vec<Capa>` — el orden de la lista es el orden de apilado
  (la última, encima). En pantalla es UN carril; el modelo admite N.
- `Clip.anidada: Option<String>` — la clave de la bobina hija. La hija se
  carga en `Proyecto.subbobinas` al abrir (y al volver de editarla), con
  profundidad ≤ 3 y guarda de ciclos.
- `PISTAS_MUSICA` se queda en 3: el carril nuevo es el de la capa, y un
  cuarto carril de música no cabía en el banco sin robarle sitio a la mesa.

## 3 · El plan (core/plan.rs)

- `Renglon { …, fuente_c: u32, t_c: f64, alfa_c: f32 }`.
- `compila` lee `clips2` (capas): por fotograma, la capa de más arriba que
  cubra ese instante; `alfa_c` = rampa de sus fundidos.
- `matriz_de` obedece `mat` si viene: el paso y las muestras del filtro de
  reducción se calculan de la matriz explícita igual que de la compuesta.
- `tramos()` añade `fuente_c` a las dependencias — **sin esto la caché fina
  serviría tramos viejos al cambiar una capa**.
- La cadencia (gemela) sigue siendo solo del lado A: una capa de vídeo con
  cadencia distinta va al fotograma más cercano (anotado, es lo honesto).

## 4 · Los shaders

`grade_bi.wgsl` (máster) y `grade.wgsl` (preview), y `chain.metal` detrás:

- binding nuevo `tRGBA` (en grade.wgsl ya existía `tVideo`);
- `src_mode`: 0 vídeo base · 1 RGBA base · 2 vídeo capa · 3 RGBA capa;
- capa (≥2): fuera del encuadre `return vec4(0,0,0,0)` — transparente;
- RGBA (1·3): sin matriz YUV y sin corrector ND (un título no pasó por la
  cámara); el alfa del píxel multiplica al peso;
- el obturador no arrastra en las capas (un rótulo no es luz de escena).

## 5 · Los motores

- **metal/bobina.rs** — el hilo de fuentes decodifica también C; las fotos y
  rótulos de capa suben UNA vez como textura RGBA residente
  (`foto::rgba()`); tercer `revela_en` con `peso = alfa_c`.
- **metal_pipe.rs** — `revela_en(…, rgba: Option<&Texture>)`; con el WGSL
  traducido el reparto de texturas es por orden de binding (tY, tUV, lutA,
  lutB, hist, rgba); un 1×1 blanco de repuesto cuando no hay capa.
- **winlab** — espejo del anillo del lado B (`ins_c`, M=4) para capas de
  vídeo, y RGBA residente para fotos/rótulos; el pase gana la textura y el
  parche del comp no se toca.

## 6 · La preview (visor.rs)

- capa foto/rótulo: textura RGBA residente + un segundo pase de grade con
  mezcla por alfa encima del lienzo (pipeline con blend, nuevo en
  `core/pipeline.rs`);
- capa de vídeo: segundo trío de planos Y/U/V servidos por el decodificador
  síncrono de scrub (el proxy decodifica en 1–3 ms) — la capa se ve en vivo
  también en reproducción;
- la receta de la capa tiene su propia huella (no ensucia la del clip base).

## 7 · La interfaz (nativa/main.rs)

- **el carril de la capa** encima de la tira de vídeo: tiras finas con
  nombre y fundidos; arrastrar una lata o una foto de la estantería y
  soltarla ahí crea la capa en ese segundo;
- mover (arrastre), recortar y ESTIRAR por los bordes (mismo trato que la
  música), ⌫ o papelera para quitar, clic para elegir;
- la ficha de la capa: fundidos que se ciclan, silencio, quitar, y la
  colocación del PiP (escala y posición arrastrando los números);
- **anidar**: menú Bobina → «Insertar otra bobina…» (elige el .json de la
  hija); el clip anidado se dibuja con marco doble y su nombre; recortarlo
  es recortar la ventana sobre la hija; editar la hija y volver refresca;
- alt-arrastre en el visor mueve el encuadre de la CAPA si hay una elegida
  (si no, el del clip base, como siempre).

## 8 · El aplanado de anidadas (nativa, al armar el payload)

- clip anidado → los clips reales de la hija recortados a su ventana, cada
  uno con su receta y su matriz compuesta (`mat`);
- las capas de la hija → `clips2` del padre, desplazadas y recortadas;
- la música de la hija → `audio` del padre, desplazada y recortada;
- el fundido del clip anidado con el siguiente lo hereda el último clip
  interior; el del anterior entra solo (el orden se conserva);
- velocidad del clip anidado: v1 sólo ×1 (se avisa y se trata como ×1);
- ciclos: prohibido insertarse a sí misma; profundidad máxima 3.

## 9 · Caché, atajos y transparencia

- la clave de cada tramo añade `fuente_c`/`t_c`/`alfa_c` y la firma de las
  fuentes de capa (fichero + receta + encuadre);
- el atajo identidad→remux se APAGA si hay capas o matrices explícitas;
- el diario dice cuántas capas y cuántos niveles de anidado llevaba el plan.

## 10 · Qué se verifica y cómo

| pieza | prueba |
|---|---|
| plan | tests: capa por fotograma, rampas de alfa, `mat`, deps de tramos |
| aplanado | test: 2 niveles con matrices compuestas contra cuentas a mano |
| máster Mac | render con base + PNG con alfa + PiP + anidada; se mira el fotograma |
| máster GPD | el mismo payload, y fotograma comparado |
| preview | captura con capa visible sobre el clip |
| caché | cambiar la capa invalida SOLO sus tramos |

## Estado

✅ = visto correr · ⧗ = escrito y compila, falta verlo en su máquina.

- ✅ **el plan**: renglón con C y D, capas por fotograma con rampas, matriz
  explícita, dependencias de tramos — 27 tests, incluidos el aplanado a dos
  niveles contra cuentas a mano y la guarda de ciclos
- ✅ **el aplanado**: ventana, velocidad exacta en las tres marchas, capas y
  música de la hija desplazadas, fundidos heredados
- ✅ **el máster en el Mac**: base + rótulo PNG con alfa (franja al 50 %
  mezclando de verdad) + PiP de vídeo + anidada a media escala, TODO en el
  mismo fotograma — mirado, no supuesto
- ✅ **la preview**: compone las capas en vivo (RGBA y vídeo), y la anidada
  proyecta el clip hijo con el encuadre exterior
- ✅ **la interfaz**: carril de capas con fundidos, soltar una lata o una
  foto lo crea, mover/recortar/estirar, ⌫, ficha con fundidos y encuadre a
  cero, alt-arrastre coloca el PiP, «Insertar otra bobina…», el clip anidado
  con marco doble y las miniaturas de su hija, deshacer con capas
- ⧗ **winlab**: los carriles C y D, el RGBA residente y el búfer por carril
  están escritos; **el GPD está apagado** y no se ha podido compilar ni ver.
  Nada de esta tanda está en Windows todavía.

## Los límites que quedan, dichos claros

- dos capas COMPUESTAS por fotograma (C y D): tres solapadas a la vez → se
  quedan las dos de encima;
- la cadencia de una capa de vídeo va al fotograma más cercano (la gemela es
  del carril base);
- la preview resuelve UN nivel de anidado (el máster, tres) y no compone el
  encuadre del hijo con el exterior si AMBOS están tocados;
- el sonido de la hija viaja al máster, no a la preview;
- la cuchilla no corta capas (se recortan por los bordes);
- velocidad del clip anidado: ×1.
