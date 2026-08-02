# La arquitectura, por dentro

Este documento es para quien vaya a tocar el código. Explica **cómo viaja un
fotograma** desde el fichero hasta la pantalla y hasta el máster, y por qué
cada pieza está donde está.

---

## 1. El mapa

```
                    ┌──────────────────────────────────────┐
   el autor  ─────► │  nativa/  LA APLICACIÓN              │
                    │  winit + wgpu · las tres salas       │
                    └───────┬──────────────────┬───────────┘
                            │                  │
                   lee/escribe            manda revelar
                            │                  │
                    ┌───────▼──────┐   ┌───────▼────────────┐
                    │ <taller>/    │   │  shell/  EL TALLER │
                    │  media/      │   │  compila la bobina │
                    │  projects/   │   │  hornea el sonido  │
                    │  luts/       │   │  cachea por tramos │
                    │  out/        │   └───────┬────────────┘
                    └──────────────┘           │
                                        llama al motor
                                               │
                          ┌────────────────────┴──────────────────┐
                          │                                       │
                  ┌───────▼─────────┐                   ┌─────────▼───────┐
                  │  metal/  MAC    │                   │ winlab/ WINDOWS │
                  │  VideoToolbox   │                   │  MF + AMF       │
                  │  + Metal        │                   │  + D3D12/wgpu   │
                  └───────┬─────────┘                   └─────────┬───────┘
                          └──────────────┬────────────────────────┘
                                         │
                                 ┌───────▼────────┐
                                 │  core/         │
                                 │  índice MP4    │
                                 │  shaders WGSL  │
                                 │  plan de bobina│
                                 └────────────────┘
```

---

## 2. Cómo se lee un fotograma

### El índice propio (`core/src/indice.rs`)

No se usa ffmpeg para navegar. Se lee el `moov` del MP4 —saltando el `mdat`
entero— y se construye una tabla de muestras con `(offset, tamaño, pts,
¿clave?)` más el orden de presentación. Con eso:

- **buscar es O(1)**: `muestra_en(t)` y `keyframe_para(i)` son índices;
- **el orden de pantalla está resuelto**: `orden_pts` reordena los
  fotogramas B, que vienen en el fichero en orden de decodificación;
- no hay proceso externo, ni tubería, ni parseo de texto.

### La fuente con residencia en GPU (`metal/src/fuente.rs`)

Une el índice con el decodificador de hardware. `busca(t)` salta al
fotograma clave anterior y **decodifica el arranque tirándolo** —esos
fotogramas no llegan a tocar la GPU del look—; `siguiente()` entrega en
orden de pantalla; el fotograma sale en un `CVPixelBuffer` que se importa
como textura **sin copiar un byte**.

> **Trampa.** Hay que pedirle a VideoToolbox el formato explícitamente
> (`x420` para HEVC 10 bits). Si no, devuelve `p420` empaquetado y la
> importación como R16/RG16 lee basura.
>
> **Trampa peor.** El `CVPixelBuffer` no se puede soltar hasta que la GPU
> haya terminado de leerlo. VideoToolbox lo recicla de su pool y el look
> acaba revelando **otra imagen**. Este fallo estuvo meses en el motor: el
> máster divergía a partir del séptimo fotograma, y de forma distinta en
> cada ejecución.

---

## 3. La cadena fílmica

Un fotograma pasa por dos grupos de pases. Los shaders viven **una sola
vez**, en WGSL, en `core/src/shaders/`; en el Mac se traducen a Metal con
`naga` en tiempo de compilación (`metal/build.rs`).

```
  YUV 10 bits ─► REVELAR ─► pirámide de desenfoques ─► COMPONER ─► P010
                    │            (½, ¼, ⅛)               │
                    │                                    │
        conform + encuadre + giro               halación · floración
        gelatina de entrada                     grano · viñeta
        curva del negativo                      aberración · tejido
        gelatina de color                       polvo · parpadeo
        obturador (IIR, fundido aquí)           fundido a color
```

**El pase de revelado escribe un solo destino.** Antes escribía dos —el
segundo, `raw`, para la cortinilla del comparador— y en el máster nadie lo
leía: 66 MB por fotograma tirados en 4K.

**El obturador va fundido dentro del revelado**, no en un pase aparte. Es un
filtro IIR de una línea que costaba otro viaje completo a memoria. Y encaja
con los encadenados sin cuidado especial porque **es lineal**:

```
   mix(mix(A,h,k), mix(B,h,k), p)  ==  mix(mix(A,B,p), h, k)
```

**Los intermedios son `RGB10A2Unorm`**, 4 bytes. No 16 bits flotantes (8
bytes, la mitad del ancho de banda tirado) ni `RG11B10Float` (4 bytes pero
6 bits de mantisa: se separa 43 dB del original, medido). Después de las
gelatinas toda la señal vive en [0,1] y el grano ditherea lo que quede.

---

## 4. El plan de bobina

`core/src/plan.rs` compila la bobina —una lista de clips con sus tiempos— a
una **tabla plana con un renglón por fotograma de salida**:

