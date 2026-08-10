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
