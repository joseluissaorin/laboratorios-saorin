<div align="center">

# LABORATORIOS SAORÍN

**Un editor de vídeo que no parece software.**

Montaje y revelado fílmico en la GPU · Rust + wgpu · macOS y Windows

[La web](https://laboratorios.joseluissaorin.com) ·
[Cómo está hecho](#la-arquitectura) ·
[El motor](MOTOR.md) ·
[La visión](NORTE.md) ·
[MIT](LICENSE)

</div>

---

## Qué es

Un editor de vídeo completo, escrito desde cero, que hace tres cosas y las
hace rápido:

- **monta** — cortar, ordenar, encadenar, poner música;
- **revela** — una cadena fílmica de verdad en la GPU: curva del negativo,
  gelatinas 3D, halación, floración, grano, obturador, viñeta, tejido;
- **saca el máster** — 4K a 60 fps más rápido que el tiempo real, con los
  motores de vídeo del chip.

Y una cuarta que es la razón de que exista: **no se parece a un editor de
vídeo**. No hay paneles grises, ni pestañas, ni un inspector con doscientas
casillas. Hay un taller de revelado con tres salas —la mesa de montaje, el
cuarto oscuro y el revelado—, papel hueso dibujado por un shader, trazos a
pulso, latas de película en una estantería y un cubo de recortes donde dejas
lo que aún no sabes dónde va.

> No es una piel encima de un editor normal. La interfaz **se dibuja con el
> mismo pipeline de GPU que el vídeo**, y las metáforas mandan sobre la
> implementación: si en un laboratorio el fundido se hace sobre la copia
> revelada y no sobre el negativo, aquí también — y resulta que además es
> más rápido.

## Por qué existe

Porque los editores de vídeo son herramientas de oficina disfrazadas de
herramientas de autor, y porque un revelado fílmico serio suele significar
esperar. Este proyecto es la respuesta a dos preguntas:

1. **¿Se puede editar en una herramienta que dé gusto tocar?** No «bonita»:
   que tenga tacto, peso y sitio. Un cubo de recortes en el que dejas cosas.
   Una manivela que gira. Una cinta de empalme que suena.
2. **¿Cuánto se puede acelerar el revelado si el motor lo hace TODO?** La
   respuesta, medida: 4K60 con la cadena fílmica completa va **más rápido que
   el tiempo real** en un portátil y en un mini-PC. Ver [MOTOR.md](MOTOR.md).

## Lo que ya funciona

**Montaje** — bobinas con clips, cortes al fotograma, encadenados, fundidos a
negro y a blanco, velocidad, encuadre por clip (escala, giro, desplazamiento,
encaje), huecos, fotos fijas, rótulos, pistas de música con envolvente
elástica y ducking, marcas, grupos, deshacer/rehacer por gestos, y un cubo de
recortes infinito al que se arrastra y del que se saca.

**Cuarto oscuro** — 52 parámetros del look en galvanómetros de laboratorio,
una lupa cuentahílos para mirar el grano al 100 %, comparador de cortinilla,
y las gelatinas 3D de entrada y de color, interpoladas por **tetraedros** (lo
que usa el mundo del etalonaje: una `.cube` se ve aquí como se ve en Resolve).
Y un **corrector de filtro ND**: el infrarrojo que cuela un ND tiñe los negros
de granate, y se quita el rojo que sobra —pesado a sombras y protegido por
saturación— sin tocar el rojo que hay.

**Revelado** — la bobina entera de un tirón, con caché por tramos: cambiar el
grade de un clip recalcula ese clip y nada más. Cuatro sellos —**REVELAR**
(el camino rápido), **ARCHIVO** (ProRes), **EN CLIPS** (una carpeta con un
fichero por plano, para montarlo en otro sitio) y **A MANO** (el cajón: hasta
8K, supermuestreo, códec, caudal, filtro y cadencia)— y una regla del rango
para sacar sólo un trozo.

**Cadencia sin tirón** — pasar de 59,94 a 24 saltaba dos y tres fotogramas de
origen alternándose; ahora el fotograma del máster cae entre dos de la fuente
y se toman los dos, pesados. Medido con una barra que avanza un número exacto
de píxeles: la desviación del avance baja de 0,500 a 0,112, y las cadencias
que dividen exacto (60→30, 60→60) siguen intactas y nítidas.

**Reproducción** — decodificación por hardware sin copias a CPU, arrastre de
la aguja con sonido (moviola), miniaturas instantáneas y salto por fotograma
clave.

## Las cifras

Medidas en las dos máquinas de desarrollo, con material real (HEVC 4K 10 bits
a 59,94), no en un banco sintético:

| | MacBook Pro M4 Max | GPD Win Max 2 (Radeon 890M) |
|---|---|---|
| la cadena fílmica sola | **541 fps** | — |
| revelado de bobina, 4K | **161–187 fps** | **73–83 fps** |
| 20 s de 4K60, extremo a extremo | 13,5 s → **7,1 s** | — |
| bobina de 44 s | — | 50,9 s → **17,5 s** |
| lo mismo, ya revelado | **0,7 s** | **1,3 s** |

En las dos máquinas el límite ya **no es la cadena fílmica**: es el
codificador de vídeo del chip. El look cuesta 0,1 ms de cada 6 en el Mac.

## Cómo se instala

Hace falta [Rust](https://rustup.rs) y `ffmpeg` en el `PATH`.

```bash
git clone https://github.com/joseluissaorin/laboratorios-saorin
cd laboratorios-saorin

cargo build --release --manifest-path nativa/Cargo.toml   # la aplicación
cargo build --release --manifest-path shell/Cargo.toml    # el taller (CLI de revelado)

# el motor de revelado, según la máquina:
cargo build --release --manifest-path metal/Cargo.toml    # macOS  (Metal + VideoToolbox)
cargo build --release --manifest-path winlab/Cargo.toml   # Windows (D3D12 + MF + AMF)

./nativa/target/release/saorin-nativa
```

El taller vive en `~/filmlab` (o donde diga `FL_HOME`): dentro van `media/`,
`projects/`, `luts/` y `out/`. El material **se importa por referencia**: no
se copia un solo byte.

### Revelar sin abrir la aplicación

```bash
shell/target/release/laboratorios-saorin cli render --json bobina.json
```

## La arquitectura

Cinco piezas. Ninguna sabe más de lo que le toca.

```
   nativa/     LA APLICACIÓN — winit + wgpu. Las tres salas, el visor, el
   9.500 ln    montaje. Dibuja la interfaz con el MISMO pipeline que el vídeo.

   core/       LO COMPARTIDO — el índice MP4 propio (búsqueda O(1) sin
   3.700 ln    ffmpeg), la cabina de proyección, los shaders del look en
               WGSL, y el plan de bobina compilado.

   metal/      EL MOTOR EN MAC — VideoToolbox para decodificar y codificar,
   3.300 ln    Metal para el look, cero copias a CPU.

   winlab/     EL MOTOR EN WINDOWS — Media Foundation + AMF, D3D11↔D3D12
   2.800 ln    con vallas, y los mismos shaders WGSL sobre wgpu.

   shell/      EL TALLER — el CLI de revelado: compila la bobina, reparte
   2.600 ln    el trabajo, hornea el sonido y cachea por tramos.
```

### Tres ideas que lo sostienen

**1. La interfaz es vídeo.** No hay framework de UI. El papel es un shader
(fibra por fbm, grano, viñeta, manchas y un cerco de café, con semilla
derivada del nombre del proyecto). Los trazos son polilíneas con temblor
determinista y presión variable. Todo se dibuja en cuatro capas con el mismo
`wgpu` que revela el vídeo, así que la interfaz cuesta lo que cuesta un
fotograma.

**2. El revelado es un compositor, no una tubería.** La bobina se compila a
una **tabla de renglones** —uno por fotograma de salida— que dice de qué
fuente sale, en qué segundo, con qué receta y con cuánto peso se encadena
con la siguiente. Con eso:

- **un corte no existe como operación**: es que el renglón siguiente mire a
  otra fuente. Ni pase extra, ni fichero intermedio, ni recodificación;
- **un encadenado es un `mix`** metido en el pase del revelado, no una
  pasada más sobre el máster;
- **un fundido a negro es una constante**, sin segunda fuente ni segundo
  decodificador.

**3. El fundido va donde iría en un laboratorio.** Un encadenado óptico es
una doble exposición sobre el mismo negativo, y el grano aparece *después*,
al revelar la copia. Así que las dos imágenes se mezclan **justo tras el
grade** y el pase caro —pirámide, halación, grano— corre una sola vez sobre
la mezcla. Es más fiel *y* más rápido: el grano no se duplica en la
transición, que es lo que produce ese «hervor» de los encadenados hechos a lo
bruto.

## Los documentos

Este repositorio se escribió documentándose a sí mismo. No son notas de
mantenimiento: son el razonamiento.

| | |
|---|---|
| **[NORTE.md](NORTE.md)** | La visión. Qué es el taller, por qué tres salas, qué es cada objeto y por qué. Se escribió *antes* de implementarlo. |
| **[MOTOR.md](MOTOR.md)** | La estrategia del revelado, con la bitácora de lo medido — **incluidas las tres veces que la medición desmintió al plan**. |
| **[TRASPASO.md](TRASPASO.md)** | El diario de obra, ronda a ronda, con cada trampa descubierta. |
| **[AVISOS.md](AVISOS.md)** | De quién es cada cosa: tipografías, gelatinas, grano. |
| **[PENDIENTE.md](PENDIENTE.md)** | Lo que le falta, por orden de cuánto estorba al montar. Sale de usarlo, no de imaginarlo. |

## Lo que aprendimos midiendo

Merece la pena decirlo porque es lo contrario de lo que suele contar un
README:

- **El plan se equivocó de formato.** Proponía `RG11B10Float` para los
  intermedios; medido, se separa 43 dB del original. El bueno era
  `RGB10A2Unorm` (1 valor de código sobre 1023), que además ya usaba el motor
  de Windows: los dos másteres llevaban tiempo sin ser el mismo fotograma.
- **El grano no es un hash.** La placa se sintetiza por FFT precisamente
  porque los hashes procedurales dejan banding axial. Sustituirla habría sido
  «optimizar» empeorando.
- **El look no era el cuello de botella.** El documento entero se apoyaba en
  que éramos *bandwidth-bound*. Falso en las dos máquinas: manda el
  codificador. Por eso la fusión a compute **se rechazó con datos** en vez de
  implementarse por fe.
- Y dos fallos de corrección que llevaban meses ahí: el máster se construía
  con búferes reciclados por el decodificador (fotogramas de otro sitio, y
  distintos en cada revelado), y el sello de tiempo nunca llegaba a la
  muestra.

## Estado y honestidad

Esto es **software de autor**, hecho para dos máquinas concretas y usado a
diario por una persona. Funciona, y funciona bien, pero no es un producto:
no hay instalador, ni telemetría, ni soporte.

Qué es legado y qué está vivo:

- **vivo**: `nativa/` (la aplicación), `core/`, `metal/`, `winlab/`,
  `shell/`;
- **legado**: `app/` y `studio/` — la generación anterior, en webview. Se
  conservan porque `shell/` todavía sirve su interfaz y el CLI de revelado
  depende de ella.

Lo que falta está anotado con nombre y apellidos en `MOTOR.md` §9bis y en
`TRASPASO.md`. No hay «pendientes» escondidos.

## Licencia

MIT — ver [LICENSE](LICENSE) y [AVISOS.md](AVISOS.md).

Las tipografías son OFL. La gelatina de color es obra del autor y va con el
mismo MIT; se agradece el crédito si la usas. Las transformadas log de
fabricante **no** se distribuyen aquí: pon la de tu cámara en
`<taller>/luts/entrada/`.
