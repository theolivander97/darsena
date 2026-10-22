# Juego remoto en Dársena — documento de diseño

> Estado: **propuesta, pendiente de aprobación**. No hay código todavía.
> Objetivo: que el anfitrión ejecute un port instalado en Dársena y que sus amigos
> se unan por internet viendo la imagen y mandando mando/teclado **sin tener el juego**,
> al estilo de Steam Remote Play Together / RemotePlayWhatever, pero sin depender de Steam.

---

## 1. Cómo funciona RemotePlayWhatever y por qué no sirve tal cual

RemotePlayWhatever (RPW, MIT, `github.com/m4dEngi/RemotePlayWhatever`) **no hace streaming**.
Es solo un botón de "invitar" que le pide al cliente de Steam que abra una sesión de Remote Play Together.

Lo que hace su código (`SteamStuff.cpp`, `RemotePlayInviteHandler.cpp`):

1. Carga `steamclient.dll` y pide la interfaz `ISteamClient019` con `SteamInternal_CreateInterface`.
2. Se conecta al usuario que ya tiene sesión en Steam (`CreateSteamPipe` + `ConnectToGlobalUser`).
3. Pide `IClientEngine`, una interfaz **interna y no documentada** del cliente de Steam,
   y de ella saca `IClientRemoteClientManager`.
4. Para invitar llama a `BCreateRemotePlayInviteAndSession(invitado, appId)`. Con juegos que no son de Steam
   activa `SetStreamingDesktopToRemotePlayTogetherEnabled(true)` y usa un AppID prestado (480, Spacewar)
   para que Steam emita **todo el escritorio**.
5. El invitado recibe un enlace por el chat de Steam o un "guest link" y se une con Steam o Steam Link.

Todo lo difícil corre dentro de Steam: captura, codificación, relés de red de Valve, entrada remota y mandos virtuales.

**Por qué no lo podemos usar en Dársena:**

| Problema | Consecuencia |
|---|---|
| Hace falta Steam abierto y con sesión iniciada en el PC del anfitrión | Dársena dejaría de ser independiente |
| Usa interfaces internas (`IClientEngine`) que Valve cambia sin avisar | Ya pasó: el propio README advierte que **no funciona bien con los clientes de Steam recientes** |
| Con juegos que no son de Steam se emite el escritorio entero con un AppID ajeno | Mala privacidad (se ve todo) y se apoya en un truco |
| El invitado necesita Steam o Steam Link | No cumple "abrir un enlace y jugar" |
| Los relés y el protocolo son de Valve | No se pueden reutilizar fuera de Steam |

Como mucho, Dársena podría ofrecer un **atajo opcional** ("abrir con Steam Remote Play") cuando detecte Steam,
pero no conviene usarlo como base. Hay que construir el streaming nosotros.

---

## 2. Arquitectura propuesta

```
 PC ANFITRIÓN (Dársena, Rust)                                    INVITADO
┌──────────────────────────────────────────────┐          ┌──────────────────────────┐
│ library::launch ─► proceso del port (PID)    │          │ Navegador (Chrome/Edge/  │
│        │                                     │          │ Firefox/Safari) o la     │
│        ▼  HWND de la ventana del juego       │          │ WebView2 de otro Dársena │
│ ┌─────────────┐   texturas D3D11 (GPU)       │          │                          │
│ │ Captura WGC │──────────────┐               │  WebRTC  │ <video> decodifica H.264 │
│ └─────────────┘              ▼               │  (SRTP)  │ por hardware             │
│ ┌──────────────────┐   ┌──────────────┐      │◄────────►│                          │
│ │ WASAPI loopback  │   │ Codificador  │──┐   │          │ Gamepad API, teclado,    │
│ │ (solo el proceso)│   │ H.264 por HW │  │   │  Data-   │ Pointer Lock ──► Data-   │
│ └───────┬──────────┘   └──────────────┘  │   │  Channel │ Channel                  │
│         ▼ Opus                           ▼   │          └──────────────────────────┘
│ ┌────────────────────────────────────────────┐          
│ │ remote::session  (webrtc-rs / str0m)       │──── STUN (gratis) / TURN (si hay CGNAT)
│ │ 1 codificación ─► N conexiones de invitado │          
│ └───────────────┬────────────────────────────┘          
│                 ▼ eventos de entrada + permisos         
│ ┌─────────────────────────────┐                        ┌──────────────────────────┐
│ │ Inyección: mandos virtuales │                        │ Señalización: Cloudflare │
│ │ (ViGEmBus) + SendInput      │      ofertas/ICE ────► │ Worker (+ página web del │
│ └─────────────────────────────┘                        │ invitado)                │
└──────────────────────────────────────────────┘          └──────────────────────────┘
```

