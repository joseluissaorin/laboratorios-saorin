# NORTE — el taller entero, nativo

> Estrella norte de la interfaz nativa. Este documento existe porque el autor
> señaló el problema real: **la app nativa tiene el motor pero no tiene el
> taller**. Las tres salas no existen como lugares, la estética Saorín está
> aguada (sin wiggly lines, sin dibujos, sin latas de verdad, sin papel vivo),
> y todo está comprimido en una sola pantalla que no se puede explotar.
>
> Referencia visual: las tres capturas del webview enviadas por correo
> («Laboratorios Saorín — las tres salas», 31-jul) + `studio/` como
> especificación de comportamiento. Pero la orden NO es calcar el webview:
> es **superarlo** — más lleno, más bold, más dibujos, más herramientas
> físicas, más información. Original, no reskin.
>
> Revisión 2 (con los ajustes del autor): miniaturas instantáneas en latas y
> recortes, la cinta de 6 fotogramas al primer toque, la manivela con rueda
> de scroll, el papel negro tiza del cuarto oscuro, y una sección nueva —
> **lo mínimo de un editor, hecho objeto** — que baja a tierra todas las
> propiedades de clip y herramientas que aún faltan.
>
> Revisión 3: el modelo de proyecto sin ambigüedad (§2bis — la bobina es
> un fichero que refiere; estanterías independientes, carpeta = balda,
> clasificador vídeo/música/fotos) y la pared configurable en ajustes.

---

## 0. La tesis

La app no es un programa con una piel bonita: es **un taller físico simulado**.
Cada píxel se gana su sitio siendo un *objeto del oficio* — una lata, una
cubeta, una ficha, un galvanómetro, un sello de goma. Donde un editor normal
pone un panel gris, nosotros ponemos la herramienta física que haría ese
trabajo en un laboratorio de cine de los años 60, dibujada con la gramática
del zine Saorín: dos tintas sobre papel hueso, tipografía bold fuera de
registro, líneas a pulso, anotaciones manuscritas, sellos, códigos de barras.

Cuatro reglas que se derivan de la tesis:

1. **Nada perfectamente recto.** Toda línea de separación, borde o subrayado
   es un trazo a pulso (wiggly). Todo objeto pegado (foto, etiqueta, ficha)
   está ligeramente rotado. Las sombras son de imprenta, no de sistema.
2. **Horror vacui del taller.** Un taller de verdad está LLENO: notas en las
   paredes, herramientas colgadas, marcas de uso. Ninguna esquina vacía sin
   propósito: si sobra sitio, ahí va información (el parte del motor, un
   doodle, un susurro, un colofón, un código de barras).
3. **La información es decoración y la decoración informa.** El código de
   barras ES el nombre del proyecto. El sello ES el estado del render. La
   aguja del galvanómetro ES el valor del parámetro. No hay adorno mudo.
4. **La imagen manda.** Todo lo que represente material de vídeo enseña
   SUS fotogramas de verdad, al instante: las latas, las tiras, los
   recortes, la cuerda de secado. El atlas de miniaturas ya lo da gratis
   (`frame_clave` + atlas GPU): ningún objeto de material sale «ciego».

Y una regla de ingeniería que no se negocia: **la velocidad es sagrada**
(VELOCIDAD.md es contrato). Todo lo de este documento se pinta en los
mismos ~2 ms de CPU/GPU por frame que hoy; nada de esto puede costar un
milisegundo del scrub.

---

## 1. El sistema estético (los materiales)

### 1.1 El papel — procedural, vivo, distinto por sala

Hoy: color plano + grain.png en mosaico. Norte: **un shader de papel**
(`papel.wgsl`) que genera el material en GPU:

- Fibra de papel: 2–3 octavas de fbm anisótropo (la fibra corre horizontal),
  grano fino encima, viñeta sutil hacia los bordes de la ventana.
- **Semilla = nombre del proyecto**: cada bobina tiene SU papel (manchas y
  fibra propias, reconocibles). El papel de «Boda de Marta» no es el de
  «Pruebas Luna».
- Imperfecciones escasas y grandes: una mancha de humedad en una esquina,
  un cerco de taza de café cerca de donde vive el timecode, una arruga
  (línea de sombra) cerca del pliegue. Pocas: dos o tres por pantalla.
- **Tres papeles, tres salas**: hueso cálido en la mesa; hueso frío en el
  revelado; y el cuarto oscuro tiene SU papel propio — **papel negro
  tiza**, mate y granulado como una pizarra, con la fibra apenas visible
  a contraluz y polvillo de tiza en las esquinas. No es «el mismo papel a
  oscuras»: es otro material que RECUBRE la pantalla mientras dura la
  sala (ver la transición en §2).

### 1.2 La tinta — dos tintas y fuera de registro

