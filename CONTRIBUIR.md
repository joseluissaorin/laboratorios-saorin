# Contribuir

Gracias por mirar. Antes de nada, una advertencia honesta: esto es **software
de autor**, hecho para dos máquinas concretas y usado a diario por una
persona. No hay hoja de ruta pública ni compromiso de mantenimiento. Si eso
te vale, adelante.

## Lo más útil que puedes hacer

**Contar en qué máquina lo has probado.** El proyecto solo se ha medido en un
MacBook Pro M4 Max y en un GPD Win Max 2 (Radeon 890M). Cualquier dato de
otra GPU —sobre todo NVIDIA, Intel Arc o Apple Silicon pequeño— vale más que
un parche.

Al abrir una incidencia, di:

- máquina, sistema y GPU;
- el códec y la resolución del material (`ffprobe` basta);
- lo que sale por consola: el diario del revelado dice quién manda
  (`esperando fuentes · componer · esperando al codificador`).

## Si tocas el look

**Mídelo.** Hay banco de aceptación: revelar el mismo tramo dos veces tiene
que dar fotogramas **idénticos al bit**, y cualquier cambio en la cadena se
compara contra una referencia con PSNR.

```bash
# revelar 24 fotogramas a ProRes 4444 y comparar con la referencia
ffmpeg -i antes.mov -i despues.mov -lavfi "[0:v][1:v]psnr" -f null -
```

La tolerancia de la casa es **1 valor de código sobre 1023** (unos 60 dB).
Por debajo de eso, no es «reorganizar cómo se calcula»: es cambiar el look, y
eso lo decide el autor.

Y una regla que ya ha salvado el proyecto tres veces: **si el plan y la
medición no coinciden, gana la medición**. Está documentado en `MOTOR.md`
§9bis, incluidas las veces que el plan era mío y estaba mal.

## Si tocas la interfaz

Lee [NORTE.md](NORTE.md) primero. No es decoración: es el contrato. Las
metáforas mandan sobre la implementación, y hay cosas que no se negocian —
nada de paneles grises, nada de esquinas perfectas, nada que parezca un
formulario.

## Si tocas el motor

Lee [ARQUITECTURA.md](ARQUITECTURA.md) §7, «las trampas». Casi todo lo que
parece un fallo tonto en el motor de vídeo es una de ellas, y están
documentadas con lo que costó encontrarlas.

Regla dura: **el uniforme y su `min_binding_size` los cuenta el compilador**
(`params::bytes_uniforme::<T>()`), nunca a mano. Ese número se escribió a
mano en cinco sitios y mordió tres veces en una sola sesión.

## Estilo

El código y los comentarios están **en español**, y los comentarios explican
*por qué*, no *qué*. Un comentario que repite lo que hace la línea sobra; uno
que dice «esto se probó de la otra forma y era 40 % más lento» vale oro.

Los nombres son del oficio, no de la informática: `bobina`, `renglón`,
`gelatina`, `carrerilla`, `cubo de recortes`. Si añades una pieza, ponle el
nombre que tendría en un taller.

## Licencia

Al contribuir aceptas que tu aportación se publique bajo MIT, como el resto.