```rust
struct Renglon {
    fuente_a: u32,      // de qué fuente sale
    fuente_b: u32,      // el otro lado del encadenado (NINGUNA si no hay)
    peso_b: f32,        // cuánto pesa el segundo lado
    t_a: f64, t_b: f64, // en qué segundo de cada fuente
    color_fijo: f32,    // negro=0, blanco=1
    nivel_color: f32,   // cuánto fundido
}
```

Se compila **una vez, antes de arrancar**. A partir de ahí el motor no
decide nada por fotograma: lee el renglón que toca y ejecuta. Consecuencias:

| | |
|---|---|
| **el corte** | que el renglón siguiente mire a otra fuente. Cero coste. |
| **el encadenado** | dibujar la segunda fuente encima con mezcla por alfa, dentro del mismo pase de revelado. |
| **el fundido a negro** | una constante. Ni segunda fuente ni segundo decodificador. |
| **el hueco** | una fuente sintética: el encuadre se manda fuera de rango y sale negro sin decodificar nada. |
| **la conversión de cadencia** | el renglón pide *el fotograma que cubre ese instante*. Una bobina a 25 con material a 59,94 sale bien por construcción. |

La rejilla es la del proyecto, así que **no hay deriva acumulada** — que era
justo el motivo por el que antes existía un corte con recodificación.

---

## 5. La caché, por tramos

La bobina se trocea en **un tramo por clip** (con su junta incluida) y cada
uno se cachea **por su contenido**: qué fuente, en qué segundo, con qué
receta y con qué encuadre. La clave **no lleva la posición en la bobina**, así
que un tramo sigue valiendo aunque lo de delante cambie de duración.

```
   primera vez     3 tramo(s): 3 revelados, 0 del cajón
   tocando el 3º   3 tramo(s): 1 revelado,  2 del cajón
```

Dos detalles que lo hacen funcionar:

- **Carrerilla.** Cada tramo se revela con unos fotogramas de más por delante
  que **no se escriben**, para que el obturador llegue a su primera imagen
  con el arrastre ya formado. Sin eso se ve el escalón en cada juntura.
- **Los tramos se pegan con `concat -c copy`**, sin recodificar, porque cada
  uno empieza con fotograma clave.

Si algo no cuadra —un tramo que falla, o una bobina que se trocearía en más
de 40— se cae al revelado de un tirón, que siempre funciona.

---

## 6. La interfaz, dibujada como vídeo

No hay framework de UI. `nativa/src/ui.rs` es un pintor inmediato de cuatro
capas sobre `wgpu`:

```
   papel → lienzo → atlas(minis) → objetos → cinta → tipos → visor
        → lienzo2 → atlas2 → tipos2 → telón (transiciones entre salas)
```

- **El papel** (`papel.rs`) es procedural: fibra por fbm, grano, viñeta,
  manchas y cerco de café. La **semilla sale del nombre del proyecto**, así
  que cada bobina tiene su hoja y siempre la misma.
- **Los trazos** (`trazo.rs`) son polilíneas con temblor determinista y
  presión variable, con sangrado de tinta en los extremos. Una caja dibujada
  a pulso en vez de un rectángulo.
- **Las miniaturas** viven en un atlas de GPU con un hilo que las va
  sacando; nunca bloquean el dibujo.

Que la interfaz use el mismo pipeline que el vídeo no es una excentricidad:
significa que **no hay dos presupuestos de fotograma**, y que la ventana
entera cuesta lo que cuesta un fotograma de 4K.

---

## 7. Las trampas, en un sitio

Las que costaron horas y no son evidentes. La lista larga está en
`TRASPASO.md`.

**VideoToolbox / macOS**
- Soltar el `CVPixelBuffer` de entrada antes de que la GPU termine → el pool
  lo recicla y se revela otra imagen. Vale para decodificar *y* codificar.
- `CMSampleBufferCreateReady` con cero entradas de tiempo **no guarda el
  pts**: el callback recibe un tiempo inventado.
- Con decodificación asíncrona el callback llega **desordenado**.
- Cada NAL suelta no es un fotograma: hay que agrupar en unidades de acceso
  (`first_slice_segment_in_pic_flag`).
- **Dos motores de HEVC son más lentos que uno** (153 → 110 fps): se pelean
  por el mismo bloque del chip. ProRes sí tiene dos de verdad.

**Windows**
- Media Foundation recicla la textura del pool en cuanto se suelta el
  sample: hay que copiarla al anillo propio y esperar la valla.
- `GradeU` mide **128 bytes** allí y 112 en el Mac (lleva una fila más). Si
  el `min_binding_size` no cuadra, wgpu **tumba el pipeline entero** y no
  arranca ningún camino. Por eso el tamaño lo cuenta ahora el compilador:
  `params::bytes_uniforme::<T>()`.
- ffmpeg es **posicional**: las opciones de una entrada van *antes* de su
  `-i`. Un `-b:a` suelto delante de un fichero se toma como opción de esa
  entrada y aborta el mux.
- La carrerilla hay que **descontarla de la valla del anillo del
  codificador** o el tramo se bloquea esperando un testigo que no llega.

**Los dos**
- El ancho de banda no era el límite: **manda el codificador**. Antes de
  optimizar la cadena fílmica, medir.
