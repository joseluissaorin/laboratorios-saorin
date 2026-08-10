<div align="center">

# LABORATORIOS SAORÍN

**Un editor de vídeo que no parece software.**

Montaje y revelado fílmico en la GPU · Rust + wgpu · macOS y Windows

[La web](https://laboratorios.joseluissaorin.com) ·
[Cómo está hecho](#la-arquitectura) ·
[El motor](MOTOR.md) ·
[La visión](NORTE.md) ·
[MIT](LICENSE)

*Monta · revela · subtitula · saca el máster. Todo en la máquina, sin nube.*

</div>

---

## Qué es

Un editor de vídeo completo, escrito desde cero, que hace cuatro cosas y las
hace rápido:

- **monta** — cortar, ordenar, encadenar, apilar pistas de vídeo, poner
  música, anidar una bobina dentro de otra;
- **revela** — una cadena fílmica de verdad en la GPU: curva del negativo,
  gelatinas 3D, halación, floración, grano, obturador, viñeta, tejido;
- **escucha** — subtítulos automáticos con un modelo local (whisper.cpp por
  Metal en el Mac y por Vulkan en Windows). Nada sale de la máquina;
- **saca el máster** — 4K a 60 fps más rápido que el tiempo real, con los
  motores de vídeo del chip… o **un solo fotograma** en la calidad que se
  elija, mejor que sacarlo del máster con ffmpeg.

Y una quinta que es la razón de que exista: **no se parece a un editor de
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
negro y a blanco, velocidad (incluida la marcha atrás y el congelado),
encuadre por clip (escala, giro, desplazamiento, encaje), huecos, fotos
fijas, rótulos, marcas, grupos, deshacer/rehacer por gestos, un cubo de
recortes infinito al que se arrastra y del que se saca, y una papelera que
acepta las tres cosas (un clip, un recorte y una cinta de la estantería).

**Pistas** — hasta **ocho de vídeo encima de la base** (V2…V9) y ocho de
música, y todas componen a la vez: PiP con su encuadre, rótulos y fotos con
su alfa por píxel, fundidos de entrada y salida por clip. Los carriles
aparecen cuando los necesitas y la mesa crece con ellos. Y **bobinas
anidadas**: una bobina dentro de otra, que se recorta como un clip y por
dentro sigue siendo suya — al revelar se aplana componiendo las matrices.

**Sonido** — pistas de música con envolvente elástica y ducking, el sonido
del vídeo desacoplable a su propia pista, niveles y silencios por pista,
vúmetros de L y R, y **marcas al compás**: un detector de pulso propio
(flujo espectral + autocorrelación + programación dinámica) siembra la bobina
de marcas en cada golpe, y como las marcas son imanes, la cuchilla y los
bordes se pegan al ritmo solos. A lo que no tiene pulso se le dice que no lo
tiene, no se le inventa.

**El pie (subtítulos)** — pista propia debajo de la tira. Se transcriben
solos con un modelo local, se corrigen escribiendo encima, se mueven y se
estiran por los bordes, se parten por la aguja. El estilo es de toda la
pista —letra, tinta, cuerpo, altura, sombra, caja, mayúsculas, caracteres por
línea— y el de casa es clásico y moderno a la vez: Fraunces en hueso,
centrado abajo, sin caja ni contorno duro, sólo un halo difuminado. Ocho
lenguas a elegir. Para el motor un subtítulo **es una capa**, así que la
preview y el máster enseñan exactamente lo mismo.

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

**La ampliadora** — en el cuarto oscuro, debajo del vidrio: saca **el
fotograma que estás mirando** con su receta, sus capas y su encuadre, al
tamaño que elijas (el lienzo supermuestreado, el doble, el cuádruple) y en el
papel que elijas (PNG de 16 bits, de 8, o JPEG). Sale del lienzo del comp en
RGB de 10 bits, antes de empaquetar: se salta el submuestreo de croma, el
rango limitado y el códec. Medido, esa es *toda* la diferencia con un
fotograma sacado del máster (36,3 dB, y el croma solo explica 36,2).

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

Y el oído, sobre 34 s de habla real (modelo `large-v3-turbo` cuantizado):

| | motor | 34 s de sonido |
|---|---|---|
| MacBook Pro M4 Max | Metal | 1,6 s · **21,5× tiempo real** |
| GPD Win Max 2 (890M) | Vulkan | 14,7 s · **2,3×** (5,8–9,2× en caliente) |
| GPD, sólo CPU | — | 153,6 s · 0,2× |

Vulkan sobre la iGPU no es un adorno: **doce veces más rápido que la CPU** y
encima con el modelo bueno en vez del ligero — 17 trozos donde la CPU sacaba
4. El camino de CPU se queda como red de seguridad.

## Cómo se instala

Hace falta [Rust](https://rustup.rs) y `ffmpeg` en el `PATH`.

```bash
git clone https://github.com/joseluissaorin/laboratorios-saorin
cd laboratorios-saorin

cargo build --release --manifest-path nativa/Cargo.toml   # la aplicación
cargo build --release --manifest-path shell/Cargo.toml    # el taller (CLI de revelado y el oído)

# el motor de revelado, según la máquina:
cargo build --release --manifest-path metal/Cargo.toml    # macOS  (Metal + VideoToolbox)
cargo build --release --manifest-path winlab/Cargo.toml   # Windows (D3D12 + MF + AMF)

./nativa/target/release/saorin-nativa
```

El taller vive en `~/filmlab`: dentro van `media/`, `projects/`, `luts/`,
`modelos/` y `out/`. Con `FL_MEDIA` se apunta a otra carpeta de material (y
el taller pasa a ser la carpeta madre de ésa). El material **se importa por
referencia**: no se copia un solo byte.

### El oído, y cómo dejarlo fuera

`shell/` enlaza whisper.cpp estáticamente. Eso pide **LLVM/libclang** para
generar los enlaces (bindgen), y en Windows además el **SDK de Vulkan**. Si
no los tienes o no quieres subtítulos:

```bash
cargo build --release --manifest-path shell/Cargo.toml --no-default-features
```

El taller entero sigue funcionando y los subtítulos automáticos **avisan de
que no están** en vez de fallar de forma rara. Las dos banderas son `oido`
(la transcripción) y `ventana` (la webview del estudio anterior, que el
editor nativo sustituyó); las dos vienen encendidas.

En **Windows**, para compilar el oído con Vulkan hacen falta cuatro cosas, y
las cuatro por un motivo que costó encontrarlas (está en
[PENDIENTE.md](PENDIENTE.md)):

1. el SDK de Vulkan, con `VULKAN_SDK` y su `Bin` en el `PATH` (`glslc`
   compila los ~2.200 shaders de ggml);
2. el entorno de MSVC cargado **antes** (`vcvars64.bat`): el generador de
   shaders se compila con un cmake anidado que no hereda lo que preparan los
   crates `cc`/`cmake`;
3. **Ninja** como generador (`CMAKE_GENERATOR=Ninja`) y `CMAKE_GENERATOR_INSTANCE`
   vaciada — con el generador de Visual Studio ese anidado ni configura;
4. y una **carpeta de destino corta** (`CARGO_TARGET_DIR=C:\fl`): la ruta del
   proyecto anidado pasa de los 260 caracteres de Windows y el enlazador
   casca con `LNK1104`.

El modelo se baja solo la primera vez a `<taller>/modelos/`, y **lo elige la
máquina**: con GPU detrás, `large-v3-turbo` cuantizado (574 MB); a pelo con
la CPU, el ligero (190 MB), que es el único que allí no desespera. Con
`--modelo 0|1|2` se manda uno a mano.

### Revelar sin abrir la aplicación

```bash
shell/target/release/laboratorios-saorin cli render --json bobina.json
```

### Subtitular sin abrir la aplicación

```bash
shell/target/release/laboratorios-saorin cli oye --media plano.mp4 --idioma es --out pie.srt
```

## La arquitectura

Cinco piezas. Ninguna sabe más de lo que le toca.

```
   nativa/     LA APLICACIÓN — winit + wgpu. Las tres salas, el visor, el
   17.500 ln   montaje, las pistas, el pie y el compás. Dibuja la interfaz
               con el MISMO pipeline que el vídeo.

   core/       LO COMPARTIDO — el índice MP4 propio (búsqueda O(1) sin
   4.400 ln    ffmpeg), la cabina de proyección, los shaders del look en
               WGSL, y el plan de bobina compilado.

   metal/      EL MOTOR EN MAC — VideoToolbox para decodificar y codificar,
   3.200 ln    Metal para el look, cero copias a CPU.

   winlab/     EL MOTOR EN WINDOWS — Media Foundation + AMF, D3D11↔D3D12
   3.200 ln    con vallas, y los mismos shaders WGSL sobre wgpu.

   shell/      EL TALLER — el CLI de revelado (compila la bobina, reparte el
   3.700 ln    trabajo, hornea el sonido, cachea por tramos) y EL OÍDO
               (whisper.cpp enlazado estático, Metal o Vulkan).
```

### Cuatro ideas que lo sostienen

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

**3. Lo que se ve encima es siempre lo mismo.** Un PiP, un rótulo con alfa,
una bobina anidada y un subtítulo parecen cuatro cosas distintas y para el
motor son **una capa**: un dibujo más sobre el fotograma, con su encuadre y
su peso. Un subtítulo se rasteriza a un PNG con su alfa y entra por ahí; una
bobina anidada se aplana componiendo las matrices afines. Así la preview y
los dos motores enseñan lo mismo sin saber qué están enseñando — que es la
única forma de que *preview = export* sea verdad y no una intención.

**4. El fundido va donde iría en un laboratorio.** Un encadenado óptico es
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
| **[MOTOR.md](MOTOR.md)** | La estrategia del revelado, con la bitácora de lo medido — **incluidas las veces que la medición desmintió al plan**. |
| **[ARQUITECTURA.md](ARQUITECTURA.md)** | Para quien vaya a tocar el código: cómo viaja un fotograma desde el fichero hasta el máster. |
| **[CAPAS.md](CAPAS.md)** | Las pistas apiladas, las capas y las bobinas anidadas: el plan entero antes de escribirlo, y el estado de cada pieza al ejecutarlo. |
| **[PENDIENTE.md](PENDIENTE.md)** | Lo que se ha ido arreglando y lo que falta, por orden de cuánto estorba al montar. Sale de usarlo, no de imaginarlo. Ahí están también las trampas de compilar en Windows. |
| **[HERRAMIENTA.md](HERRAMIENTA.md)** | Qué le faltaba para dejar de ser un juguete, dicho por quien lo usa. |
| **[AVISOS.md](AVISOS.md)** | De quién es cada cosa: tipografías, gelatinas, grano, modelos. |
| **[CONTRIBUIR.md](CONTRIBUIR.md)** | Qué esperar si te apetece tocar algo. |

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
- **La teoría de los hilos se equivocó.** ggml quiere núcleos físicos, dice
  el manual; en el Ryzen del mini-PC —cuatro Zen 5 y ocho Zen 5c— darle sólo
  los doce físicos salió **peor** (191 s) que los dieciséis lógicos (153).
  Se quedó lo que midió mejor, con la medida escrita al lado.
- **Y un fallo que llevaba ahí desde el principio**: todo el material H.264
  de 8 bits salía doblado y con los colores rotos en el máster del Mac —
  los planos NV12 se importaban como si fueran de 16 bits. No se vio nunca
  porque el material de casa es HEVC de 10. Lo destapó una prueba de ácido
  con seis capas.
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
- **legado**: `app/` y `studio/` — la generación anterior, en webview. El
  editor nativo la sustituyó, así que ya no se le debe la compilación: vive
  detrás de la bandera `ventana` y se sirve con `laboratorios-saorin serve`.
  El CLI de revelado y el oído no dependen de ella.

Lo que falta está anotado con nombre y apellidos en `MOTOR.md` §9bis y en
`PENDIENTE.md`. No hay «pendientes» escondidos.

## Licencia

MIT — ver [LICENSE](LICENSE) y [AVISOS.md](AVISOS.md).

Las tipografías son OFL. La gelatina de color es obra del autor y va con el
mismo MIT; se agradece el crédito si la usas. Las transformadas log de
fabricante **no** se distribuyen aquí: pon la de tu cámara en
`<taller>/luts/entrada/`. Los modelos de transcripción tampoco: se bajan de
[whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) la primera vez
que se usan, con su propia licencia.
