# Cómo funciona

Dársena separa tres cosas que otros launchers mezclan.

## 1. El catálogo

La lista de juegos y de sus ports: nombre, consola, portada, sinopsis, tráiler, repositorio oficial y qué archivos
del juego original necesita cada port. La mantiene el autor de Dársena y se actualiza sola al abrir el launcher.

- **Juego y port no son lo mismo.** Un juego (por ejemplo *Gears of War 2*) puede tener **varios ports** de distintos
  autores. En la ficha ves el juego; en la biblioteca eliges qué port instalar.
- La mayoría de ports se descargan de las **versiones oficiales de GitHub** de su autor.
- Algunos se publican fuera de GitHub (**ports externos**): para esos hace falta una [[fuente|Fuentes]] que los traiga.
  En la biblioteca muestran *«Solo fuentes»* y, si ninguna de tus fuentes los tiene, el botón dice
  **NECESITA UNA FUENTE**.

## 2. Las fuentes

Listas JSON de enlaces de descarga (torrent, magnet o directos) que **añades tú**. Dársena no trae ninguna. Sirven para dos cosas:

- **Ports**: mirrors de un port de GitHub o la única forma de bajar un port externo.
- **Datos del juego**: tus propias copias de los juegos, desde donde las tengas subidas.

Más en **[[Fuentes|Fuentes]]**.

## 3. Tus datos

Ningún port incluye el juego original: cada uno pide **tu copia** (ROM, ISO, carpeta del disco…). Dársena te dice qué
formato acepta cada port y, si tus fuentes los tienen, los descarga y los coloca. Más en
**[[Datos del juego|Datos-del-juego]]**.

## Del catálogo a jugar

1. En **Explorar** abres la ficha del juego y pulsas **Añadir a la biblioteca**.
2. En la **Biblioteca** eliges port (si hay varios) y pulsas **Instalar**. En el mismo diálogo puedes marcar
   *«Descargar también tus datos del juego»* si alguna de tus fuentes los tiene.
3. Cuando termina, pulsas **Jugar**. Dársena cuenta el tiempo de juego.

Todo queda ordenado por juego:

```
DarsenaLibrary\
└── Gears of War 2\
    ├── gears-of-war-2-pc-port\   ← el port
    ├── Datos\                    ← tus archivos del juego
    └── .compartido\              ← lo que compartes por torrent (si está activado)
```

Desinstalar un port borra su carpeta, pero **conserva `Datos`**: tus archivos siguen ahí para otro port del mismo juego.

## Clic derecho

Sobre cualquier juego (biblioteca, tarjetas de Explorar, carrusel) hay un menú con lo habitual: jugar o instalar, ver
ficha, ver en GitHub, abrir carpeta, quitar de la biblioteca o desinstalar.
