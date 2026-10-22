# Fuentes

Una fuente es un archivo `.json` con enlaces de descarga. Dársena no trae ninguna: las añades tú en la pestaña **Fuentes**,
pegando su dirección (`https://…`) o la ruta de un archivo local (`D:\fuentes\mi-fuente.json`, útil para probar).

> [!IMPORTANT]
> Las fuentes son de terceros. Quien publica una fuente y quien la usa responden de lo que se descarga con ella.
> Pon solo tus propias copias y contenido que tengas derecho a compartir.

## Formato (schemaVersion 2)

```json
{
  "name": "Mi fuente",
  "description": "Mis builds y mis copias",
  "schemaVersion": 2,

  "downloads": [
    {
      "gameId": "autor-nombre-del-proyecto",
      "label": "Port v1.2",
      "uris": ["magnet:?xt=urn:btih:...", "https://mirror.com/port-v1.2.zip"],
      "size": 150000000,
      "uploadDate": "2026-10-17"
    }
  ],

  "data": [
    {
      "game": "super-mario-galaxy",
      "label": "Mi copia NTSC-U",
      "contains": "iso",
      "format": "file",
      "dest": "smg.iso",
      "uris": ["https://archive.org/download/<id>/<id>_archive.torrent"]
    }
  ]
}
```

Las dos listas son opcionales: una fuente puede tener solo `data`.

### `downloads`: ports

| Campo | Qué es |
|---|---|
| `gameId` | El **id del port** en el catálogo (por ejemplo `alexbeavs-ps1-ports-dino-crisis-recomp`). También vale `title`. |
| `label` | Texto visible: versión, origen… |
| `uri` / `uris` | Enlaces al archivo: `magnet:`, `.torrent` o descarga directa. Son mirrors del mismo archivo. |
| `size`, `uploadDate` | Opcionales, informativos. |
| `platform` | Opcional: `windows`, `linux` o `macos`. |

Para los ports que ya están en GitHub **no hace falta** `downloads`: Dársena los baja de las versiones oficiales.

### `data`: tus datos del juego

| Campo | Qué es |
|---|---|
| `game` | El **id del juego** (el que agrupa los ports, por ejemplo `dino-crisis`). También vale `title`. |
| `label` | Texto visible: región, versión, de dónde sale tu copia. |
| `contains` | Qué hay dentro: `iso`, `rvz`, `z64`, `chd`, `bin/cue`, `carpeta`… Dársena lo compara con lo que acepta cada port y solo ofrece como compatibles los que encajan. |
| `format` | `file`: se copia tal cual. `zip`: se extrae. |
| `dest` | Dónde dejarlo dentro de la carpeta `Datos`: un archivo (`oot.z64`) o una carpeta (`PSP_DATA`). Nunca puede salir de `Datos`. |
| `sha1` | Opcional. Huella del archivo: Dársena la comprueba al terminar y rechaza el archivo si no coincide. |
| `ports` | Opcional. Ids de los ports para los que valen estos datos. |
| `uri` / `uris` | Igual que en `downloads`. |

Más detalles en **[[Datos del juego|Datos-del-juego]]**.

## Encontrar los ids

En la ficha de cada juego, el menú del clic derecho → **Ver en GitHub** te lleva al port. Los ids exactos están en
[`catalog.json`](https://github.com/theolivander97/darsena-launcher/blob/main/catalog.json): busca el juego con Ctrl+F.

```json
{"id":"harbourmasters-shipwright", … ,"game":"the-legend-of-zelda-ocarina-of-time"}
```

- `id` → va en `downloads` → `gameId`.
- `game` → va en `data` → `game`.

## Publicar tu fuente

Súbela a cualquier sitio que dé un enlace directo al archivo. Lo más fácil es un **Gist** (gist.github.com): pega el
contenido, guárdalo, pulsa **Raw** y copia esa dirección. Cuando la edites, pulsa **Actualizar todas** en Fuentes.

## Comprobaciones rápidas

- En la tarjeta de la fuente verás cuántos enlaces encajan con el catálogo. Si alguno no aparece, revisa el `gameId` o el `game`.
- Una entrada con `"uris": []` se ignora: sirve de borrador hasta que tengas el enlace.
- Si cambias la fuente y Dársena sigue usando el enlace viejo, pulsa **Actualizar todas**.
