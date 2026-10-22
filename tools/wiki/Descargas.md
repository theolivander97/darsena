# Descargas

La pestaña **Descargas** muestra lo que se está bajando, con velocidad, tiempo restante y de dónde viene
(**HTTP · 8 conexiones · 2 mirrors**, **TORRENT**…). Puedes pausar, reanudar y cancelar.

## De dónde se descarga

| Origen | Cómo lo baja Dársena |
|---|---|
| **GitHub oficial** | El archivo de Windows de la última versión del port, directamente del repositorio de su autor. |
| **Enlace directo** (`https://…`) | Por trozos y con varias conexiones a la vez. Si la fuente da varios mirrors del mismo archivo, baja de todos en paralelo. |
| **Torrent** (`.torrent`) o **magnet** | Con el motor de torrents de Dársena, con trackers públicos añadidos para encontrar pares más rápido. |
| **archive.org** | Su `.torrent` no tiene pares: Dársena lee los servidores de archive.org que vienen dentro (*web seeds*) y baja de ahí con varias conexiones, comprobando cada pieza con los hashes del torrent. |

## Torrent y enlace directo en la misma fuente

Si una entrada tiene un torrent **y** enlaces directos, Dársena prueba primero el torrent durante 8 segundos:

- Si baja a buen ritmo (≈1 MB/s o más), sigue solo con el torrent.
- Si no tiene pares o va muy lento, lo corta, **borra lo que llevaba** y sigue solo con la descarga directa.

Nunca se bajan las dos a la vez: cada una reservaría el archivo entero en disco.

Con varios torrents en la misma entrada, si uno no consigue pares en un minuto, se pasa al siguiente.

## Compartir lo que descargas

Por defecto, Dársena **sigue compartiendo** (sembrando) en segundo plano los ports que bajas por torrent, mientras está
abierto. Cuanta más gente comparte, más rápido baja cada port para los demás.

- Se configura en **Ajustes → Compartir descargas**, con un **límite de subida** (1 MB/s por defecto).
- Para poder compartir, se guarda una copia del archivo descargado en la carpeta `.compartido` del juego; se borra al
  desinstalar o al desactivar la opción.
- **Tus datos del juego nunca se comparten.**

## Si algo no descarga

- **«Conectando…» con un magnet**: nadie lo está compartiendo, o quien lo comparte no es accesible. Pon siempre un enlace
  directo detrás del magnet en tu fuente.
- **403 Forbidden** en archive.org: archive.org no permite descargar ese elemento. Dársena no puede saltárselo.
- **«El SHA-1 no coincide»**: el archivo descargado no es el que dice la fuente (otra versión o dañado).
