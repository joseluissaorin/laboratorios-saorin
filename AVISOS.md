# Lo que hay dentro y de quién es

El código de este repositorio es **MIT** (ver `LICENSE`). Pero un editor de
vídeo no es solo código: lleva tipografías, una curva de color y una placa de
grano. Esto dice de quién es cada cosa.

## Tipografías — SIL Open Font License 1.1

Van empotradas en el binario (`nativa/assets/fonts/`) y también como `.woff2`
en la interfaz web (`studio/zine/fonts/`). Las tres son de Google Fonts y las
tres son OFL, que permite empotrarlas y redistribuirlas:

| tipografía | autoría | dónde se usa |
|---|---|---|
| **Space Grotesk** | Florian Karsten | los rótulos, la voz que grita |
| **Fraunces** | Undercase Type (Phaedra Charles, Flavia Zimbardi) | el texto con serifa |
| **Caveat** | Impallari Type (Pablo Impallari) | la letra a mano de los márgenes |

El texto de la licencia está en `nativa/assets/fonts/OFL.txt`.

## La gelatina de color — «Saorín · 65 puntos»

`studio/luts/color/Saorín · 65 puntos.cube` es **obra del autor** y va con el
mismo MIT que el resto. Es la firma del taller: sin ella el proyecto compila
y funciona, pero no se parece a lo que enseña la web.

Si la usas en un trabajo tuyo, MIT no te obliga a nada más que conservar el
aviso de copyright — pero se agradece el crédito.

## La gelatina de entrada — no hay

`studio/luts/entrada/Directo · sin transformar.cube` es la identidad: no toca
la señal.

**A propósito.** Las transformadas log→709 (S-Log3, Apple Log, V-Log, I-Log…)
son de los fabricantes de cámara, no nuestras, y no se redistribuyen aquí.
Si tu material viene en una curva log, pon el `.cube` que dé tu fabricante en
`<taller>/luts/entrada/` y el revelado lo aplicará antes del grade.

## La placa de grano

`app/ui/assets/grain.bin` **no es una foto de grano de nadie**: se sintetiza
con `tools/make_grain.py` (ruido gaussiano periódico por FFT, con semilla
fija). Se puede regenerar entera con ese script. Es matemática, no material
escaneado.

## Dependencias externas

- **ffmpeg** — el taller lo llama para el sonido y para muxar. No se
  distribuye aquí; se espera encontrarlo en el `PATH`. ffmpeg es LGPL/GPL
  según cómo esté compilado: si redistribuyes un binario del taller con
  ffmpeg dentro, revisa bajo qué licencia está el que empaquetas.
- **VideoToolbox** (macOS) y **Media Foundation** / **AMF** (Windows) son
  del sistema operativo. Se usan por sus APIs públicas.
- El resto son crates de Rust y paquetes de npm con sus propias licencias,
  declaradas en los `Cargo.toml` y `package.json`.

## El oído (transcripción)

`shell/` enlaza **[whisper.cpp](https://github.com/ggerganov/whisper.cpp)**
(MIT, de Georgi Gerganov) a través del crate `whisper-rs`. **Los modelos no
se distribuyen aquí**: se bajan la primera vez que se usan desde el
[repositorio de whisper.cpp en Hugging Face](https://huggingface.co/ggerganov/whisper.cpp)
y se guardan en `<taller>/modelos/`. Son los pesos de **Whisper**, de OpenAI,
publicados bajo MIT. Nada de lo que se transcribe sale de la máquina.

## Lo que NO está en el repositorio

`assets/` (los restos de los experimentos), el metraje de rodaje y los
másteres. Nada de eso hace falta para compilar ni para usar el programa: eran
las sobras de ir probando con material real, y ese material es privado.
