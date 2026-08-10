# PENDIENTE — lo que el autor ha visto roto (1-ago-2026, segunda vuelta)

> La lista anterior está ejecutada y guardada en
> [`docs/PENDIENTE-cerrado-1ago.md`](docs/PENDIENTE-cerrado-1ago.md).
> Ésta es nueva: dieciséis cosas dichas por el autor después de usar el taller
> de verdad. Están agrupadas por dónde duelen, no por el orden en que
> salieron: varias son el mismo agujero visto desde sitios distintos.
>
> Regla de la casa: *verlo funcionando, no darlo por hecho*. ✅ = visto correr;
> ⧗ = escrito y compila, falta verlo; ▢ = sin tocar.

---

## §1 · EL MODELO DE PISTAS (el cimiento: casi todo lo demás cuelga de aquí)

✅ **7. El audio del vídeo se desacopla.** `Proyecto::desacopla` baja el sonido
del plano a la primera pista de audio libre —mismo fichero, mismo trozo, mismo
segundo— y calla el clip para que no suene dos veces. Menú «Desacoplar el
sonido» y ⇧D. A partir de ahí es material: se mueve, se corta con la cuchilla
y tiene su nivel. El revelado ya respetaba `mute` por clip.

✅ **16. Títulos y textos con su propia pista de vídeo** — hecho como LA CAPA
(CAPAS.md): un carril encima de la bobina que acepta rótulos y fotos CON su
alfa por píxel y vídeo (PiP) con encuadre, fundidos de entrada y salida, y
hasta dos compuestas a la vez. Y de propina: **bobinas anidadas** (una bobina
dentro de otra, aplanada al revelar con las matrices compuestas).

✅ **5. La cuchilla también corta la música.** Con una pista elegida, la
cuchilla muerde la música; si no, el vídeo. La herramienta es la misma.

✅ **6. El imán, en la música.** Cortando y colocando: la cuchilla se pega al
empalme o a la marca más cercana, y al arrastrar una canción se prueban sus
**dos bordes** (pegar por el final es tan frecuente como por el principio).

✅ **8. La ficha de la música transparenta.** Taparla con un rectángulo opaco
no bastaba: la ficha del clip lleva miniatura, y las texturas van en el atlas,
que se pinta **siempre** por encima de la capa de rectángulos. Ahora la del
clip **no se dibuja** si hay una música elegida.

## §2 · EL SONIDO (no obedece)

✅ **3 y 4. Los mandos de sonido no hacían nada.** Eran dos fallos encadenados:

1. las palancas de silencio vivían en `arranca_toca`, así que bajarlas con la
   bobina sonando no hacía nada nunca. Ahora los mudos y los dos niveles son
   atómicos que el hilo de audio lee **en cada bloque**;
2. y sobre todo: **el clic no llegaba**. La rama de la estantería se quedaba
   con la columna izquierda ENTERA por debajo de la cabecera y siempre
   retornaba, así que el margen del banco —los dos mandos, las dos palancas y
   la manivela— era código inalcanzable. Faltaba un `&& my < banco`.

✅ **Y vúmetros de L y R**, aparte de las dos agujas por banda: dos barras con
escala de oído, la marca de −6 dB y el techo en rojo.

## §3 · LA MESA

✅ **2. La marca de corte no se puede quitar.** Se quita con un clic encima,
con Esc y con ⌘Z (que la deshace antes que nada, porque una cuchilla puesta
todavía no ha cortado). **Y se dibuja donde va a morder**: con una música
elegida, la tijera baja a su carril — antes marcabas en un sitio y cortaba en
otro.

## §4 · LA PREVIEW (miente sobre lo que enseña)

✅ **13 y 11.** Los dos eran lo mismo: la receta se EMPUJABA («acabo de tocar
el clip i») en vez de tirarse de ella («¿qué clip estoy viendo?»). Ahora
`cadena()` mira la aguja en cada fotograma, y la huella lleva el índice del
clip **y su encuadre** — que es lo que faltaba para verlo moverse en vivo. De
paso hubo que memorizar el tamaño de las gelatinas: preguntarlo lee y parsea
el .cube entero, y eso ahora se pregunta sesenta veces por segundo.

