# Subir un port

¿Has creado un port? Puedes subirlo a Dársena desde la propia app. Lo revisa el autor de Dársena y, si cumple los
estándares de calidad, entra en el catálogo para todos los jugadores.

## Requisitos

- **Ser autor del port.** Solo puedes subir repositorios de GitHub de los que seas **dueño o colaborador**. Dársena lo
  comprueba con tu cuenta.
- **Algo que instalar:** una release con build de Windows (`.zip` o `.exe`) publicada en GitHub, o la aplicación del port (`.exe` o `.bat`) en la raíz del repositorio. En este caso Dársena instala el código de la rama principal, y si hay un instalador que prepara el juego con los datos del jugador (por ejemplo, un recompilador), JUGAR lo abre la primera vez.
- **Sin archivos del juego original.** Ni ROMs, ni ISOs, ni BIOS, ni assets extraídos: cada jugador usa su propia copia.
- **Que sea un juego.** Por ahora el catálogo solo admite ports de juegos: no herramientas, editores ni emuladores.

## Cómo subirlo

1. En Dársena, **inicia sesión con GitHub** (arriba a la derecha).
2. Abre el menú de tu cuenta → **Subir un port** (también está en el menú de *Explorar*).
3. Elige tu repositorio y pulsa **Comprobar**: Dársena verifica que eres autor, que hay build de Windows y que no está
   ya en el catálogo.
4. Rellena la ficha: nombre del **juego original** (no el del proyecto), consola, qué datos pide y, si quieres, una
   sinopsis y un tráiler de YouTube.
5. Pulsa **Subir para revisión**.

En **Mis ports subidos** ves el estado: *pendiente de revisión*, *aprobado* o *no aprobado* (con el motivo). Si no se
aprueba, puedes corregirlo y volver a subirlo.

## Personalizar la ficha: `darsena.json`

Pon un archivo `darsena.json` en la **raíz de tu repositorio** (rama principal) y Dársena lo mostrará en la ficha de tu
port, en la sección *Del autor del port*. Cada vez que lo cambies, la ficha se actualiza sola, sin pasar por revisión
(Dársena lo vuelve a leer cada media hora).

```json
{
  "description": {
    "es": "Port nativo con **60 FPS**, pantalla panorámica y soporte de mandos.\n\n- Guardado en la nube\n- Texturas en HD opcionales",
    "en": "Native port with **60 FPS**, widescreen and controller support.\n\n- Cloud saves\n- Optional HD textures"
  },
  "screenshots": [
    "https://raw.githubusercontent.com/usuario/mi-port/main/docs/captura1.png",
    "https://raw.githubusercontent.com/usuario/mi-port/main/docs/captura2.png"
  ],
  "trailers": ["https://www.youtube.com/watch?v=XXXXXXXXXXX"],
  "website": "https://usuario.github.io/mi-port",
  "support": ["https://ko-fi.com/usuario", "https://github.com/sponsors/usuario"],
  "requirements": {
    "minimum": { "os": "Windows 10 (64 bits)", "cpu": "Intel i3 o equivalente", "ram": "4 GB", "gpu": "OpenGL 3.3", "storage": "500 MB" },
    "recommended": { "ram": "8 GB", "gpu": "Vulkan 1.2" }
  },
  "needsNote": {
    "es": "Necesita la ISO NTSC-U v1.0.",
    "en": "Needs the NTSC-U v1.0 ISO."
  }
}
```

| Campo | Qué es | Límite |
|---|---|---|
| `description` | Qué hace tu port: mejoras, estado, controles… Admite Markdown sencillo (negritas, listas, enlaces). Puede ser un texto o `{ "es": …, "en": … }`. | 3000 caracteres |
| `screenshots` | Capturas del port, con enlaces `https://`. | 8 |
| `trailers` | Vídeos de YouTube (enlace o id). | 3 |
| `website` | Web del proyecto (`https://`). | — |
| `support` | Enlaces para apoyarte: Ko-fi, Patreon, GitHub Sponsors, PayPal… Salen como botones «Apoyar al autor» en la ficha. | 4 enlaces `https://` |
| `requirements` | Requisitos del sistema, mínimos (`minimum`) y recomendados (`recommended`): `os`, `cpu`, `ram`, `gpu`, `storage` y `other`. Cada uno en texto o `{ "es", "en" }`. Salen en «Requisitos del sistema» de la ficha. | 200 caracteres por campo |
| `needsNote` | Aclaración sobre los datos que pide (región, versión…). | 300 caracteres |

Todo es opcional. Los campos que no estén en la tabla se ignoran. Si la ficha de un autor incumple las normas, el autor de
Dársena puede ocultarla.
