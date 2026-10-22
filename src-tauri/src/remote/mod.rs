//! Juego remoto (prototipo F0): captura una ventana y la emite por WebRTC a navegadores de la LAN.
//!
//! Diseño completo en `docs/juego-remoto.md`. F1 añade audio (WASAPI por proceso + Opus) y entrada remota
//! (ViGEmBus + SendInput con permisos). Sigue sin TURN ni señalización por internet. La señalización es un POST /offer al servidor HTTP local.

mod audio;
mod capture;
mod encode;
mod input;
mod session;
mod windows_list;

pub use input::{GuestView, Perms};
pub use session::{guests, kick, set_input_paused, set_permissions, start, status, stop, RemoteInfo};
pub use windows_list::{list_windows, WindowInfo};
