# Dársena

**Launcher de escritorio para juegos porteados** (decomps, recompilaciones y ports nativos de PC), en español.
Descarga, actualiza y juega, y gestiona tus datos de juego. Gratis, de código abierto y sin anuncios.

**English:** Dársena is a free, open-source desktop launcher for community PC ports (decompilations, static recompilations and native ports). It downloads and updates ports, keeps your game data organized, tracks playtime and runs on Windows. It ships no games: you bring your own copy.

[Descargar / Download](https://github.com/theolivander97/darsena-launcher/releases/latest) ·
[Wiki](https://github.com/theolivander97/darsena-launcher/wiki) ·
[Catálogo y reseñas](https://github.com/theolivander97/darsena-launcher)

> Dársena **no distribuye juegos**. Solo lista ports hechos por la comunidad y descarga las builds que sus autores publican en GitHub.
> Los datos del juego original (ROM, ISO, archivos) los pones tú, de tu propia copia.

## Qué hace

- **Explorar:** catálogo de ports con ficha, sinopsis, tráilers, capturas, requisitos del sistema y reseñas.
- **Biblioteca:** estanterías, tiempo de juego, accesos directos, actualización de ports (con comprobación SHA-256) y juegos propios.
- **Descargas inteligentes:** HTTP con varias conexiones y torrent a la vez (librqbit), con alternativa automática.
- **Taller y Comunidad:** mods de la comunidad, reseñas, informes de «¿funciona en tu PC?».
- **Partidas en la nube** en un Gist privado de tu cuenta de GitHub, y **modo mando** a pantalla completa.
- **Para autores:** sube tu port, edita su ficha con un `darsena.json` y responde a las reseñas.

Hecho con **Tauri 2 + Rust**. La interfaz es HTML y JavaScript sin framework (`ui/`).

## Compilar

Requisitos: Rust estable, Node.js 20+ y, en Windows, las herramientas de C++ (MSVC) o un toolchain GNU.

```sh
npm ci
node tools/fetch-data.mjs   # baja el catálogo y el índice de portadas que se incrustan en el binario
npm run tauri build         # instalador NSIS en src-tauri/target/release/bundle/nsis
```

Para desarrollar: `npm run dev`. El catálogo, las portadas y los logos viven en otros repositorios
([datos](https://github.com/theolivander97/darsena-launcher), `portlauncher-covers` y `darsena-logos`); aquí solo está el código.

## Contribuir

Los ports se añaden al catálogo desde la propia aplicación (**Subir un port**) o con una solicitud de juego; ver la
[wiki](https://github.com/theolivander97/darsena-launcher/wiki). Para cambios en el código, abre un issue o una pull request.
Las traducciones están en `ui/i18n.js` (la clave es el texto en español).

## Licencia

[GPL-3.0-or-later](LICENSE). El nombre y el logo de Dársena son del autor del proyecto.

---

## Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

- **Qué se firma / What is signed:** únicamente el instalador de Windows de Dársena (`Darsena_<versión>_x64-setup.exe`), compilado por GitHub Actions a partir del código de este repositorio (`.github/workflows/build.yml`). No se firma ningún programa de terceros.
- **Roles de equipo / Team roles:**
  - Autor, revisor y aprobador / Author, reviewer and approver: [@theolivander97](https://github.com/theolivander97). Cada solicitud de firma la aprueba manualmente.
- Los miembros del equipo usan autenticación en dos pasos en GitHub y SignPath.

## Privacy policy / Política de privacidad

Dársena **no recoge telemetría ni datos de uso** y no tiene servidores propios. Estas son todas las conexiones que hace:

| A quién | Para qué | Datos enviados |
|---|---|---|
| GitHub (API, releases y Discussions) | Catálogo, descargas oficiales de ports, actualizaciones de Dársena, reseñas, Taller | Tu token de GitHub, solo si inicias sesión (se guarda en el almacén de credenciales de Windows) |
| jsDelivr (CDN) | Portadas y logos de los juegos | Ninguno |
| Wikipedia / MyMemory | Sinopsis y su traducción | El nombre del juego |
| Fuentes que añadas tú | Enlaces de descarga | Lo que pida esa fuente |
| Torrents (si descargas por torrent) | Descarga y compartición | Tu IP, como en cualquier torrent; se puede desactivar compartir en Ajustes |
| Gists de tu cuenta | Perfil y partidas en la nube (opcional) | Tu perfil de juego y tus partidas |

Todo lo que se guarda en tu PC está en `%APPDATA%\com.darsena.app`. Desinstalar Dársena no toca tus juegos.