### 2.1 Captura de vídeo

- **Windows Graphics Capture (WGC)** sobre el `HWND` del juego. Funciona con D3D9/11/12, OpenGL y Vulkan
  si la ventana pasa por el compositor (ventana normal o "borderless"). Entrega texturas D3D11
  que se pasan al codificador **sin bajar a la CPU**. Windows 10 1903+. Desde Windows 11 se puede quitar el borde amarillo.
- **DXGI Desktop Duplication** como alternativa para pantalla completa exclusiva (captura el monitor, no la ventana).
  Menos privado; mejor recomendar modo ventana sin bordes.
- Se busca el `HWND` a partir del PID que ya conoce `library::launch` (con `EnumWindows`), esperando a que aparezca la ventana.
- Rust: crate `windows` (bindings oficiales). Existe `windows-capture`, pero el código propio es corto y nos da control total.

### 2.2 Captura de audio

- **WASAPI loopback por proceso** (`AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`, Windows 10 2004+):
  captura **solo el audio del juego** (y de sus procesos hijos), no Discord ni las notificaciones del anfitrión.
- Codificación **Opus** a 48 kHz, en estéreo y con tramas de 10 ms (crate `opus` o `audiopus`, que compilan libopus con `cc`).

### 2.3 Codificación de vídeo

Códec: **H.264** (el que todos los navegadores decodifican por hardware). HEVC o AV1 quedan para después;
en el navegador su soporte es irregular.

Ajustes de baja latencia: sin B-frames, CBR con un búfer de aproximadamente un fotograma, GOP infinito
con fotograma clave solo cuando el invitado lo pide (PLI/FIR) o refresco intra, y perfil Constrained Baseline/High.

| Opción | Ventajas | Inconvenientes |
|---|---|---|
| **Media Foundation** (MFT de H.264) | Viene con Windows, sin DLLs extra. Usa NVENC/AMF/QSV a través del MFT del fabricante. **La licencia de patentes H.264 la cubre Windows** | API COM verbosa. Los MFT de hardware son asíncronos y cada fabricante tiene sus rarezas |
| FFmpeg (`ffmpeg-next`, `h264_nvenc`/`h264_amf`/`h264_qsv`) | Mucho control, bien documentado y probado (Sunshine lo usa) | Unos 60–100 MB de DLLs, licencia LGPL/GPL y más complicado de compilar con la cadena GNU |
| SDK directos (NVENC, AMF, oneVPL) | Latencia mínima y control total | Tres implementaciones que mantener |
| `openh264` (software) | Fácil y portable | Usa CPU (le quita rendimiento al juego). Si lo compilamos nosotros, **no tiene la cobertura de patentes de Cisco** |

**Recomendación:** Media Foundation desde el principio (primero el codificador software de Microsoft, que es síncrono
y siempre está disponible, y luego el MFT de hardware). FFmpeg queda como plan B si MF da problemas con alguna GPU.

### 2.4 Transporte: WebRTC

WebRTC es lo único que un navegador entiende para vídeo en tiempo real con NAT traversal, cifrado obligatorio
(DTLS-SRTP) y canales de datos. Hay dos opciones en Rust:

