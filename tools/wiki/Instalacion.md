# Instalación

## Requisitos

- **Windows 10 u 11 de 64 bits.**
- **WebView2**, el motor web de Microsoft. Ya viene con Windows 11; en Windows 10 el instalador lo instala si falta
  (necesita conexión a internet).
- Espacio en disco para tus juegos. Cada port pesa poco, pero los datos del juego original pueden ocupar varios GB.

## Instalar

1. Descarga **`Darsena_<versión>_x64-setup.exe`** desde la
   [última versión](https://github.com/theolivander97/darsena-launcher/releases/latest).
2. Ejecútalo. Si Windows muestra *«Windows protegió su PC»*, pulsa **Más información → Ejecutar de todas formas**:
   el instalador no lleva firma de código de Microsoft.
3. Al abrir Dársena aparece una ventana de comprobación: busca versiones nuevas del launcher y actualiza el catálogo.

## Actualizaciones

Dársena se actualiza solo. Al abrirlo, si hay una versión nueva, te pregunta si quieres instalarla: la descarga,
**comprueba su firma** y se reinicia ya actualizado. Puedes posponerlo.

El **catálogo**, las sinopsis y los tráilers se actualizan aparte, sin instalar nada: los juegos nuevos aparecen solos
al abrir el launcher.

> [!TIP]
> Si cada vez que abres Dársena te pide actualizar a la misma versión, probablemente tienes **dos instalaciones**
> y tu acceso directo abre la vieja. Desinstala desde *Configuración de Windows → Aplicaciones* la que no se actualiza
> y vuelve a instalar la última desde Releases.

## Dónde guarda Dársena tus cosas

| Qué | Dónde |
|---|---|
| Juegos instalados (biblioteca) | `<unidad>:\DarsenaLibrary` por defecto. Se cambia en **Ajustes**. |
| Ajustes, fuentes, biblioteca, perfil y caché de portadas | `%APPDATA%\com.darsena.app` |
| Sesión de GitHub | Administrador de credenciales de Windows |

Desinstalar Dársena **no borra** tus juegos instalados ni tus datos del juego.