- **Ultramar** (#2b3bc7) para estructura y datos; **rojo bermellón** para lo
  vivo (avisos, sellos, nombres de clip, subrayado de la sala activa);
  **grafito** para lo manuscrito secundario; **ámbar** solo donde ya es
  canon (subtítulos filtración, badges).
- Sobre el papel negro tiza del cuarto oscuro la tinta cambia de cuerpo:
  todo se escribe en **tiza y luz de seguridad** (rojo anaranjado), como
  rotulado con lápiz blando sobre pizarra.
- Los titulares grandes van **fuera de registro**: la misma palabra impresa
  en rojo desplazada 2 px bajo la azul (como el logo del webview). Es UN
  quad extra por titular: gratis.
- Sombras de objetos = **trama de semitono** (puntos) u offset duro de
  tinta, jamás blur suave de sistema.

### 1.3 El trazo a pulso — primitiva universal (`trazo.rs`)

La pieza que más cambia la cara de la app. Un generador de polilíneas con
jitter determinista (semilla = id del elemento, estable entre frames para
que no «hierva»):

- **Línea wiggly**: separadores, subrayados, la regla del tiempo, los
  bordes de cajas. Grosor variable a lo largo del trazo (presión), tinta
  que sangra en los extremos (un punto más gordo al empezar y acabar).
- **Flecha a mano**, **círculo/elipse de rotulador** (para rodear cosas),
  **corchete**, **tachón**, **llave**.
- Se cachean como vertex buffers (triángulos con grosor); regenerar solo
  al cambiar tamaño de ventana. Coste de dibujo: el mismo que una línea
  recta.
- Uso obligatorio: **toda** línea de la UI pasa a ser trazo. La sala
  activa lleva subrayado wiggly rojo (como el webview). Cero `Rect`
  perfectos visibles salvo el vidrio del visor y los fotogramas (lo
  fotográfico es lo único recto: el contraste es la gracia).

### 1.4 Los dibujos — atlas de doodles y objetos

Segundo atlas de texturas (`doodles.png`, 9-slice donde toque) con dos
fuentes: los assets del webview (`studio/`, `app/ui/assets/`) y piezas
nuevas horneadas con Pillow/Blender en build:

- **Objetos**: lata metálica (tapa + canto, con hueco para la miniatura),
  botella de baño marrón con etiqueta, caja de stock estampada, cubeta
  con líquido, pinza de tender, clip de papel, chincheta, banderita de
  chincheta, celo (cinta adhesiva), cinta washi de colores, etiqueta
  troquelada, sello de goma (marco), lupa cuentahílos, manivela,
  interruptor de palanca, imán de herradura, grapadora + grapa, tampón,
  acetato (hoja transparente), archivador, interruptor de luz, bombilla
  roja, tijeras, lápiz graso.
- **Doodles**: la mosca, flechas rojas, asteriscos, estrellitas, un sol y
  una luna (según la hora local — el sol del revelado se vuelve luna por
  la noche), espiral de lápiz, «×» de tachadura, cerco de café.
- **Fotos encoladas**: la foto B/N del laboratorista (Library of Congress)
  pegada con celo; hueco para que el autor ponga las suyas — **LA PARED,
  que se configura en ajustes** (⚙): un panel con las fotos pegadas en
  miniatura, botón «pegar una foto…» (diálogo nativo), quitar una
  despegándola, y también arrastrar una imagen directamente sobre la
  portada o los márgenes de la mesa para pegarla ahí. Las fotos de la
  pared se COPIAN a `~/filmlab/pared/` (excepción consciente a la regla
  de cero copias: son pequeñas y la pared no debe romperse si los
  originales se mueven).
- Todos los objetos «pegados» llevan rotación aleatoria estable de ±2°
  (necesita quads rotados en el lienzo — primitiva nueva P3).

### 1.5 La tipografía — jerarquía bold

Ya están las cuatro familias. El norte es usarlas con el volumen del zine:

- **Grot Bold enorme** para los nombres de sala (EL CUARTO OSCURO a 40+ px,
  fuera de registro) — hoy los rótulos son tímidos.
- **Courier espaciado** (tracking ancho) para datos: timecodes, contadores,
  ETIQUETAS EN VERSALES con letter-spacing como la del webview
  («ESTANTERÍA DE MATERIAL»).
- **Caveat** para TODO lo humano: nombres de clip sobre la bobina, notas,
  susurros, etiquetas de lata, valores escritos a mano en las fichas.
- **Fraunces** para el colofón y los textos largos (ajustes, avisos).

### 1.6 El sonido del taller

Cada sala tiene su fondo (muy bajo, toggle en ajustes): la mesa un reloj y
un proyector lejano; el cuarto oscuro un goteo y un ventilador; el revelado
un burbujeo. Cambiar de sala = **clac de interruptor** (entrar al cuarto
oscuro apaga la luz de verdad: ver §2). El foley de acciones ya existe
(corte, lata, tick): se amplía con verter (baños), sello (revelar), pinza
(colgar), grapa, palanca, y el **trinquete de la manivela** (un clic por
fotograma).

---

## 2. La arquitectura: TRES SALAS de verdad

El error estructural actual: una sola pantalla con todo. El norte: un
`enum Sala { Portada, Mesa, CuartoOscuro, Revelado }` donde **cada sala es
dueña de la pantalla entera** y tiene SU layout, SU papel y SUS objetos.

- **Navegación**: la cabecera del taller es común (logo fuera de registro +
  las tres salas con subrayado wiggly en la activa + campanilla de avisos)
  y vive en las tres salas. Atajos: `1`/`2`/`3` (mesa/cuarto/revelado),
  `Esc` vuelve a la mesa.
- **Transición entre salas de papel** (mesa ↔ revelado): el **pliegue de
  papel** — la sala saliente se pliega en dos mitades con sombra de doblez
  (180 ms) y la entrante se despliega.
- **Transición al cuarto oscuro** (la ceremonia completa, ajuste del
  autor): primero **el papel hueso de la mesa se pliega** y se retira;
  clac de interruptor y fundido breve a negro; y entonces **se despliega
  el papel negro tiza** que recubre la pantalla mientras dura el cuarto
  oscuro — la imagen del visor enciende primero (retroiluminada) y el
  instrumental en tiza/safelight aparece después, en 200 ms, como ojos
  que se acostumbran a la oscuridad. Al salir, lo mismo al revés: se
  pliega el papel tiza, clac, se despliega el hueso.
- **El estado viaja**: el clip seleccionado en la mesa es el que se
  revela/etalona; el TC de la aguja se conserva entre salas; `R` desde
  cualquier sala lleva al revelado con el botón armado.
- La **portada** es la cubierta del zine (ya existe, se re-viste): másthead
  gigante, proyectos como latas en estantería —cada una con la miniatura
  de su último fotograma dentro de la tapa—, etiqueta manuscrita + código
  de barras, colofón abajo («hecho a mano en los Laboratorios Saorín ·
  dos tintas sobre papel hueso · LAB SAORIN 2026»).

---

## 2bis. La bobina y su material (el modelo de proyecto, sin ambigüedad)

Hoy el modelo es confuso incluso para el autor: ¿el proyecto apunta a una
carpeta o es un fichero? La respuesta canónica, escrita de una vez:

### 2bis.1 Qué es un proyecto

- **Un proyecto es UN FICHERO** — la bobina (`.bobina`, JSON versionado;
  hoy `projects/*.json`) — **que REFIERE material, jamás lo contiene**.
  Regla de la casa absoluta: la app **nunca copia, nunca mueve y nunca
  renombra ficheros del disco del autor** (la única excepción es la
  pared, §1.4). Renombrar una cinta cambia su nombre lógico en el
  registro; el fichero queda intacto.
- Cada ruta se guarda **absoluta Y relativa al fichero de la bobina**:
  si bobina y material viajan juntos (misma carpeta o subcarpetas), el
  proyecto abre limpio en otra máquina. Si algo no aparece: pizarra de
  claqueta + relink recursivo (localizar uno re-enlaza a sus hermanos).
- El **taller** (`~/filmlab/`) guarda solo lo DERIVADO y regenerable:
  proxies, miniaturas, formas de onda, renders, la pared, ajustes. Se
  puede vaciar sin perder ni un empalme.
- «Empaquetar la bobina» (HERRAMIENTA §1) es el único momento en que se
  copia material: proyecto + media usada a una carpeta elegida, con
  rutas reescritas — el backup de verdad y el «pásamelo al portátil».

### 2bis.2 Las estanterías — el material se organiza en baldas

La estantería deja de ser una lista única: son **estanterías
independientes** (los bins de un editor), cada una una balda de madera
con su **nombre manuscrito en una etiqueta troquelada** y su sección
plegable. El clip guarda a qué estantería pertenece (solo registro).

- **Soltar una CARPETA del Finder/Explorer = una estantería nueva** con
  el nombre de la carpeta, poblada recursivamente. Con ⌥ al soltar,
  cada subcarpeta de primer nivel hace SU estantería.
- **La balda recuerda su carpeta**: una estantería nacida de una carpeta
  queda «enchufada» a ella (un cablecito dibujado a lápiz hacia el
  nombre de la ruta). Botón **«volver a mirar»** (rescan manual): trae
  lo nuevo con badge «3 latas nuevas» — sin vigilancia automática de
  fondo que gaste batería; mirar es un gesto.
- **Crear, renombrar, quitar y reordenar** estanterías a mano (clic
  derecho en la balda). Quitar una estantería quita sus latas del
  registro — el disco no se toca jamás, y avisa si algún clip de la
  bobina usa material de ahí.
- **Arrastrar latas entre estanterías** (con selección múltiple).
- Una lata puede estar en UNA estantería (modelo simple de baldas, no
  etiquetas múltiples — para eso está la washi).

### 2bis.3 Separar por naturaleza (vídeo / música / fotos)

Dos mecanismos, ambos baratos y visibles:

- **El clasificador al importar** (interruptor de palanca en ajustes,
  encendido por defecto): lo que entra suelto se reparte solo — el
  vídeo a la estantería activa, el audio a **«LA DISCOTECA»** (balda de
  carretes de cinta magnética) y las fotos a **«EL ÁLBUM»** (balda de
  marquitos de diapositiva). Las carpetas arrastradas NO se reparten
  (respetan su balda propia), salvo que se pida.
- **Las pestañitas de filtro** sobre la estantería: `todo · vídeo ·
  audio · fotos` (pestañas de archivador asomando) + **la etiqueta de
  buscar**: un campo manuscrito que filtra latas por nombre al teclear.
  Filtrar no mueve nada: es mirar la misma estantería con otros ojos.

### 2bis.4 El parte del material

En el parte del proyecto (ficha sin selección, §3.3): nº de latas por
estantería, minutos totales referidos, tamaño en disco del material y de
lo derivado, cuántas latas offline. El proyecto se entiende de un
vistazo: qué es, cuánto pesa, qué le falta.

---

## 3. LA MESA (montaje) — la sala que ya existe, reordenada

Lo que hay se reordena con la geografía del webview y se llena:

### 3.1 Las estanterías (izquierda) — latas que enseñan su material

- Rótulo «ESTANTERÍA DE MATERIAL» en versales espaciadas con flecha roja
  a mano apuntando a la primera lata; debajo, **las baldas** (§2bis.2):
  cada estantería con su etiqueta manuscrita, plegable, reordenable, con
  su cablecito a la carpeta de origen si está enchufada. Encima, las
  pestañitas de filtro `todo · vídeo · audio · fotos` y la etiqueta de
  buscar (§2bis.3).
- **Latas de verdad, nunca ciegas**: círculo metálico con canto y brillo
  (atlas), y **dentro de la tapa, la miniatura real del clip** (el
  fotograma-póster, instantáneo vía el atlas de miniaturas que ya existe)
  como si la película asomara por la lata abierta. Encima, la etiqueta
  troquelada con el nombre lógico manuscrito y la duración estampada.
  Lata de audio = carrete de cinta magnética marrón con su onda; lata de
  foto = marquito de diapositiva con la imagen dentro.
- **El gesto en tres tiempos** (ajuste del autor):
  1. **Un toque** → la lata se abre y **se desenrolla una cinta de 6
     fotogramas igualmente espaciados** (0 %, 20 %, 40 %, 60 %, 80 %,
     100 % de la duración) — una tira de película con perforaciones que
     cae de la lata sobre el papel, cada fotograma con su TC estampado
     debajo. Los 6 salen del atlas al instante (`frame_clave` ya tarda
     milisegundos). La misma acción manda la cinta al **monitor de
     fuente** (paridad con el flujo de 3 puntos que ya existe).
  2. **Tocar un fotograma de la cinta** → el monitor de fuente salta a
     ese punto (la cinta es también navegación).
  3. **Doble toque** (en la lata o en un fotograma) → a la bobina, por
     los puntos de entrada/salida si los hay.
- Debajo, en Caveat azul: «las latas se abren con dos toques» (y el pool
  de susurros rota: consejos contextuales, ver §8).
- La foto B/N pegada con celo + código de barras de la bobina actual.
- Badge de proxy en el canto de la lata (verde listo / ámbar cociéndose);
  pizarra de claqueta si el fichero está offline.

### 3.2 El visor (centro), la manivela y la rueda

- El vidrio con **marcas de registro** (⌖) en las esquinas y borde de
  tinta azul; el letterbox es papel, no negro.
- Transporte: timecode en **contador mecánico** (dígitos en fichas negras
  que giran al correr), botones azules de imprenta, y a la derecha **LA
  MANIVELA**: una rueda con manija que
  (a) gira sola durante la reproducción — velocidad proporcional al fps —,
  (b) se agarra y se le da vueltas para scrub con inercia (clics de
  trinquete por fotograma),
  (c) responde a J/K/L girando, y
  (d) **responde a la rueda de scroll** (ajuste del autor): con el cursor
  encima, cada paso de rueda gira la manivela **un fotograma** (con ⇧,
  un segundo; con ⌥, un empalme) — moverse por el vídeo sin teclado.
  El trackpad hace scroll fino continuo (inercia incluida). La manivela
  es el objeto-firma de la sala.
- Bajo el visor, la línea de la **empalmadora**: «EMPALMADORA: MARCA CON
  B, CORTA CON B OTRA VEZ» + LUPA (slider de rombo rojo para el zoom de
  bobina).
- **Vúmetros analógicos** (dos agujas, L/R) junto al transporte — el nivel
  de audio en vivo, con física de muelle.
- **El acetato de guías**: una hoja transparente que se «pone encima» del
  vidrio (tecla `G` o su pestañita asomando por el borde): regla de
  tercios, centro y áreas seguras dibujadas en lápiz graso. Otra hoja:
  la del **encuadre** (fit/fill y el recuadro del zoom del clip).
- **La lupa cuentahílos**: mantenerla pulsada sobre el vidrio enseña la
  zona al 100 % de píxeles (comprobar foco/grano del máster).

### 3.3 La ficha del clip (derecha) — EL CAMBIO DE PANEL

El panel derecho deja de ser «8 mandos de grade» y pasa a ser **LA FICHA
DEL CLIP**: una ficha de catálogo sujeta con un clip de papel — el lugar
natural de TODAS las propiedades del clip seleccionado. Contenido, de
arriba abajo:

- **Cabecera**: miniatura del clip (atlas), nombre manuscrito
  (renombrable ahí mismo), lata de origen, y el sello del material
  («3840×2160 · 25 · HEVC»).
- **Tiempo**: TC entrada / salida **estampados** (editables a mano),
  duración resultante, y la **velocidad** como mando de gramófono
  (0.25×–4×) con sello «2×» cuando no es 1; junto a ella, **congelar
  fotograma** (una chincheta clava el fotograma actual: el clip pasa a
  ser un congelado de la duración que se pida) e **invertir** (la flecha
  al revés — reproducir hacia atrás, cuando el motor lo dé; hasta
  entonces, la casilla existe tachada: honestidad de taller).
- **Fundidos**: las dos esquinas superiores de la propia ficha están
  dobladas; arrastrar cada doblez = duración del fundido de entrada/
  salida. Un selector pequeño: encadenado / a negro / a blanco.
- **El sonido del vídeo** (sub-ficha con su interruptor de palanca):
  ganancia (galvanómetro pequeño ±12 dB), palanca **silenciar**, botón
  **despegar** (separa la banda óptica de la tira: el sonido del vídeo
  pasa a ser un clip de la banda de música, deslizable — el detach de
  toda la vida) y deslizamiento fino de sincronía (±fotogramas).
- **El croquis del encuadre**: un rectángulo a lápiz con el formato del
  proyecto y dentro, dibujado, el recuadro del zoom/posición del clip —
  se arrastra ahí mismo (mini-editor, además del gesto sobre el vidrio).
  Botones: fit / fill, **voltear ↔** (espejo), enderezar ±5° con guía de
  horizonte, reset (alt-clic).
- **La receta del cuarto oscuro en resumen**: etiquetas pegadas
  («saorín · revelado», «I-Log», «65 puntos») + los 3 galvanómetros
  principales en miniatura + una flecha a mano: «→ llévala al cuarto
  oscuro» (clic = sala 2 con ese clip).
- **Lo humano**: la **cinta washi de color** (etiqueta de color del clip:
  un trocito de washi cruza la esquina de la ficha Y la tira en la
  bobina — organizar de un vistazo) y la **nota manuscrita** (texto
  libre en Caveat, pegado con celo; asoma como pico de post-it sobre la
  tira).
- **Acciones de tampón**: duplicar (el tampón estampa una copia detrás),
  **copia de contacto** (exporta el fotograma actual a PNG en
  `~/filmlab/contactos/` — el botón es una prensa pequeña), quitar (a
  RECORTES).
- Sin clip seleccionado, la ficha muestra **el parte del proyecto**:
  formato con sello «9:16 · 1080 · 25», duración total, nº de empalmes,
  pietaje, motor activo y sus milisegundos medidos (etiqueta de
  mantenimiento del taller: «VT · 3 ms · máster»).
- Con una **foto** seleccionada: la duración por defecto editable y el
  Ken Burns de croquis (encuadre inicial → final, dos rectángulos a
  lápiz unidos por flecha) cuando llegue; con un **título**: doble clic
  edita texto y estilo ahí mismo.

### 3.4 El cubo de RECORTES

«RECORTES *(por si acaso)*»: los clips borrados caen como **tiras de
película que penden del borde del cubo, CON SUS MINIATURAS reales**
(ajuste del autor: nada de tiras negras mudas — cada recorte enseña 1–3
fotogramas del atlas según su duración) + la duración estampada y el
nombre en Caveat. **Arrastrar un recorte de vuelta a la bobina lo
rescata** (con su receta, su washi y su nota intactas). Es la papelera
visible del webview convertida en objeto — undo espacial además del ⌘Z.
El cubo se vacía con «tirar la basura» (confirmación con sello).

### 3.5 La bobina — monopista ROTULADA

La bobina sigue siendo una tira, pero el margen izquierdo se rotula a mano
(Caveat, tinta azul) con las tres bandas, cada una con SU material y SU
interruptor de palanca (mute) y su candado (lock):

- **«vídeo»** — la tira de fotogramas con perforaciones (lo que ya hay).
- **«el sonido del vídeo»** — pegada justo debajo, una banda fina estilo
  **banda óptica** (la forma de onda en tinta, dentro de la misma tira,
  como el sonido óptico de una copia de cine). Muda si el clip está
  silenciado o acelerado; despegable desde la ficha (§3.3).
- **«la música»** — una banda separada de **cinta magnética marrón** con
  la onda pintada en lápiz blanco y los puntos de la banda elástica como
  chinchetas unidas por un hilo. Material distinto = se entiende al
  primer golpe de vista qué es película y qué es cinta.
- La regla del tiempo es una **regla de madera** con números estampados;
  el **imán de herradura** junto a ella es el snap (activado por defecto;
  clic lo suelta). Los **marcadores** son **chinchetas con banderita**:
  chincheta simple = marca; con banderita = marca con texto (Caveat) y
  color; `M` planta una en la aguja.
- La aguja es un brazo metálico con contrapeso y sombra.
- Los empalmes llevan su cinta de empalme; los nombres de clip van encima
  en Caveat rojo con su washi de color si lo tiene; la esquina doblada
  de un fundido se ve TAMBIÉN en la tira (triangulito).
- **La grapadora**: seleccionar varios clips y graparlos (`⌘G`) los une
  con una grapa visible — se mueven juntos, se recortan por los
  extremos, se desgrapan con la uña (`⌘⇧G`). Es el grouping, hecho
  objeto.
- Selección múltiple: caja elástica de lápiz (arrastrar en vacío) +
  ⌘-clic; mover en bloque arrastra todo lo grapado/seleccionado.

### 3.6 El vocabulario de montaje que falta (y su gesto)

Lo mínimo de cualquier editor, con su verbo físico:

- **Insertar vs sobrescribir**: soltar una cinta SOBRE un empalme la
  inserta (los demás se apartan — ripple); soltarla SOBRE un clip lo
  sobrescribe ese tramo (aviso de tijeras). Desde el monitor de fuente:
  `,` inserta, `.` sobrescribe (los botones de la empalmadora).
- **Quitar cerrando vs dejando hueco**: `⌫` quita y cierra (ripple
  delete); `⌥⌫` deja el hueco (lift) — el hueco se dibuja como papel
  vacío con borde a lápiz «hueco de 2.3 s» y se puede rellenar o cerrar
  con un clic en sus tijeras.
- **Copiar/pegar clips** (⌘C/⌘V — pega en la aguja, con receta y todo)
  entre bobinas incluso.
- **Recortar con precisión**: además del arrastre de bordes, la ficha
  admite TC a mano; `[`/`]` recortan el borde al fotograma de la aguja
  (trim to playhead).
- **Zoom de bobina**: la LUPA + `⇧Z` ajusta la bobina entera a la
  ventana; rueda sobre la bobina con ⌘ = zoom centrado en el cursor.
- **Ir a**: `↑`/`↓` saltan de empalme en empalme (ya), `⇧M` de chincheta
  en chincheta; `Home`/`End` al principio/fin.

---

## 4. EL CUARTO OSCURO (color) — la sala fundamental

Sala propia, pantalla entera, **a oscuras**: papel negro tiza (§1.1)
desplegado sobre la pantalla, tinta de tiza y luz de seguridad, la imagen
del visor a todo color y **retroiluminada** (halo sutil). Se entra con la
ceremonia del §2. Geografía del webview, completada:

### 4.1 Los baños (izquierda)

Botellas marrones de laboratorio con etiqueta manuscrita — los 5 presets
(`presets.js`): «saorín · revelado», «La Chimera · S16», «La Chimera ·
Bolex», «CineStill 800T», «FX off». **Aplicar un baño = cogerlo y
VERTERLO sobre la imagen** (arrastrar la botella al vidrio: animación de
vertido + glu-glu + los 48 galvanómetros saltan a sus nuevos valores con
física de muelle). Clic simple = aplicar sin ceremonia. La botella activa
lleva una etiqueta roja de «EN CUBETA».

### 4.2 Los stocks (izquierda, debajo)

Cajas de película estampadas: KODAK 50D / 250D / 500T / FUJI ETERNA /
CINESTILL 800. Elegir stock = el color del negativo. La caja activa está
abierta (tapa levantada). Debajo, el susurro canónico: «no abrir la puerta
con papel dentro».

### 4.3 La cabecera de la receta (arriba)

Tres fichas: **ENTRADA** (perfil de la cámara detectada: «Insta360 Luna
Ultra · I-Log», editable — con `none` expresable para material 709),
**COLOR** (el look: «Saorín · 65 puntos»), **EL CAJÓN DE GELATINAS**: un
cajón que se desliza y muestra las gelatinas (LUTs) como rectángulos de
gel de colores; **mantener el ratón sobre una gelatina la sostiene
delante de la imagen** (preview en vivo — esto el webview no podía
hacerlo instantáneo, nosotros sí); clic la deja puesta.

### 4.4 El panel de instrumentos (derecha) — los 48 galvanómetros

El corazón de la sala. Secciones plegables con línea punteada, cada
parámetro con su **galvanómetro real**: arco graduado + aguja con física
de muelle + valor en Courier. Secciones (de `presets.js`):

- **EL REVELADO**: exposición, push/pull, compresión, rango, obturador.
- **EL COLOR DEL STOCK**: la matriz del negativo.
- **LA HALACIÓN**: halación, umbral, extensión, tono, blanqueo, velo
  (bloom), umbral del velo, calidez del velo.
- **EL GRANO**: tamaño, intensidad, sombras/altas, monocromo.
- **LA ÓPTICA**: viñeta, aberración, difusión, respiración.
- **LA MECÁNICA**: gate weave, parpadeo, polvo y pelo, inestabilidad.

Arrastrar la aguja (o **la rueda encima** — misma convención que la
manivela: la rueda mueve agujas) cambia el valor **con la imagen
respondiendo al fotograma** (el motor ya lo da). Alt-clic = valor del
baño. Doble clic = escribir el número a mano (Caveat).

### 4.5 La tira de prueba A/B y las recetas

- **TIRA DE PRUEBA A/B**: dos fotogramas grapados lado a lado (antes/
  ahora), el gesto del cuarto oscuro de verdad. Toggle con `\`.
- **La receta se calca**: botón «calcar receta» → la receta del clip queda
  en un papelito prendido con chincheta; sobre otro clip, «pegar receta»
  (o `⌘⇧C`/`⌘⇧V` de atributos, el estándar). «Pegar a todos los de la
  lata» = un solo gesto para igualar una escena.
- Abajo: transporte mínimo (play rojo + timecode + la manivela pequeña,
  que también obedece a la rueda) — se etalona viendo el plano correr.
- **La tira de contactos del proyecto**: al pie, los empalmes del
  proyecto en miniatura (atlas); clic = saltar de plano sin volver a la
  mesa. Se etalona una película entera sin salir del cuarto.

---

## 5. EL REVELADO (render) — la sala del resultado

Papel otra vez (volver de la oscuridad a la luz es parte del ritmo).
Geografía del webview, animada de verdad:

- Titular gigante «EL REVELADO» + el sol/luna según la hora.
- **El parte**: «DE LA MESA DE MONTAJE AL MÁSTER · 5 EMPALMES · 37.5 S ·
  GELATINAS: …» — la línea de resumen ya canónica.
- **ETIQUETA DE LA LATA**: campo manuscrito sobre papel con celo (nombre
  del render) + presets del máster como sellos a elegir (ProRes / H.264 /
  HEVC, loudnorm sí/no, resolución) — la «receta del baño final» impresa
  en un ticket. **Revelar solo un tramo**: si hay entrada/salida marcadas
  en la bobina, un sello extra «SOLO EL TRAMO» aparece armado.
- Botón-sello rojo **«REVELAR LA BOBINA»**.
- **Las cubetas**: revelador → baño de paro → fijador → lavado, con
  líquido animado (shader de ondulación barata). Durante el render, **la
  tira de película pasa físicamente por las cubetas** — una tira con los
  fotogramas REALES del proyecto (atlas) avanzando: la barra de progreso
  es el propio material — con el % y la ETA estampados y el burbujeo
  sonando. Cancelar = sacar la tira (arrastrarla fuera).
- **La cuerda de secado**: lo revelado cuelga de pinzas — cada render
  terminado es su **fotograma-póster real** colgado con su etiqueta
  (nombre, fecha, duración, códec, tamaño del fichero). Clic = abrir;
  **arrastrar fuera de la ventana = exportar** a donde se suelte
  (drag-out nativo). La mosca pasea por la cuerda.
- **BOBINAS REVELADAS**: las latas con el sello rojo **REVELADA** girado,
  con tinta imperfecta y la fecha — el sello se estampa con animación
  (cae, aplasta, rebota) y clac al terminar un render. Dentro de la
  tapa, la miniatura del render (regla 4 del §0: nada ciego).
- **La cola**: si se revela más de una cosa, las latas esperan en fila
  junto a las cubetas («2 esperando»); se reordenan arrastrando.
- El **colofón** abajo, como manda el zine.

---

## 6. Lo mínimo de un editor, hecho objeto (el mapa completo)

La tabla de control: cada capacidad estándar de un editor y el objeto del
taller que la encarna. Lo que no esté aquí y aparezca en un editor normal
es candidato a entrar — esta lista es la definición de «deja de ser un
juguete»:

| Capacidad estándar | Objeto del taller | Dónde |
|---|---|---|
| Trim in/out, slip | bordes de la tira + TC estampados en la ficha | §3.3, §3.6 |
| Insert / overwrite | soltar sobre empalme / sobre clip; `,`/`.` | §3.6 |
| Ripple delete / lift | `⌫` / `⌥⌫` y el «hueco» con sus tijeras | §3.6 |
| Copy / paste / duplicate | ⌘C/⌘V + el tampón | §3.3, §3.6 |
| Agrupar | la grapadora | §3.5 |
| Marcadores con nota | chinchetas con banderita | §3.5 |
| Snap | el imán de herradura | §3.5 |
| Etiquetas de color | cinta washi | §3.3 |
| Notas por clip | post-it manuscrito | §3.3 |
| Velocidad / congelar / invertir | gramófono + chincheta + flecha | §3.3 |
| Transiciones (encadenado/negro/blanco) | esquinas dobladas de la ficha | §3.3 |
| Volumen / mute por clip | galvanómetro ±dB + palanca | §3.3 |
| Detach audio | «despegar» la banda óptica | §3.3 |
| Sync fino de audio | deslizamiento ± fotogramas | §3.3 |
| Mute/solo/lock de pista | palancas y candado del margen | §3.5 |
| Transformación (zoom/pos/flip/rotar) | el croquis del encuadre | §3.3 |
| Guías / safe areas | el acetato | §3.2 |
| Zoom 1:1 del visor | la lupa cuentahílos | §3.2 |
| Navegación sin teclado | la manivela + rueda de scroll | §3.2 |
| Export still | la copia de contacto | §3.3 |
| Render de un tramo | el sello «SOLO EL TRAMO» | §5 |
| Cola de renders | la fila de latas | §5 |
| Historial de renders | la cuerda de secado | §5 |
| Copy/paste de grade | calcar/pegar la receta | §4.5 |
| Presets de grade | los baños | §4.1 |
| LUT preview en vivo | sostener la gelatina | §4.3 |
| Before/after | la tira A/B grapada | §4.5 |
| Autosave + versiones | **el archivador**: un cajón en ajustes con las copias de seguridad rotatorias, cada una una carpetita con fecha manuscrita; abrir una = restaurar (con copia previa de lo actual) | §7 |
| Undo espacial | el cubo de RECORTES con miniaturas | §3.4 |
| Bins / carpetas de material | las estanterías con etiqueta manuscrita | §2bis.2 |
| Importar carpeta como bin | soltar la carpeta = una balda nueva (⌥ = una por subcarpeta) | §2bis.2 |
| Refrescar un bin vinculado | «volver a mirar» la balda enchufada | §2bis.2 |
| Auto-clasificar al importar | el clasificador: LA DISCOTECA y EL ÁLBUM | §2bis.3 |
| Buscar / filtrar material | pestañitas de filtro + la etiqueta de buscar | §2bis.3 |
| Proyecto portable | la bobina: un fichero que refiere, rutas dobles | §2bis.1 |
| Media offline / relink | la pizarra de claqueta + «buscar…» recursivo | §2bis.1, §3.1 |
| Atajos descubribles | la chuleta (`?`) + etiquetas colgantes (tooltips de cartón con el atajo) | ya + §8 |

---

## 7. Cosas originales que el webview NO tenía

Lo nativo permite lo que el navegador no podía. Ideas propias, no calco:

1. **La manivela con inercia, trinquete y rueda de scroll** (§3.2) — el
   scrub como objeto; la rueda del ratón como manivela.
2. **Sostener la gelatina** (§4.3) — preview de LUT en vivo al hover.
3. **Verter el baño** (§4.1) — presets como líquidos, los galvanómetros
   saltando con física.
4. **La tira pasando por las cubetas** como barra de progreso, con los
   fotogramas reales del proyecto.
5. **El papel con memoria**: semilla por proyecto + marcas de uso — donde
   más se trabaja, el papel se gasta. Ningún editor tiene un workspace
   que envejece contigo.
6. **El papel negro tiza**: el cuarto oscuro no es un tema oscuro, es
   OTRO PAPEL que se despliega y recubre la pantalla (§2).
7. **La cinta de 6 fotogramas** que se desenrolla de la lata al primer
   toque — hoja de contactos convertida en gesto y en navegación (§3.1).
8. **El contador mecánico** de timecode + el **pietaje** (metraje en
   pies+fotogramas, el contador secundario de cine de verdad, en el
   parte del proyecto).
9. **Vúmetros de aguja** con física (L/R) en la mesa.
10. **El cubo de recortes como undo espacial**, con miniaturas —
    rescatar cortes arrastrándolos de vuelta.
11. **La grapadora** (§3.5) — agrupar clips con una grapa visible.
12. **Los susurros contextuales**: las notas manuscritas del margen
    rotan y responden al uso real (si nunca has usado `B`, la nota de la
    empalmadora se subraya sola; si un render falló, la mosca se posa en
    la lata y la nota dice qué pasó). Ayuda que no parece ayuda: parece
    que alguien dejó notas en TU taller.
13. **El parte del motor como etiqueta de mantenimiento** («VT · 3 ms ·
    máster · 50 Hz») — la velocidad, que es el orgullo de la casa, a la
    vista como la chapa de revisión de una máquina.
14. **Sol/luna y la hora del taller** — detalles que hacen lugar.
15. **La pared del autor**: fotos propias pegadas con celo en la portada
    y la mesa, gestionadas desde el panel LA PARED en ajustes o
    arrastrándolas directamente (§1.4).
16. **Arrastrar un render desde la cuerda** al Finder/Explorer.
17. **El archivador de versiones** (§6) — time machine de andar por
    casa, visible y sin miedo.
18. **Las baldas enchufadas** (§2bis.2): estanterías que recuerdan su
    carpeta de origen y se refrescan con un gesto — bins que no mienten
    sobre de dónde viene el material.

---

## 8. Primitivas técnicas nuevas (lo que hay que construir una vez)

| # | Primitiva | Qué es | Coste |
|---|-----------|--------|-------|
| P1 | `papel.wgsl` | fbm + fibra + manchas por semilla + paleta por sala + **material tiza** (uniforme de sala) | 1 pase fullscreen barato (sustituye al pase actual del papel) |
| P2 | `trazo.rs` | polilíneas wiggly con semilla estable, grosor variable, flechas/círculos/corchetes; cacheadas en vertex buffers | generación en resize; dibujo = triángulos normales |
| P3 | Quads rotados | ángulo por quad en `Lienzo`/`Atlas` (2 floats más por vértice) | trivial |
| P4 | `galva` widget | arco + aguja con muelle (solo redibuja mientras se mueve); sirve para galvanómetros, vúmetros y la ganancia de la ficha | por-widget |
| P5 | `sello` | estampado con textura de tinta imperfecta + animación caer/aplastar | atlas + 3 frames |
| P6 | Doodle atlas | §1.4, horneado en build (Pillow/Blender) desde assets de `studio/` + nuevos | asset estático |
| P7 | Transición de sala | render-a-textura + pliegue (2 mitades con sombra de doblez) encadenable: plegar hueso → clac → desplegar tiza (§2) | 1 rtt solo durante la transición |
| P8 | Líquido | sin() + ruido en las cubetas, solo en sala revelado | trivial |
| P9 | Contador mecánico | dígitos en fichas con flip (atlas de dígitos) | trivial |
| P10 | Salas | `enum Sala` con layout/papel/objetos propios; el estado (clip, TC) viaja; `main.rs` se parte en `sala_mesa.rs`, `sala_cuarto.rs`, `sala_revelado.rs` | refactor |
| P11 | Rueda-como-mando | routing de scroll por zona: manivela (fotogramas), agujas (valores), bobina+⌘ (zoom), listas (scroll normal) | tabla de hit-testing que ya existe |
| P12 | Drag-out nativo | arrastrar render/fotograma fuera de la ventana (NSDraggingSource / DoDragDrop) | por plataforma, solo 2 sitios |

**Regla de rendimiento**: todo lo animado (agujas, manivela, líquido,
mosca) pide redraw SOLO mientras se mueve; en reposo la app sigue sin
pintar. El papel procedural sustituye al tile actual (mismo pase). El
trazo se cachea. Las miniaturas (latas, cintas de 6, recortes, cuerda,
tira-progreso) salen TODAS del atlas existente — cero decodes nuevos en
el camino interactivo. Presupuesto: cero regresión en los números de
VELOCIDAD.md (scrub ≤ 4 ms proxy, pausa→play ≤ 4 ms).

---

## 9. Plan de obra (orden de dependencia)

- **S1 — Los materiales**: P1 papel procedural (hueso + tiza) + P2 trazo +
  P3 rotación + P6 atlas de doodles. La mesa actual se re-viste con ellos
  (sin mover layout todavía). *Aceptación*: captura de la mesa nativa con
  papel vivo, TODAS las líneas a pulso, latas metálicas con miniatura,
  doodles y titular fuera de registro — al lado de la captura del correo,
  la nativa se ve MÁS llena.
- **S2 — Las salas**: P10 + P7. Portada re-vestida, tres salas navegables;
  pliegue de papel entre salas de papel; la ceremonia completa del cuarto
  oscuro (plegar hueso → clac → desplegar tiza). *Aceptación*: `1`/`2`/`3`
  cambian de sala con transición; el cuarto oscuro está recubierto de
  papel tiza con la imagen encendida.
- **S3 — La mesa completa**: el modelo de material entero (§2bis:
  estanterías-balda, carpeta→balda, «volver a mirar», clasificador,
  pestañitas de filtro, etiqueta de buscar), ficha del clip entera
  (§3.3, TODAS las propiedades), latas con cinta de 6 fotogramas,
  pistas rotuladas con palancas y candados, manivela con inercia +
  rueda (P11), cubo de recortes con miniaturas, regla de madera + imán
  + chinchetas, vúmetros, contador mecánico, acetato, lupa, grapadora,
  vocabulario §3.6. *Aceptación*: soltar una carpeta y verla nacer como
  balda; montar un vídeo entero sin tocar el teclado (solo objetos y
  rueda), y con teclado sin tocar el ratón.
- **S4 — El cuarto oscuro entero**: baños-botella (verter), stocks-caja,
  cajón de gelatinas (sostener = preview), panel de 48 galvanómetros por
  secciones (rueda incluida), A/B grapado, calcar/pegar receta +
  «a todos los de la lata», tira de contactos al pie. *Aceptación*:
  reproducir la receta de la captura del correo tocando solo
  instrumentos; el A/B responde al fotograma; etalonar 3 planos sin
  salir de la sala.
- **S5 — El revelado**: cubetas animadas, tira-progreso con fotogramas
  reales, etiqueta + sellos del máster + «SOLO EL TRAMO», cola de latas,
  cuerda de secado con drag-out (P12), sello REVELADA, colofón.
  *Aceptación*: revelar de verdad y ver la tira pasar; el render cuelga
  de la cuerda y se arrastra al Finder.
- **S6 — El alma**: susurros contextuales, sonidos por sala, papel con
  memoria, sol/luna, pietaje, la pared del autor (con su panel LA PARED
  en ajustes y el pegado por arrastre), la mosca, etiquetas colgantes
  (tooltips), el archivador de versiones. *Aceptación*: dejar la app
  abierta 5 minutos y que se sienta un LUGAR; pegar una foto propia en
  la pared desde ajustes y verla en la portada.

Cada S se verifica contra la app real (capturas + gestos guionizados),
se despliega en Mac y GPD, y se documenta en TRASPASO.md. Los documentos
HITO1.md y HERRAMIENTA.md siguen vigentes para las capacidades; NORTE.md
manda sobre la forma Y ahora también sobre el vocabulario mínimo (§6).
Donde choquen, **primero la forma de las salas**, porque es la
arquitectura que el resto habita.

---

## 9bis. ESTADO (1-ago, rondas 35–44 del TRASPASO)

**Hecho y verificado en la app real**: S1 materiales (papel procedural
hueso/frío/tiza, trazo a pulso universal, quads rotados, atlas de
objetos) · S2 las tres salas con pliegue y apagón · S3 la mesa (ficha
del clip, cinta de 6, cubo de recortes con miniaturas, bandas rotuladas
con palancas, manivela con rueda, marcas-chincheta, acetato, contador
mecánico, vúmetros, grapadora, notas, §2bis baldas con carpeta=balda /
rescan / filtros / scroll, **la barra de la bobina** con rueda,
⌘+rueda anclada y seguimiento de la aguja) · S4 el cuarto oscuro entero
(37 galvanómetros en 6 baterías, baños que se vierten, stocks, cajón de
gelatinas, calcar/pegar receta, tira de contactos, A/B) · S5 el revelado
(cubetas animadas, la tira de fotogramas como progreso, cuerda-galería,
cola, sello REVELADA, **ruta del máster elegible**) · S6 el alma
(susurros contextuales, la pared en ajustes, archivador, sol/luna,
mosca, **ambientes sonoros por sala**, ducking) · y de §6: J/K/L, lupa
cuentahílos ×4, caja de selección elástica, **arrastrar el máster fuera
de la ventana** (NSDraggingSession / DoDragDrop de verdad).

**Pendiente**: el pliegue con render-a-textura (hoy es una hoja que
barre, no la hoja doblada), el scrub audible, y el resto de detalles
finos marcados en HITO1/HERRAMIENTA.

**Regla de velocidad que dejó esta obra**: lo que se sube a la GPU se
recuerda. Ver VELOCIDAD.md (addendum): la receta del cuarto oscuro
cacheada bajó la cadena de 750 ms a 0,8 ms por fotograma.

## 10. Lo que NO es este documento

- No es un reskin: cambia la arquitectura (salas), el panel derecho
  (ficha del clip con TODAS las propiedades), la bobina (tres bandas
  rotuladas con materiales distintos, palancas, candados, chinchetas) y
  añade objetos con comportamiento (manivela con rueda, baños,
  gelatinas, cubo, cuerda, grapadora, imán, acetato, archivador).
- No es una maqueta: cada objeto listado tiene su función y su criterio
  de aceptación; la tabla del §6 es la definición operativa de «deja de
  ser un juguete».
- No es negociable la velocidad: si un adorno cuesta un milisegundo del
  camino interactivo, se cachea o se cae.

## §9ter — EL CUBO DE RECORTES, de cajón a mesa auxiliar

El cubo era una papelera con memoria: tope de doce, se veían tres y solo
sabía devolver a la aguja. Ahora es lo que tenía que ser desde el principio
— **la mesa auxiliar donde uno deja un trozo mientras hace hueco**:

- **Sin fondo.** No hay tope. Se recorre con la rueda encima, en rejilla de
  tres columnas, y lo último que apartaste está arriba (que es lo que uno
  busca). Contador a la derecha del rótulo y barrita de recorrido.
- **Arrastrar un clip al cubo lo aparta**: sale de la bobina y se guarda.
  Mientras se lleva encima, el cubo se ilumina y pone «¡suelta aquí!».
- **Arrastrar del cubo a la bobina lo coloca DONDE SE SUELTA**, con la
  regla de siempre: si el punto cae en la mitad derecha del clip que hay
  debajo, entra después (sin eso no había forma de dejarlo al final).
  Un **clic** sin mover sigue devolviéndolo a la aguja, que es más rápido
  cuando da igual dónde.

El gesto que esto habilita, y que era el que faltaba: corto los últimos
segundos de un plano, **los arrastro al cubo**, corto por la mitad, hago
hueco, y **traigo el recorte al sitio nuevo**. Sin portapapeles, sin
deshacer, sin perder de vista lo que aparté.