- **`webrtc` (webrtc-rs) 0.20.x**: la nueva línea Sans-I/O sobre el crate `rtc`, estable desde 0.20.0, con backend Tokio
  (Dársena ya usa Tokio). La 0.17.x antigua solo recibe correcciones.
- **`str0m`**: también Sans-I/O, pequeño y muy usado en servidores SFU. Es más simple, pero su API es de más bajo nivel.

Cómo se usa:
- Pista de vídeo H.264 y pista de audio Opus. **Una sola codificación** que se reenvía a las N conexiones
  (los invitados ven lo mismo, así que no hace falta codificar una vez por invitado).
- DataChannel `input` **no ordenado y sin reintentos** para ejes analógicos y ratón (vale el último valor),
  y otro **fiable** para botones, teclas y control (permisos, kick).
- Control de bitrate con TWCC/REMB más PLI. Si la librería no lo resuelve sola, se ajusta el bitrate del codificador a mano.

**NAT:**
- STUN público y gratuito (Google o Cloudflare) resuelve la mayoría de conexiones.
- **TURN** es necesario cuando hay NAT simétrico o **CGNAT** (habitual en redes móviles y en muchos ISP de LATAM).
  Opciones: Cloudflare Realtime TURN (tiene capa gratuita; hay que comprobar las cifras actuales),
  un `coturn` propio en un VPS barato, o arrancar sin TURN y avisar de que "tu red no permite conexión directa".
  El TURN reenvía todo el vídeo, así que cuesta ancho de banda: 720p60 a 8 Mbps son unos 3,6 GB por hora y por invitado.

### 2.5 Señalización (cómo se encuentran los dos lados)

WebRTC necesita intercambiar una oferta y una respuesta SDP más candidatos ICE antes de conectar.

| Opción | Coste | Experiencia | Comentario |
|---|---|---|---|
| A. Intercambio manual de códigos | 0 | Mala: dos códigos largos que copiar y pegar en cada sentido | Útil solo como modo "sin internet" o de emergencia |
| **B. Cloudflare Worker + Durable Object** | Gratis en la capa free | Buena: un enlace `…/j/ABCD-1234` y listo | El mismo Worker sirve la página del invitado. WebSocket con hibernación y nada de estado permanente |
| C. Apoyado en GitHub (Gists o comentarios como buzón) | 0 | Mala: polling lento, límites de peticiones y **el invitado necesita cuenta de GitHub** | Descartado para invitados desde el navegador. Sirve para "invitar a un amigo de Dársena" más adelante |
| D. Servidor propio (VPS) | 4–6 $/mes | Buena | Más mantenimiento. Solo tiene sentido si además alojamos coturn |

**Recomendación: B.** El Worker solo hace de buzón efímero: crea una sala con un código aleatorio,
pasa SDP e ICE entre anfitrión e invitados y caduca en unos minutos. Nunca ve el vídeo.
La identidad de GitHub que ya tiene Dársena puede servir para firmar la creación de salas
y para mostrar el nombre del anfitrión, pero los invitados no la necesitan.

### 2.6 El invitado

- **Navegador, sin instalar nada:** una página estática de HTML/JS vanilla (en la misma línea que `ui/`) servida por el Worker.
  Usa `RTCPeerConnection`, `<video>` con `jitterBufferTarget`/`playoutDelayHint` a 0, la **Gamepad API**
  (mandos USB/Bluetooth del invitado), teclado y ratón con **Pointer Lock**, y pantalla completa.
  En el móvil, controles táctiles más adelante.
- **Desde otro Dársena:** la WebView2 de Tauri es Chromium, así que se carga la **misma página** dentro del launcher.
  Esto añade un botón "Unirse" y, más adelante, invitar a amigos.

### 2.7 Inyección de entrada y permisos

