# Guía del catálogo (mantenedor)

> [!NOTE]
> Esta página es para quien mantiene el catálogo. Si quieres proponer un juego, usa
> [[Pedir un juego|Pedir-un-juego]]: el catálogo no acepta cambios directos.

El catálogo es `catalog/catalog.json`: una lista de **ports**, uno por línea. Cada port dice **de dónde sale** con `origin`:

- `"origin": "github"` (lo normal): el port tiene repositorio en GitHub. El launcher descarga de ahí la build de Windows
  más reciente, así que no hace falta poner enlaces de descarga.
- `"origin": "external"`: el port **no está en GitHub** (ModDB, Discord, una web…). El catálogo solo lo da a conocer;
  para instalarlo, el usuario necesita una **fuente** que traiga el port. Ver [Ports externos](#ports-externos).

## Flujo de trabajo

Desde la carpeta del proyecto:

1. **Edita** `catalog/catalog.json`: copia la línea de un juego parecido y cambia sus datos, o usa la plantilla de abajo.
   Cada línea termina en coma `,`, **excepto la última** de la lista.
2. **Valida y ordena:**
   ```powershell
   node tools\catalog\actualizar.mjs
   ```
   Comprueba que todo está bien, rellena lo que falta, agrupa los ports por juego y ordena el archivo.
   Si hay un error dice cuál y en qué línea, y **no guarda nada** hasta que lo corrijas.
3. **Portada:**
   ```powershell
   node tools\covers\build-covers.mjs --only <id-del-juego>
   node tools\covers\publish-covers.mjs "Portada de <juego>"
   ```
4. **Sinopsis** (Wikipedia, más lo puesto a mano):
   ```powershell
   node tools\catalog\informacion.mjs
   ```
5. **Publica:**
   ```powershell
   node tools\catalog\publicar.mjs "Añade <juego>"
   ```
   Sube el catálogo y la información al repositorio. Los launchers lo reciben al abrirse, sin instalar una versión nueva
   (GitHub tarda hasta 5 minutos en servir el cambio).

También se puede editar `catalog.json` directamente en GitHub: la próxima vez que ejecutes `publicar.mjs`, trae esos
cambios a tu copia local y los valida. Si cambió en los dos sitios, se para y te avisa.

## Plantilla

Lo mínimo son tres campos. Lo demás se rellena solo al ejecutar `actualizar.mjs`:

```json
{"title":"Nombre del juego","repo":"usuario/repositorio","console":"Nintendo 64"},
```

Port externo (no está en GitHub):

```json
{"title":"Nombre del juego","port":"Nombre del proyecto","author":"Autor","origin":"external","homepage":"https://www.moddb.com/mods/...","console":"Xbox 360"},
```

Completa (lo recomendable):

```json
{"title":"Star Fox 64","repo":"HarbourMasters/Starship","console":"Nintendo 64","franchise":"Star Fox","aliases":["Starfox 64","SF64"],"asset":"^Starship-.*-Windows\\.zip$","exe":"Starship.exe","experimental":false,"platforms":["Windows","Linux","macOS"]},
```

## Campos

| Campo | ¿Obligatorio? | Qué poner |
|---|---|---|
| `title` | Sí | Nombre **del juego**, no del proyecto: `"Crash Bandicoot 2: Cortex Strikes Back"`, no `"Crash2Recomp"`. Es lo que sale en la tienda. |
| `origin` | No | `"github"` (por defecto) o `"external"` si el port no está en GitHub. |
| `repo` | Sí, si es de GitHub | Repositorio de GitHub como `usuario/repositorio`. Los externos no lo llevan. |
| `port` | Sí, si es externo | Nombre del proyecto del port. En los de GitHub se saca del repo. |
| `author` | No | Autor del port. En los de GitHub se saca del repo; en los externos, si falta, sale «autor desconocido». |
| `homepage` | No | Página oficial del proyecto. En los externos es el enlace que ve el usuario en lugar de «Ver en GitHub». |
| `console` | Sí | Consola original. Una de: `Nintendo 64`, `Super Nintendo`, `NES`, `Game Boy`, `Game Boy Advance`, `Nintendo DS`, `Nintendo 3DS`, `GameCube`, `Wii`, `Wii U`, `Switch`, `Virtual Boy`, `PlayStation`, `PlayStation 2`, `PlayStation 3`, `PlayStation 4`, `PSP`, `Xbox`, `Xbox 360`, `Sega`, `Arcade`, `PC`, `Otros`. |
| `franchise` | No | Saga, por ejemplo `"The Legend of Zelda"`. |
| `aliases` | No | Otros nombres para la búsqueda y para emparejar enlaces de fuentes: `["CTR","Crash Team Racing"]`. |
| `asset` | No | Qué archivo de la release es la build de Windows (ver abajo). Si falta, el launcher elige el que más pinta tenga. |
| `exe` | No | Nombre del ejecutable, por ejemplo `"CoD3.exe"`. Si falta, se detecta al instalar. |
| `experimental` | No | `true` si el port es una beta o pre-release. |
| `needs` | No | Qué datos acepta el port: `["iso","carpeta"]`, `["z64"]`… Si falta, se usan los de su consola. Ponlo solo si el port difiere. |
| `needsNote` | No | Aclaración que verá el usuario: región o versión exigida, carpeta concreta… |
| `platforms` | No | Dónde funciona el port. Por defecto, `["Windows"]`. |
| `id`, `game` | No | **No los toques**: se calculan solos a partir de `repo` (o `author` + `port`) y `title`. |

### El campo `asset`

Muchas releases traen varios archivos (Windows, Linux, Android, texturas…). `asset` es una expresión regular que dice
cuál es el de Windows. Mira el nombre del archivo en *Releases* y cambia la versión por `.*`:

| Archivo en la release | `asset` |
|---|---|
| `CoD3-PC-Port-0.4.0.zip` | `"^CoD3-PC-Port-.*\\.zip$"` |
| `TwoTowers-Recompiled-v1.9.2-win64.zip` | `"^TwoTowers-Recompiled-.*-win64\\.zip$"` |
| `VCSNative-Windows.zip` | `"^VCSNative-Windows\\.zip$"` |

El punto se escribe `\\.` (con dos barras, porque estamos dentro de JSON); `^` significa «empieza por» y `$` «termina en».

## Sinopsis, tráilers y destacados

Cada ficha muestra de qué trata el juego y su tráiler. Eso sale de `catalog/info.json`, que **se genera** con
`informacion.mjs`: busca la sinopsis en Wikipedia (en español si existe) y solo acepta coincidencias seguras. Los juegos
que no encuentra quedan en `catalog/info-pendientes.txt`.

Lo que quieras poner o corregir a mano va en **`catalog/info-overrides.json`**, que siempre gana:

```json
{
  "featured": ["gears-of-war-2", "bloodborne"],
  "games": {
    "id-del-juego": {
      "synopsis": { "text": "De qué trata…", "source": { "name": "Redacción de Dársena" } },
      "trailers": [ { "kind": "youtube", "id": "EQx0flOIRXY", "title": "Tráiler de lanzamiento" } ]
    }
  }
}
```

- **`featured`**: los juegos que salen siempre primero en el carrusel de *Destacados y novedades*, en ese orden.
- **`id-del-juego`** es el campo `game` de `catalog.json`, no el `id` de un port.
- **`trailers`**: `id` es lo que va después de `watch?v=` en YouTube. Comprueba que se pueda incrustar:
  `https://www.youtube.com/oembed?url=https://www.youtube.com/watch?v=<id>&format=json` debe responder con su título.

## Ports externos

1. Añádelo con `"origin":"external"`, `"port"` y, si la hay, `"homepage"`.
2. Ejecuta `actualizar.mjs` y mira el `id` que le ha puesto.
3. Las descargas van en una **fuente**, en `downloads`, usando ese `id` como `gameId` (ver [[Fuentes|Fuentes]]).

Sin una fuente que lo traiga, el launcher muestra el juego pero el botón dice **NECESITA UNA FUENTE**.

## Varios ports del mismo juego

Si dos ports tienen el mismo `title` y la misma `console`, se juntan en una sola ficha con selector de ports.
Si se juntan por error, o al revés, corrígelo en `catalog/games-overrides.json`:

```json
{ "voicedrew11-verdite1": "kings-field-japan" }
```

La clave es el `id` del port y el valor, el id del juego al que debe pertenecer.

## Qué ports se pueden añadir

Solo ports que **piden tu propia copia** del juego y cuya release **no incluye los datos del juego**:

- El README dice algo como *"bring your own copy"*, *"no game data included"* o *"you need your own disc"*.
- El tamaño de la release encaja con un programa (decenas o cientos de MB), no con un juego completo (varios GB).
- Desconfía de los que dicen *"no ROM needed"* en juegos de consola: suelen llevar el juego dentro.

## Errores comunes

| Mensaje | Solución |
|---|---|
| `no es JSON válido … Línea N` | Revisa esa línea y el final de la anterior: falta o sobra una coma, o hay comillas sin cerrar. |
| `falta "title"` / `"repo"` / `"console"` | Añade ese campo. Si el port no está en GitHub, pon `"origin":"external"` y `"port"` en lugar de `repo`. |
| `consola "…" no válida` | Usa exactamente uno de los nombres de la tabla. |
| `"asset" no es una regex válida` | Revisa los `\\.` y que los paréntesis estén cerrados. |
| `id "…" repetido` | Ese repo ya está en el catálogo. Edita esa línea en lugar de añadir otra. |
