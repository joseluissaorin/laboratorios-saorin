import { useState } from 'react';

// LAS TRES SALAS. Es lo único de la página que pide interacción de verdad:
// enseñar tres capturas grandes una tras otra sin hacer bajar tres pantallas.
// Todo lo demás es HTML servido, sin JavaScript.
const SALAS = [
  {
    id: 'mesa',
    nombre: 'La mesa de montaje',
    lema: 'Donde se corta.',
    cap: '/cap/mesa.jpg',
    alt: 'La mesa de montaje: la estantería de material a la izquierda, el visor en el centro y la bobina abajo',
    puntos: [
      ['La bobina', 'Los clips en una tira de película con sus perforaciones. Se arrastran, se recortan por los bordes y se estiran otra vez para recuperar lo que quitaste.'],
      ['La cuchilla', 'Se coloca antes de cortar y se ve dónde va a morder. Se pega al empalme o a la marca más cercana, y corta vídeo o música según lo que tengas elegido.'],
      ['El cubo y la papelera', 'El cubo guarda lo que aún no sabes dónde poner. La papelera tira — y acepta clips, recortes y cintas de la estantería.'],
      ['El sonido', 'Dos pistas de música con envolvente elástica y ducking, el audio del vídeo desacoplable a su propia pista, y vúmetros de L y R antes de entregar.'],
    ],
  },
  {
    id: 'cuarto',
    nombre: 'El cuarto oscuro',
    lema: 'Donde se decide el color.',
    cap: '/cap/cuarto.jpg',
    alt: 'El cuarto oscuro con luz de seguridad roja: los baños, los stocks y el panel de instrumentos',
    puntos: [
      ['Cincuenta y dos agujas', 'El look entero en galvanómetros de laboratorio: exposición, hombro de altas luces, halación, floración, grano por zonas, óptica, mecánica del proyector.'],
      ['Los baños y los stocks', 'Recetas completas —Kodak 50D, 250D, 500T, Fuji Eterna, CineStill— que se vierten sobre el clip y se ajustan encima.'],
      ['Las gelatinas', 'LUT de entrada y de color, en .cube de cualquier tamaño, interpoladas por tetraedros: una .cube se ve aquí como se ve en Resolve.'],
      ['El filtro ND, deshecho', 'Los ND dejan pasar infrarrojo y el rojo del sensor lo recoge: los negros se van a granate. Se quita el rojo que sobra sin tocar el que hay.'],
    ],
  },
  {
    id: 'revelado',
    nombre: 'El revelado',
    lema: 'Donde sale el máster.',
    cap: '/cap/revelado.jpg',
    alt: 'La sala de revelado: los cuatro sellos, la regla del rango, las cubetas del baño y la cuerda de secado',
    puntos: [
      ['Cuatro sellos', 'REVELAR es el camino rápido y no mira nada más. ARCHIVO saca ProRes. EN CLIPS deja una carpeta con un fichero por plano. A MANO abre el cajón.'],
      ['El cajón del máster', 'Tamaño hasta 8K, supermuestreo, códec, caudal, filtro de escalado y cadencia. Un 8K de un 4K hace que la plataforma no se coma el grano.'],
      ['El rango', 'Una regla con la bobina entera, las juntas marcadas y dos tiradores: se revela sólo el trozo que quieras.'],
      ['La cuerda de secado', 'Lo revelado se cuelga ahí. Y la caché es por tramos: cambiar el color de un plano recalcula ese plano y nada más.'],
    ],
  },
];

export default function Salas() {
  const [activa, setActiva] = useState(0);
  const s = SALAS[activa];

  return (
    <div className="salas">
      <div className="salas-pestanas" role="tablist" aria-label="Las tres salas del taller">
        {SALAS.map((x, i) => (
          <button
            key={x.id}
            role="tab"
            aria-selected={i === activa}
            className={i === activa ? 'activa' : ''}
            onClick={() => setActiva(i)}
          >
            {x.nombre}
          </button>
        ))}
      </div>

      <div className="salas-cuerpo">
        <figure className="salas-cap">
          <img src={s.cap} alt={s.alt} width="1482" height="940" loading="lazy" />
        </figure>

        <div className="salas-texto">
          <p className="salas-lema mano">{s.lema}</p>
          <dl>
            {s.puntos.map(([t, d]) => (
              <div key={t}>
                <dt>{t}</dt>
                <dd>{d}</dd>
              </div>
            ))}
          </dl>
        </div>
      </div>

      <style>{`
        .salas-pestanas { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 24px; }
        .salas-pestanas button {
          font-family: 'Space Grotesk', sans-serif; font-weight: 700; font-size: 14px;
          padding: 10px 18px; border: 1.5px solid var(--papel-borde); background: transparent;
          color: var(--tinta-tenue); border-radius: 2px; cursor: pointer;
          transition: color .12s ease, border-color .12s ease;
        }
        .salas-pestanas button:hover { color: var(--tinta); }
        .salas-pestanas button.activa { color: var(--rojo); border-color: var(--rojo); }
        .salas-cuerpo { display: grid; grid-template-columns: 1.45fr 1fr; gap: 34px; align-items: start; }
        @media (max-width: 980px) { .salas-cuerpo { grid-template-columns: 1fr; } }
        .salas-cap { margin: 0; }
        .salas-cap img {
          width: 100%; height: auto; display: block; border-radius: 3px;
          border: 1px solid var(--papel-borde);
          box-shadow: 0 18px 44px rgba(29, 27, 22, 0.16);
        }
        .salas-lema { font-size: 26px; color: var(--rojo); margin: 0 0 14px; }
        .salas-texto dl { margin: 0; }
        .salas-texto dt {
          font-family: 'Space Grotesk', sans-serif; font-weight: 700; font-size: 16px;
          margin-top: 18px;
        }
        .salas-texto dd { margin: 5px 0 0; font-size: 16px; color: var(--tinta-tenue); line-height: 1.5; }
      `}</style>
    </div>
  );
}