- **Mandos virtuales:** **ViGEmBus** crea mandos Xbox 360/DS4 que los juegos ven como reales (XInput/SDL).
  Rust: crate `vigem-client`. Ojo: **ViGEmBus está archivado desde noviembre de 2023** por un conflicto de marca.
  Sigue funcionando y lo usan DS4Windows, Sunshine y muchos otros, pero ya no evoluciona. El sucesor anunciado
  (VirtualPad) aún no tiene estado confirmado. El driver pide **instalarlo con permisos de administrador una vez**;
  Dársena lo instalaría a petición la primera vez que alguien active el juego remoto.
  Alternativa sin driver a investigar: `Windows.UI.Input.Preview.Injection` (`InputInjector` de gamepad),
  que solo lo ven los juegos que usan Windows.Gaming.Input, no XInput.
- **Teclado y ratón:** `SendInput` con scancodes (`KEYEVENTF_SCANCODE`, que los juegos leen mejor que las teclas virtuales)
  y ratón relativo. Es **global**: solo se inyecta mientras la ventana del juego está en primer plano.
- **Permisos por invitado** (los decide el anfitrión en el panel de la sesión):

  | Permiso | Por defecto |
  |---|---|
  | Ver y oír | sí |
  | Mando (asigna el slot P2, P3…) | sí |
  | Teclado | no |
  | Ratón | no |

  Además: **lista negra fija** de teclas peligrosas (Win, Alt+Tab, Alt+F4, Ctrl+Alt+Supr, Ctrl+Shift+Esc),
  expulsar y bloquear invitados, y "pausar entrada de todos". Inyectar teclado equivale a tocar el PC del anfitrión,
  por eso viene desactivado.

### 2.8 Latencia objetivo y límites

Presupuesto "de cristal a cristal" (desde que el juego pinta hasta que el invitado lo ve):

| Etapa | LAN | Internet (misma región) |
|---|---|---|
| Captura WGC | 1–8 ms | 1–8 ms |
| Codificación por hardware | 3–6 ms | 3–6 ms |
| Red (ida) | <2 ms | 15–40 ms |
| Jitter buffer | 0–10 ms | 10–30 ms |
| Decodificación + presentación | 10–20 ms | 10–20 ms |
| **Total** | **≈ 25–45 ms** | **≈ 50–100 ms** |

Para el input del invitado hay que sumar la ida de vuelta (otra latencia de red más un fotograma del juego).
Es jugable en plataformas, lucha casual y beat 'em up. Para lucha competitiva o ritmo, no.

Límites conocidos:
- **La subida del anfitrión manda:** 720p60 ≈ 6–10 Mbps por invitado. Con 3 invitados hacen falta unos 30 Mbps de subida.
  Valores por defecto prudentes: 720p60 a 8 Mbps, con 1080p como opción.
- La pantalla completa exclusiva no se captura por ventana; hay que recomendar modo ventana sin bordes.
- Si el anfitrión cambia de ventana, el teclado inyectado se pausa (por seguridad).
- Para varios jugadores el port tiene que soportar multijugador local (varios mandos). Si no, los invitados solo miran
  o se pasan el mando.
- Solo Windows en la primera versión (todo lo de arriba son APIs de Windows).

### 2.9 Encaje en Dársena

- Módulo nuevo `src-tauri/src/remote/` (`capture.rs`, `encode.rs`, `audio.rs`, `session.rs`, `input.rs`, `signaling.rs`).
- Comandos nuevos: `remote_start(game_id)`, `remote_stop`, `remote_invite_link`, `remote_set_permissions(guest, perms)`
  y eventos `remote://guest-joined` y `remote://stats`.
- Reutiliza `library::launch` (y su PID) para lanzar el juego. **No toca el catálogo, las fuentes ni las descargas.**
- UI: botón "Jugar en remoto" en la ficha del juego instalado y un panel lateral con invitados, permisos y estadísticas
  (ping, fps, bitrate).

---

## 3. Propio frente a integrar algo existente