✅ **12. Pantalla completa de la PREVIEW, no de la app.** Doble clic en el
vidrio: desaparece el taller entero y queda la imagen encajada por su
proporción sobre negro, con el teclado intacto. Esc o doble clic para volver.

## §5 · LA SALA DE REVELADO

✅ **9. Repensarla desde cero:** el fallo de fondo era que el dibujo y el
ratón llevaban cada uno sus números a mano y se separaban solos — al añadir un
cuarto sello, los sellos midieron 960 px y el parte de salida se fue fuera de
la pantalla. Ahora hay **una sola planta** (`struct Planta`) que leen los dos,
y los sellos van en rejilla de dos columnas.

✅ **15. El rango, ahí dentro.** Una regla con la bobina entera de punta a
punta, el tramo en rojo, **las juntas de los planos marcadas** para pegarse a
ellas, dos tiradores que se arrastran y un «TODA LA BOBINA» para quitarlo.

✅ **14. Las gelatinas se eligen**: LUT de entrada y LUT de color, con sus dos
carpetas configurables en ajustes.

## §6 · LAS VENTANAS

✅ **1. La ventana de proyectos, independiente.** «Ventanas → Las bobinas,
aparte». Lista con nombre, clips, duración y formato; la abierta en rojo; un
clic cambia de bobina. Sin carteles: esa ventana no tiene capa de texturas y
prometer una miniatura que no se puede pintar sería mentir.

✅ **10. La app de Windows no tiene icono.** Dos cosas: el PNG de la ventana
ahora va **dentro del binario** (`include_bytes!`, antes se leía del árbol de
compilación en tiempo de ejecución) y el .exe lleva su **recurso de Win32**
(`build.rs` + `winresource`), que es lo que ve el Explorador y el acceso
directo. Si el compilador de recursos faltara, avisa y sigue: sin icono se
trabaja, sin binario no.

---

# Segunda tanda (2-ago-2026)

> **De propina (3-ago): EL COMPÁS.** «Marcas al ritmo de la música»: un
> detector de pulso propio (`ritmo.rs` — flujo espectral + autocorrelación
> con prior + programación dinámica de Ellis, con FFT de casa y symphonia;
> cero dependencias nuevas) siembra la bobina de marcas ♩ en cada golpe.
> Como las marcas son imanes, la cuchilla y los bordes se pegan al pulso
> solos. En la ficha de la música («al compás ♩» / «compás fuera», idempotente
> y con caché por cinta) y en el menú (con una música elegida, la suya; si
> no, todas). Los golpes se dibujan como palitos de metrónomo, no chinchetas
> —trescientas chinchetas serían una pared— y de lejos ni se pintan. A lo
> que no tiene pulso (habla, rubato, ruido) se le DICE «sin pulso», no se le
> inventa: la guarda es la autocorrelación de Pearson sin rectificar (medido:
> ruido 0.014, metrónomo 0.955; el Concerto in F de Gershwin, con razón,
> 0.07 → fuera). Verificado con tests de señal fabricada (120 clavado, 84 no
> se dobla) y con música CC0 real; y de paso symphonia gana mp3/wav/flac,
> que la estantería ya aceptaba sin poder decodificarlos. Las MARCAS entran
> en el historial: ⌘Z también las deshace.

## §7 · EL EDITOR

✅ **El dibujo del volumen se altera al recortar.** Los puntos de la banda se
guardan en tiempo de FUENTE; al recortar caían fuera de la ventana visible y
**se seguían dibujando**, por encima de los vecinos. Ahora el tramo que cruza
el borde se corta EN el borde interpolando su altura, los puntos de fuera no
se pintan, y donde la banda aún no empieza (o ya acabó) se dibuja el nivel
plano para que el trozo no aparezca sin línea.

