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

## Lo único que sigue pendiente

▢ **Windows, recompilar y ver.** Todo lo de arriba está hecho y visto en el
Mac; la tanda de capas/pistas (y su mitad winlab, escrita) espera a que el
GPD esté encendido para compilarse, pasarse la prueba de ácido e instalarse.