| | Sunshine + Moonlight | Parsec | Steam (vía RPW) | **Propio (este diseño)** |
|---|---|---|---|---|
| Licencia | GPL-3.0 | Cerrado; SDK restringido | Interfaces internas de Valve | Nuestra |
| ¿El invitado juega desde el navegador? | **No** (necesita la app Moonlight; hay clientes web experimentales) | No (app de Parsec) | No (Steam o Steam Link) | **Sí** |
| ¿Varios invitados a la vez? | Pensado para 1 usuario en su propio PC | Sí | Sí | Sí |
| NAT traversal | No (hay que abrir puertos o usar Tailscale/ZeroTier) | Sí | Sí | Sí (STUN/TURN) |
| Cuentas | Emparejamiento con PIN | Cuenta de Parsec obligatoria | Cuenta de Steam | Ninguna para el invitado |
| Latencia | Excelente | Excelente | Buena | Buena (algo peor que estas en la v1) |
| Esfuerzo | Medio (integrar un sidecar GPL y su configuración) | Bajo, pero solo podemos "abrir Parsec" | Bajo, pero se rompe | **Alto** (semanas) |
| Control de la experiencia | Bajo | Nulo | Nulo | Total |

Referencias útiles para inspirarse (no para integrar): **Selkies** y **Neko** (streaming WebRTC al navegador, en Linux)
y el propio código de Sunshine para los ajustes de NVENC/AMF/QSV y ViGEm.

**Conclusión:** ninguna opción existente cumple "un enlace y a jugar en el navegador" sin cuentas ni instalaciones.
Hacerlo propio es lo único que da esa experiencia, a cambio de más trabajo. Mientras tanto, se podría añadir un enlace
"Usar Parsec" como solución provisional.

---

## 4. Plan por fases

| Fase | Contenido | Resultado |
|---|---|---|
| **F0 – Prototipo LAN** | WGC de una ventana → H.264 (MF) → WebRTC → página servida por un HTTP local de Dársena que también hace de señalización. Sin audio ni entrada | Ver la ventana en el navegador de otro equipo de la red y medir la latencia |
| F1 – Jugable en LAN | Audio WASAPI + Opus, entrada (ViGEm + SendInput), permisos y panel en la UI | Jugar de verdad en casa |
| F2 – Internet | Cloudflare Worker (señalización + página del invitado), STUN y TURN, enlaces caducables | Jugar con amigos por internet |
| F3 – Pulido | MFT de hardware por fabricante, control de bitrate, reconexión, Dársena↔Dársena, invitar amigos (GitHub) | Versión pública |

---

## 5. Decisiones que tienes que tomar

1. **¿Construirlo propio (recomendado) o integrar Sunshine/Parsec?**
2. **Señalización:** ¿Cloudflare Worker (recomendado; necesita una cuenta gratuita de Cloudflare tuya),
   códigos manuales o un VPS propio?
3. **TURN:** ¿Cloudflare TURN, `coturn` en un VPS o empezar sin TURN?
4. **Codificador:** ¿Media Foundation (recomendado) o FFmpeg (más control, unos 80 MB de DLLs y LGPL/GPL)?
5. **Crate WebRTC:** ¿`webrtc` 0.20 (recomendado; encaja con Tokio) o `str0m`?
6. **Mandos virtuales:** ¿aceptas que Dársena instale ViGEmBus (driver archivado pero funcional, pide administrador una vez)?
7. **Permisos por defecto:** ¿te valen "ver + mando sí, teclado/ratón no" y la lista negra de teclas?
8. **¿Apruebas el prototipo F0** en `src-tauri/src/remote/`? Añade dependencias (`webrtc`, `windows` con las features
   de captura y Media Foundation, un servidor HTTP/WebSocket pequeño) que aumentarán el tiempo de compilación
   y el tamaño del ejecutable. Riesgo a vigilar: que todo compile bien con la cadena GNU (`ring` y las features de `windows` suelen ir bien).

---

## 6. Estado del prototipo F0 (`src-tauri/src/remote/`)