✅ **La música, con los mandos de un clip.** Le faltaban los fundidos: estaban
en el modelo y se leían en la ficha, pero no había forma de tocarlos. Ahora se
ciclan con un clic, igual que en la ficha del clip.

✅ **No se podía estirar un recorte para recuperar material.** Dos causas:

1. **el tirador no sabía dónde acababa la cinta** — ahora hay un almacén de
   duraciones (`dur_fuente`, memorizado porque preguntarlo abre el
   contenedor) y el tope de arriba es el final del material, no el sitio donde
   soltaste el tirador la última vez;
2. **el borde compartido entre dos trozos pegados** —justo lo que deja la
   cuchilla— caía dentro de las zonas de tirador de los dos, que eran ±7 px
   alrededor de cada borde: cogías la cola de uno o la cabeza del otro según
   el orden en la lista, o sea al azar. Ahora cada zona vive DENTRO de su
   propio trozo y el reparto es exacto.

✅ **El fantasma seguía dibujándose al arrastrar al cubo.** Se pintaba el clip
sobre la línea de tiempo con su línea de inserción mientras el cubo se abría:
dos cosas contrarias a la vez. Ya no se pinta si vas a soltarlo fuera.

✅ **LA PAPELERA, universal.** El cubo GUARDA («por si acaso», y de ahí se
saca) y la papelera TIRA. Acepta las tres cosas: un clip de la bobina, un
recorte del cubo y una cinta de la estantería. Y ⌫ sobre un recorte lo tira
sin arrastrar.

Una cosa que **no** hace, a propósito: la cinta sale de la ESTANTERÍA, no del
disco. El taller trabaja por referencia (NORTE §1.4) y el material es del
autor: de aquí no se borra un fichero nunca.

## §8 · EL REVELADO, GENERALIZADO

✅ Los ocho puntos de la revisión del shader y **el filtro ND**, con lo medido
y lo que queda sin aislar, en [`MOTOR.md` §9quater](MOTOR.md).

## §9 · LA AMPLIADORA (3-ago-2026)

✅ **Sacar un fotograma en la calidad que se elija.** Va en EL CUARTO OSCURO,
debajo del vidrio —que es donde se está mirando la imagen—, no en la sala de
revelado (esa saca la bobina entera). Dos mandos que se ciclan con un clic:
**tamaño** (el lienzo supermuestreado · el doble · el cuádruple, con los
píxeles reales al lado) y **papel** (PNG 16 bits · PNG 8 · JPEG 95), y el
botón. La copia sale a `copias/` con el segundo en el nombre.

Con «el lienzo» la copia se revela AL DOBLE y se reduce con lanczos: el grano
y los bordes salen sin escalones. Es el mismo motor, la misma receta y las
mismas capas que el máster — y mejor que sacar el fotograma del máster con
ffmpeg, medido: la diferencia entre ambos es exactamente el croma a la mitad
([`MOTOR.md` §12](MOTOR.md)).

## §10 · EL PIE — subtítulos automáticos (3-ago-2026)

✅ **Subtítulos automáticos con modelo local**, en casa y sin red: whisper.cpp
enlazado estático (`shell/src/oido.rs`). En el Mac por **Metal**; en Windows
por CPU, que en el HX 370 son doce núcleos Zen 5 con AVX-512 y ggml los usa
todos. Medido en el M4 Max con `large-v3-turbo` cuantizado: **21,5× tiempo
real**. El modelo (574 MB) se baja UNA vez a `<taller>/modelos/`.

Se escucha **la bobina entera** con una sola carga del modelo: se le manda la
lista de planos con sonido —fichero, trozo, dónde cae y a qué velocidad— y los
tiempos vuelven ya en segundos de la línea de tiempo.

✅ **Pista propia** (`subtitulo.rs`), debajo de la tira: bloques con su texto,
que se eligen, se mueven, se estiran por los bordes (con imán), se parten por
la aguja y se quitan. Se corrige escribiendo encima: clic en el texto de la
ficha, ⏎ guarda. Entra en el historial: ⌘Z deshace también el pie.

