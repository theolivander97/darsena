# Datos del juego

Los ports no traen el juego: necesitan **tu copia** del original. Cada port indica qué formatos acepta
(por ejemplo **ISO · CARPETA** o **BIN/CUE · CHD · ISO**) y, a veces, una nota con la región o el archivo exacto.

## Dónde van

Dársena guarda tus datos en la carpeta **`Datos`** del juego, junto a sus ports:

```
DarsenaLibrary\Dino Crisis\
├── dino-crisis-recomp\   ← el port
└── Datos\                ← tus archivos
```

Así los comparten todos los ports del mismo juego y **no se borran al desinstalar** un port.
En la biblioteca tienes el botón **Abrir carpeta Datos** para ponerlos a mano.

## Instalarlos desde tus fuentes

Si una fuente tiene datos para ese juego, aparecen en la sección **Datos del juego** de la biblioteca y en el diálogo
de instalación del port. Cada opción indica:

- **Compatible** / **No compatible**: si lo que contiene (`contains`) encaja con lo que acepta el port.
- **SHA-1 comprobado**: si la fuente da la huella del archivo.
- **Instalado**: si ya lo bajaste.

## Archivos .zip

Si los datos son un `.zip`, al instalar aparece la casilla **«Extraer el .zip al instalar»**:

- **Marcada**: Dársena extrae el contenido dentro de `Datos` (o de la carpeta `dest` si la fuente la indica).
- **Desmarcada**: guarda el `.zip` tal cual en `Datos`.

Viene marcada si la fuente dice `"format": "zip"` y desmarcada si dice `"format": "file"`; puedes cambiarla cada vez.

## Sacar la huella SHA-1

En PowerShell, con la ruta de tu archivo:

```powershell
Get-FileHash "D:\ruta\mi-archivo.zip" -Algorithm SHA1
```

Copia el valor de `Hash` **en minúsculas** en el campo `sha1` de la fuente. Si el torrent trae varios archivos,
basta con que uno coincida.

## Ejemplos de `contains`

| El port pide | `contains` | `format` |
|---|---|---|
| Una ROM de N64 | `z64` | `file` |
| Una ISO de PS2 | `iso` | `file` |
| `.bin/.cue` de PS1 dentro de un zip | `bin/cue` | `zip` |
| Una carpeta del disco (por ejemplo `PSP_DATA`) dentro de un zip | `carpeta` | `zip` |