Implementado y probado en LAN: captura WGC de una ventana → H.264 Baseline (MFT software de Microsoft, NV12 convertido en CPU)
→ WebRTC (`webrtc` 0.21, un `TrackLocalStaticSample` por invitado alimentado desde un `broadcast`) → página `guest.html`
servida por un servidor `axum` local (puerto 47800+) que también hace de señalización (`POST /offer`, ICE sin trickle).

Prueba hecha: ventana de 1280×672 vista en el navegador por `http://localhost:47800/`, 1,3 Mb/s con contenido casi estático,
RTT 1 ms, jitter buffer 20 ms. **Sin medir aún** la latencia de cristal a cristal con contenido en movimiento.

Cómo usarlo:
- Desde la UI (aún sin botón): `invoke('remote_list_windows')`, `invoke('remote_start', { hwnd, fps, bitrateKbps })`,
  `invoke('remote_stop')`, `invoke('remote_status')`.
- Prueba sin UI: arrancar Dársena con la variable `DARSENA_REMOTE_TEST="parte del título"`; el resultado queda en
  `%TEMP%\darsena_remote_test.txt`.
- Windows puede pedir permiso de firewall la primera vez (el invitado entra por la IP de la LAN del anfitrión).

Limitaciones conocidas del prototipo (todo ya previsto en las fases F1–F3):
- Sin audio, sin entrada remota (mando/teclado) y sin permisos por invitado.
- Codificación y conversión de color en CPU (limita fps/latencia con ventanas grandes); luego MFT de hardware y texturas D3D11.
- Un IDR cada 2 s en lugar de reaccionar a PLI/NACK; si la red pierde paquetes habrá artefactos hasta el siguiente IDR.
- Una ventana sin cambios solo emite ~4 fps (repetición cada 250 ms); no es problema en juegos.
- Se selecciona por ventana (HWND), no aún por juego lanzado desde Dársena (`library::launch` no expone el PID).
- Sin STUN/TURN ni señalización por internet (F2).
- El binario de ejemplo/pruebas no se pudo ejecutar fuera de la app (la toolchain GNU sin manifiesto no carga ciertas DLL de WinRT); por eso la prueba va por la variable de entorno.

### Estado F1 (aparcado por decisión del usuario, 2026-10-17)

Escrito y compilando: audio WASAPI por proceso + Opus, entrada ViGEm/SendInput con permisos por invitado, comandos
`remote_guests`, `remote_set_permissions`, `remote_kick`, `remote_pause_input`, página del invitado con mando/teclado/ratón.
Probado en LAN: vídeo, pista de audio, canal de control y mando virtual (XInput lo vio con ViGEmBus instalado).
**Sin probar:** teclado/ratón con permisos, audio con un juego real, expulsar/pausar. Pendiente: buffer del invitado ~324 ms
(investigar), botón/panel en la UI, F2 (internet) y F3 (hardware).
**Aviso:** el código ya está en el árbol y entra en cualquier release compilada; para evitarlo, ponerlo detrás de una
feature de Cargo desactivada por defecto.

### Cómo activarlo (feature de Cargo, desactivada por defecto)

Por decisión del usuario, el juego remoto **no se compila en las releases normales**: sus dependencias (`webrtc`, `rtc`, `axum`,
`opus`, `vigem-client`, `windows`…) son opcionales y cuelgan de la feature `remote`, que no está en `default`. Sin la feature,
`mod remote`, los comandos `remote_*` y el gancho `DARSENA_REMOTE_TEST` no existen, y la UI no los referencia.

```powershell
# desde src-tauri, con el entorno de dev.ps1
cargo check --features remote                 # solo comprobar
cargo build --features remote                 # binario debug con juego remoto
npm run build -- --features remote            # instalador (si el script reenvía los argumentos a tauri build)
```

Al añadir comandos remotos nuevos: ponerles `#[cfg(feature = "remote")]` y listarlos en el bloque `app_handler!(...)` con feature
de `lib.rs`. La lista común de comandos vive en la macro `app_handler!`; añadir ahí cualquier comando normal nuevo.