✅ **El estilo, de toda la pista**: letra (Fraunces serif · Space Grotesk ·
Grotesk negra), tinta (hueso · ámbar · blanco · tinta), cuerpo, altura,
sombra, caja, MAYÚSCULAS y caracteres por línea. El de casa es **clásico y
moderno a la vez**: Fraunces —un old-style de corte contemporáneo— en hueso y
no en blanco, centrado abajo, sin caja ni contorno duro; sólo un halo
difuminado que lo despega del fondo. La anchura del halo se midió mirando:
ceñida se veía preciosa sobre cielo oscuro y desaparecía sobre un parabrisas
al sol.

Para el revelado un subtítulo **es una capa**: cada línea se rasteriza a un
PNG con su alfa y entra por el camino ya probado de CAPAS §4 — así la preview
y los dos motores lo dibujan sin saber qué es un subtítulo, y salen idénticos.
El PNG va recortado al texto (un lienzo entero por línea serían 8 MB de
textura cada uno).

### La tercera vuelta del pie: PALABRA A PALABRA

✅ **Seguían saliendo párrafos.** El corte lo hacía whisper y la app se lo
comía tal cual, así que si el corte era malo no había nada que hacer. Ahora
el oído devuelve **las palabras con su segundo**, y el que arma los
subtítulos es la app (`subtitulo::arma`).

Dos cosas hicieron falta para que los tiempos por palabra fueran de verdad:

- **DTW** (`dtw_parameters`, con el preset del modelo). Sin él, los sellos
  por token caen en los bordes del tramo y no en la palabra — se veía a la
  legua: todo a segundos redondos. Con él, «Todavía» va de 0,12 a 0,45.
- **cerrar los huecos pequeños**: DTW da un INSTANTE por palabra, no un
  tramo, así que cada palabra se estira hasta la siguiente… salvo que haya
  un silencio de verdad, que es justo lo que se usa para cortar.

Y el armador corta —por este orden— donde hay **punto**, donde hay una
**pausa** de 0,6 s, donde **el modelo rompía** (esa pista sabe cosas que un
contador de letras no sabe), donde ya no cabe en **dos renglones** y donde el
pie llevaría más de **siete segundos**. Nunca parte una palabra y **nunca
saca tres renglones**. Medido sobre los mismos 20 s: 17 pies de ≤74 letras y
≤3,6 s, con cada frase en el suyo. Con el umbral anterior salían 15 de hasta
77 letras y 5,4 s, y uno juntaba dos frases distintas.

✅ **Cambiar de idea es gratis.** Las palabras se guardan en la bobina, así
que elegir otro ancho **recompone la pista entera** al instante y sin volver
a escuchar: 28 letras → 18 pies, 52 → 17. Comprobado por la interfaz.

✅ **La ventana del oído, con mandos de verdad.** El primitivo que faltaba es
`mandos.rs`: un **desplegable** que abre la lista entera (se ve lo que hay y
se va directo, en vez de ciclar a ciegas) y una **regla de dos tiradores**
para el trozo, porque un rango se elige arrastrando, no escribiendo dos
números. La lengua, el sonido, el modelo y las letras por renglón son ahora
listas. Con una lista abierta lo demás **no se dibuja**: en este taller el
texto va siempre por encima de los rectángulos, así que lo que se tapa es lo
que no se pinta (la lección de la ficha de la música, otra vez).

✅ **Y la ventana se cierra sola** cuando el oído termina. Antes había que
adivinar si había acabado y cerrarla a mano.

✅ **Y la velocidad del clip, también desplegable.** Era un ciclo a ciegas de
nueve marchas donde llegar a «marcha atrás» eran siete clics, y el rótulo
decía «×-2.00» en vez de decir qué es. Ahora se abre la lista con sus
nombres: *marcha atrás*, *fotograma congelado*, *muy lento · ×0,25*… Con la
lista abierta, la ficha de debajo no se dibuja, por lo mismo de siempre.

### La segunda vuelta del pie (lo que se vio usándolo)

✅ **Salían párrafos, no subtítulos.** Era lo más grave: whisper devuelve
tramos de hasta treinta segundos y aparecía un muro de texto de golpe.
whisper.cpp sabe partirlo, pero hay que pedírselo con **tres cosas a la vez**
—tiempos POR TOKEN, largo máximo y cortar POR PALABRA— y ninguna estaba
puesta. Ahora el largo va atado al ancho de línea del estilo (dos renglones
exactos) y encima hay dos topes de tiempo que el largo en caracteres no
detecta: **mínimo un segundo** (medio segundo no da tiempo ni a mirarlo) y
**máximo siete** (whisper le cuelga a veces a un tramo corto el silencio que
viene detrás, y el pie se quedaba doce segundos en pantalla). Medido sobre
20 s de habla: 20 cues, ≤80 caracteres, ≤4,8 s, sin un solo solape.

✅ **El color, libre; negrita y cursiva.** El color era una lista de cuatro y
ahora son tres números (con los seis de la casa como atajos y la muestra al
lado, que un nombre no dice qué color es). La negrita usa la **fundición de
verdad** cuando la familia la tiene —Space Grotesk trae su negra— y engorda
el trazo cuando no; la cursiva inclina 12° con el eje en la línea base de
cada renglón, no en el borde del lienzo: si no, el bloque se abre en abanico.

✅ **La ventana del oído.** «Subtítulos automáticos» tenía que preguntar y no
preguntaba. Ahora abre su ventana: la lengua, qué trozo (toda la bobina o
sólo el rango), de qué sonido (los planos o una pista de música — que es
donde cae una voz en off desacoplada), el modelo, y una **barra que avanza
por planos** con lo que está haciendo y cuánto lleva. De paso: `curl` escribía
su medidor a stderr y el taller lo leía como si fueran pasos suyos («0 0 0
--:--:--» donde debía decir qué hacía).

### Lo medido en las dos máquinas (y lo que queda por hacer)

| máquina | modelo | cómo | 34 s de sonido |
|---|---|---|---|
| M4 Max · **Metal** | large-v3-turbo q5 | haz 5 | 1,6 s · **21,5×** |
| HX 370 · **Vulkan** (890M) | large-v3-turbo q5 | haz 5 | 14,7 s · **2,3×** |
| HX 370 · CPU | small q5_1 | haz 2 · 16 hilos | 153,6 s · 0,2× |
| HX 370 · CPU | small q5_1 | haz 2 · 12 hilos | 191,5 s · 0,2× |

**Vulkan sobre la Radeon 890M cambia el GPD de sitio**: doce veces más rápido
que la CPU y encima con el modelo BUENO (turbo en vez de small) y el haz
largo — 17 trozos donde la CPU sacaba 4. Por el camino de la bobina entera,
con el modelo ya caliente, sube a 5,8× y 9,2×.

El camino de CPU se queda como red de seguridad y no era culpa del build
—ggml se compila con `/arch:AVX512` en Release, comprobado en el log de
cmake— ni de los hilos: dejarle sólo los doce núcleos físicos salió PEOR
(191 s) que los dieciséis lógicos (153), porque ese chip mezcla cuatro Zen 5
con ocho Zen 5c y al planificador le va mejor tener dónde elegir.

Y el español en el GPD, comprobado sobre el fichero: UTF-8 con sus tildes
(«Todavía las cuidan», «Acuérdate de ponerlo»).

**En el GPD, Vulkan sobre la Radeon 890M**, que es lo que convierte esa iGPU
en el motor del oído igual que Metal en el Mac. Lo que costó montarlo ahí:

- el **SDK de Vulkan** (winget `KhronosGroup.VulkanSDK`), con `VULKAN_SDK` y
  su `Bin` en el PATH — `glslc` compila los shaders de ggml;
- **el entorno de MSVC cargado antes** (`vcvars64.bat`): el generador de
  shaders de ggml-vulkan se compila con un cmake ANIDADO que no hereda lo que
  preparan los crates `cc`/`cmake`, y sin vcvars su `cl.exe` «no puede
  compilar un programa simple»;
- y **Ninja como generador** (`CMAKE_GENERATOR=Ninja`): con el generador de
  Visual Studio ese anidamiento se rompe igual aunque el entorno esté puesto.
  Con Ninja no hay proyectos anidados que valga. Dos avisos: hay que vaciar
  `CMAKE_GENERATOR_INSTANCE` (vcvars deja `VSINSTALLDIR` y Ninja rechaza la
  «instancia»)… y **borrar el `whisper-rs-sys-*` de `target`**, porque el
  `CMakeCache.txt` de la tentativa anterior se acuerda de la instancia y la
  vuelve a imponer aunque ya no esté en el entorno. Cambiar de generador
  obliga a configurar desde cero;
- y **compilar el shell en una carpeta corta** (`CARGO_TARGET_DIR=C:\fl`).
  El proyecto anidado del generador de shaders cuelga de `target/` y su ruta
  —`…\whisper-rs-sys-…\out\build\ggml\src\ggml-vulkan\
  vulkan-shaders-gen-prefix\src\vulkan-shaders-gen-build\CMakeFiles\
  CMakeScratch\TryCompile-…`— pasa de los 260 caracteres de Windows: el
  enlazador casca con `LNK1104` al no poder abrir su propio manifiesto. En
  `C:\fl` cabe. `instala.ps1` coge el binario de ahí.

Los tres están en `compilatodo.bat` del GPD.

### El idioma del pie

Se elige antes de escuchar y es **de la pista**: español, inglés, gallego,
catalán, portugués, francés, italiano y «lo adivina». Está en la ficha del pie
(con «escuchar otra vez» al lado, que es lo que se quiere después de
cambiarlo) y en el menú Editar — porque antes de la primera transcripción no
hay ningún subtítulo que elegir para llegar a su ficha. Comprobado que la
bandera llega de verdad: el mismo audio español sale «Todavía las cuidan» en
español y «They still take care of them» en inglés.

### Lo que costó llevarlo a Windows (para no repetirlo)

Tres trampas, ninguna del código del pie:

1. **`whisper-rs-sys` genera sus enlaces con bindgen, que pide libclang.** El
   GPD no tenía LLVM y eso tumbaba el shell ENTERO —revelado incluido— por
   una función que igual no se usa. Se instaló LLVM (winget) *y* el oído pasó
   a ser una bandera de compilación (`--no-default-features` deja el taller
   entero funcionando y los subtítulos avisan de que no están).
2. **`tauri-build` 2.6 no encuentra el `cargo:dev` de `tauri` 2.11 en
   Windows.** El shell usaba tauri sólo para la ventana del estudio viejo,
   que el editor nativo sustituyó. Ahora es la bandera `ventana` (encendida
   por defecto) y el GPD compila sin ella.
3. **Un `[features]` mal colocado partió la lista de dependencias**: tauri,
   tiny_http, rust-embed, serde_json y percent-encoding quedaron dentro del
   bloque `[target.'cfg(target_os = "macos")'.dependencies]`. En el Mac
   seguían resolviéndose y en Windows desaparecieron las cinco — 136 errores
   de importación en un crate intacto. **Un fallo que sólo se ve en la otra
   máquina es el más caro de todos**; por eso ahora se comprueba en cruzado
   desde el Mac, y va gratis:

       cargo check --manifest-path winlab/Cargo.toml --target x86_64-pc-windows-msvc
       cargo check --manifest-path nativa/Cargo.toml --target x86_64-pc-windows-msvc

   (El shell no se puede: `zstd-sys` necesita un compilador de C para MSVC.
   Ése hay que compilarlo en el GPD.) Esto destapó de paso un préstamo
   imposible en el código de CAPAS de winlab que llevaba dos días escrito y
   sin compilar nunca.

## Lo único que sigue pendiente

▢ **Windows, recompilar y ver.** Todo lo de arriba está hecho y visto en el
Mac; la tanda de capas/pistas (y su mitad winlab, escrita) espera a que el
GPD esté encendido para compilarse, pasarse la prueba de ácido e instalarse.
