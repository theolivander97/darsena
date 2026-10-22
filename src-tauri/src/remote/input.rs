//! Entrada remota: mandos virtuales (ViGEmBus, Xbox 360) y teclado/ratón (SendInput),
//! con permisos por invitado.
//!
//! Reglas de seguridad:
//! - Teclado y ratón vienen desactivados; el anfitrión los concede por invitado.
//! - Solo se inyecta mientras el proceso del juego tiene el foco.
//! - Lista negra de combinaciones peligrosas (Win, Alt+Tab, Alt+F4, Ctrl+Esc, Ctrl+Shift+Esc).
//! - Al desconectar, expulsar o quitar un permiso se sueltan todas las teclas/botones y se desenchufa el mando.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};
use webrtc::data_channel::DataChannel;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub const MAX_PADS: usize = 4;

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Perms {
    pub pad: bool,
    pub keyboard: bool,
    pub mouse: bool,
}

impl Default for Perms {
    fn default() -> Self {
        Self { pad: true, keyboard: false, mouse: false }
    }
}

/// Mensajes que envía el navegador del invitado por los DataChannels.
#[derive(Deserialize)]
#[serde(tag = "t")]
pub enum Msg {
    #[serde(rename = "hello")]
    Hello { name: String },
    /// Estado completo del mando en formato XInput (idempotente: sirve el canal no fiable).
    #[serde(rename = "pad")]
    Pad { b: u16, lt: u8, rt: u8, lx: i16, ly: i16, rx: i16, ry: i16 },
    #[serde(rename = "key")]
    Key { c: String, d: bool },
    #[serde(rename = "mb")]
    MouseButton { b: u8, d: bool },
    #[serde(rename = "mm")]
    MouseMove { dx: i32, dy: i32 },
    #[serde(rename = "wh")]
    Wheel { dy: i32 },
}

pub struct Guest {
    pub id: u32,
    pub name: Mutex<String>,
    pub perms: Mutex<Perms>,
    pub slot: Mutex<Option<usize>>,
    pressed: Mutex<HashSet<String>>,
    buttons: Mutex<HashSet<u8>>,
    pub kick: tokio::sync::Notify,
    pub ctl: Mutex<Option<Arc<dyn DataChannel>>>,
}

impl Guest {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            name: Mutex::new(format!("Invitado {id}")),
            perms: Mutex::new(Perms::default()),
            slot: Mutex::new(None),
            pressed: Mutex::new(HashSet::new()),
            buttons: Mutex::new(HashSet::new()),
            kick: tokio::sync::Notify::new(),
            ctl: Mutex::new(None),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct GuestView {
    pub id: u32,
    pub name: String,
    pub pad: bool,
    pub keyboard: bool,
    pub mouse: bool,
    pub slot: Option<usize>,
}

struct Pad {
    owner: u32,
    target: Xbox360Wired<Arc<Client>>,
}

pub struct Hub {
    pid: u32,
    pub paused: AtomicBool,
    pads: Mutex<Vec<Option<Pad>>>,
    vigem: Mutex<Option<Result<Arc<Client>, String>>>,
    pub guests: Mutex<HashMap<u32, Arc<Guest>>>,
}

impl Hub {
    pub fn new(pid: u32) -> Self {
        Self {
            pid,
            paused: AtomicBool::new(false),
            pads: Mutex::new((0..MAX_PADS).map(|_| None).collect()),
            vigem: Mutex::new(None),
            guests: Mutex::new(HashMap::new()),
        }
    }

    pub fn views(&self) -> Vec<GuestView> {
        let mut v: Vec<GuestView> = self
            .guests
            .lock()
            .unwrap()
            .values()
            .map(|g| {
                let p = *g.perms.lock().unwrap();
                GuestView {
                    id: g.id,
                    name: g.name.lock().unwrap().clone(),
                    pad: p.pad,
                    keyboard: p.keyboard,
                    mouse: p.mouse,
                    slot: *g.slot.lock().unwrap(),
                }
            })
            .collect();
        v.sort_by_key(|g| g.id);
        v
    }

    /// Estado que se le comunica al invitado (permisos, número de mando, driver).
    pub fn state_json(&self, g: &Guest) -> String {
        let p = *g.perms.lock().unwrap();
        let vigem = match &*self.vigem.lock().unwrap() {
            Some(Err(e)) => Some(e.clone()),
            _ => None,
        };
        serde_json::json!({
            "t": "state", "pad": p.pad, "kb": p.keyboard, "mouse": p.mouse,
            "slot": *g.slot.lock().unwrap(), "paused": self.paused.load(Ordering::Relaxed),
            "vigemError": vigem,
        })
        .to_string()
    }

    pub async fn notify(&self, g: &Guest) {
        let ch = g.ctl.lock().unwrap().clone();
        if let Some(ch) = ch {
            let _ = ch.send_text(&self.state_json(g)).await;
        }
    }

    pub fn set_permissions(&self, id: u32, perms: Perms) -> Option<Arc<Guest>> {
        let g = self.guests.lock().unwrap().get(&id).cloned()?;
        *g.perms.lock().unwrap() = perms;
        // Quitar un permiso suelta lo que el invitado tuviera pulsado.
        if !perms.keyboard {
            self.release_keys(&g);
        }
        if !perms.mouse {
            self.release_buttons(&g);
        }
        if !perms.pad {
            self.free_pad(&g);
        }
        Some(g)
    }

    pub fn release_all(&self, g: &Guest) {
        self.release_keys(g);
        self.release_buttons(g);
        self.free_pad(g);
    }

    fn free_pad(&self, g: &Guest) {
        if let Some(slot) = g.slot.lock().unwrap().take() {
            let mut pads = self.pads.lock().unwrap();
            if pads[slot].as_ref().map(|p| p.owner) == Some(g.id) {
                pads[slot] = None; // al soltar el objetivo, ViGEm desenchufa el mando
            }
        }
    }

    fn release_keys(&self, g: &Guest) {
        let keys: Vec<String> = g.pressed.lock().unwrap().drain().collect();
        for k in keys {
            if let Some((sc, ext)) = scancode(&k) {
                send_key(sc, ext, false);
            }
        }
    }

    fn release_buttons(&self, g: &Guest) {
        let bs: Vec<u8> = g.buttons.lock().unwrap().drain().collect();
        for b in bs {
            send_mouse_button(b, false);
        }
    }

    /// El proceso del juego (o uno de su árbol de ventanas) tiene el foco.
    fn game_focused(&self) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            pid == self.pid
        }
    }

    fn vigem_client(&self) -> Result<Arc<Client>, String> {
        let mut v = self.vigem.lock().unwrap();
        if v.is_none() {
            *v = Some(
                Client::connect()
                    .map(Arc::new)
                    .map_err(|e| format!("ViGEmBus no disponible ({e:?}). Instala el driver ViGEmBus para usar mandos.")),
            );
        }
        v.as_ref().unwrap().clone()
    }

    fn ensure_slot(&self, g: &Guest) -> Result<usize, String> {
        if let Some(s) = *g.slot.lock().unwrap() {
            return Ok(s);
        }
        let client = self.vigem_client()?;
        let mut pads = self.pads.lock().unwrap();
        let slot = pads.iter().position(|p| p.is_none()).ok_or("no quedan mandos libres (máximo 4)")?;
        let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
        target.plugin().map_err(|e| format!("no se pudo crear el mando virtual: {e:?}"))?;
        target.wait_ready().map_err(|e| format!("el mando virtual no está listo: {e:?}"))?;
        pads[slot] = Some(Pad { owner: g.id, target });
        *g.slot.lock().unwrap() = Some(slot);
        Ok(slot)
    }

    /// Procesa un mensaje del invitado. Devuelve true si cambió el estado que hay que notificarle.
    pub fn handle(&self, g: &Arc<Guest>, msg: Msg) -> bool {
        let perms = *g.perms.lock().unwrap();
        let live = !self.paused.load(Ordering::Relaxed);
        match msg {
            Msg::Hello { name } => {
                let name: String = name.chars().filter(|c| !c.is_control()).take(24).collect();
                if !name.trim().is_empty() {
                    *g.name.lock().unwrap() = name;
                }
                true
            }
            Msg::Pad { b, lt, rt, lx, ly, rx, ry } => {
                if !perms.pad || !live {
                    return false;
                }
                let first = g.slot.lock().unwrap().is_none();
                match self.ensure_slot(g) {
                    Ok(slot) => {
                        let mut pads = self.pads.lock().unwrap();
                        if let Some(p) = pads[slot].as_mut() {
                            // Solo los botones estándar de XInput; el resto se descarta.
                            let pad = XGamepad {
                                buttons: XButtons { raw: b & 0xF3FF },
                                left_trigger: lt,
                                right_trigger: rt,
                                thumb_lx: lx,
                                thumb_ly: ly,
                                thumb_rx: rx,
                                thumb_ry: ry,
                            };
                            let _ = p.target.update(&pad);
                        }
                        first
                    }
                    Err(_) => first, // primer fallo: se informa por state_json (vigemError)
                }
            }
            Msg::Key { c, d } => {
                if !perms.keyboard || !live {
                    if !d {
                        // Permitimos soltar aunque ya no haya permiso (evita teclas atascadas).
                        self.try_release_key(g, &c);
                    }
                    return false;
                }
                let Some((sc, ext)) = scancode(&c) else { return false };
                if d {
                    if !self.game_focused() || blocked_combo(&c, &g.pressed.lock().unwrap()) {
                        return false;
                    }
                    g.pressed.lock().unwrap().insert(c);
                    send_key(sc, ext, true);
                } else {
                    self.try_release_key(g, &c);
                }
                false
            }
            Msg::MouseButton { b, d } => {
                if !live || (!perms.mouse && d) {
                    if !d {
                        self.try_release_button(g, b);
                    }
                    return false;
                }
                if d {
                    if !self.game_focused() {
                        return false;
                    }
                    g.buttons.lock().unwrap().insert(b);
                    send_mouse_button(b, true);
                } else {
                    self.try_release_button(g, b);
                }
                false
            }
            Msg::MouseMove { dx, dy } => {
                if perms.mouse && live && self.game_focused() {
                    send_mouse_move(dx.clamp(-2000, 2000), dy.clamp(-2000, 2000));
                }
                false
            }
            Msg::Wheel { dy } => {
                if perms.mouse && live && self.game_focused() {
                    send_mouse_wheel(dy.clamp(-1200, 1200));
                }
                false
            }
        }
    }

    fn try_release_key(&self, g: &Guest, code: &str) {
        if g.pressed.lock().unwrap().remove(code) {
            if let Some((sc, ext)) = scancode(code) {
                send_key(sc, ext, false);
            }
        }
    }

    fn try_release_button(&self, g: &Guest, b: u8) {
        if g.buttons.lock().unwrap().remove(&b) {
            send_mouse_button(b, false);
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
        if paused {
            let all: Vec<Arc<Guest>> = self.guests.lock().unwrap().values().cloned().collect();
            for g in all {
                self.release_keys(&g);
                self.release_buttons(&g);
            }
        }
    }
}

// ---------- Teclado: KeyboardEvent.code → scancode (set 1) ----------

fn scancode(code: &str) -> Option<(u16, bool)> {
    let s: u16 = match code {
        "Escape" => 0x01,
        "Digit1" => 0x02, "Digit2" => 0x03, "Digit3" => 0x04, "Digit4" => 0x05, "Digit5" => 0x06,
        "Digit6" => 0x07, "Digit7" => 0x08, "Digit8" => 0x09, "Digit9" => 0x0A, "Digit0" => 0x0B,
        "Minus" => 0x0C, "Equal" => 0x0D, "Backspace" => 0x0E, "Tab" => 0x0F,
        "KeyQ" => 0x10, "KeyW" => 0x11, "KeyE" => 0x12, "KeyR" => 0x13, "KeyT" => 0x14,
        "KeyY" => 0x15, "KeyU" => 0x16, "KeyI" => 0x17, "KeyO" => 0x18, "KeyP" => 0x19,
        "BracketLeft" => 0x1A, "BracketRight" => 0x1B, "Enter" => 0x1C, "ControlLeft" => 0x1D,
        "KeyA" => 0x1E, "KeyS" => 0x1F, "KeyD" => 0x20, "KeyF" => 0x21, "KeyG" => 0x22,
        "KeyH" => 0x23, "KeyJ" => 0x24, "KeyK" => 0x25, "KeyL" => 0x26,
        "Semicolon" => 0x27, "Quote" => 0x28, "Backquote" => 0x29, "ShiftLeft" => 0x2A, "Backslash" => 0x2B,
        "KeyZ" => 0x2C, "KeyX" => 0x2D, "KeyC" => 0x2E, "KeyV" => 0x2F, "KeyB" => 0x30,
        "KeyN" => 0x31, "KeyM" => 0x32, "Comma" => 0x33, "Period" => 0x34, "Slash" => 0x35,
        "ShiftRight" => 0x36, "NumpadMultiply" => 0x37, "AltLeft" => 0x38, "Space" => 0x39, "CapsLock" => 0x3A,
        "F1" => 0x3B, "F2" => 0x3C, "F3" => 0x3D, "F4" => 0x3E, "F5" => 0x3F,
        "F6" => 0x40, "F7" => 0x41, "F8" => 0x42, "F9" => 0x43, "F10" => 0x44,
        "NumLock" => 0x45, "ScrollLock" => 0x46,
        "Numpad7" => 0x47, "Numpad8" => 0x48, "Numpad9" => 0x49, "NumpadSubtract" => 0x4A,
        "Numpad4" => 0x4B, "Numpad5" => 0x4C, "Numpad6" => 0x4D, "NumpadAdd" => 0x4E,
        "Numpad1" => 0x4F, "Numpad2" => 0x50, "Numpad3" => 0x51, "Numpad0" => 0x52, "NumpadDecimal" => 0x53,
        "F11" => 0x57, "F12" => 0x58,
        // Extendidas
        "ControlRight" => return Some((0x1D, true)),
        "AltRight" => return Some((0x38, true)),
        "ArrowUp" => return Some((0x48, true)),
        "ArrowLeft" => return Some((0x4B, true)),
        "ArrowRight" => return Some((0x4D, true)),
        "ArrowDown" => return Some((0x50, true)),
        "Home" => return Some((0x47, true)),
        "End" => return Some((0x4F, true)),
        "PageUp" => return Some((0x49, true)),
        "PageDown" => return Some((0x51, true)),
        "Insert" => return Some((0x52, true)),
        "Delete" => return Some((0x53, true)),
        "NumpadEnter" => return Some((0x1C, true)),
        "NumpadDivide" => return Some((0x35, true)),
        // Bloqueadas a propósito: MetaLeft, MetaRight, ContextMenu, PrintScreen…
        _ => return None,
    };
    Some((s, false))
}

/// Combinaciones que nunca se inyectan, aunque el invitado tenga permiso de teclado.
fn blocked_combo(code: &str, pressed: &HashSet<String>) -> bool {
    let alt = pressed.contains("AltLeft") || pressed.contains("AltRight");
    let ctrl = pressed.contains("ControlLeft") || pressed.contains("ControlRight");
    let shift = pressed.contains("ShiftLeft") || pressed.contains("ShiftRight");
    (alt && (code == "Tab" || code == "F4" || code == "Escape" || code == "Space"))
        || (ctrl && code == "Escape")
        || (ctrl && shift && code == "Escape")
        || (ctrl && alt && code == "Delete")
        || (code == "Escape" && (ctrl || alt))
}

// ---------- SendInput ----------

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn send_key(scan: u16, extended: bool, down: bool) {
    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    send(&[INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }]);
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: u32) {
    send(&[INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }]);
}

fn send_mouse_move(dx: i32, dy: i32) {
    mouse(MOUSEEVENTF_MOVE, dx, dy, 0);
}

fn send_mouse_wheel(dy: i32) {
    mouse(MOUSEEVENTF_WHEEL, 0, 0, dy as u32);
}

/// Botones según `MouseEvent.button`: 0 izquierdo, 1 central, 2 derecho, 3 atrás, 4 adelante.
fn send_mouse_button(b: u8, down: bool) {
    match (b, down) {
        (0, true) => mouse(MOUSEEVENTF_LEFTDOWN, 0, 0, 0),
        (0, false) => mouse(MOUSEEVENTF_LEFTUP, 0, 0, 0),
        (1, true) => mouse(MOUSEEVENTF_MIDDLEDOWN, 0, 0, 0),
        (1, false) => mouse(MOUSEEVENTF_MIDDLEUP, 0, 0, 0),
        (2, true) => mouse(MOUSEEVENTF_RIGHTDOWN, 0, 0, 0),
        (2, false) => mouse(MOUSEEVENTF_RIGHTUP, 0, 0, 0),
        (3, true) => mouse(MOUSEEVENTF_XDOWN, 0, 0, 1),
        (3, false) => mouse(MOUSEEVENTF_XUP, 0, 0, 1),
        (4, true) => mouse(MOUSEEVENTF_XDOWN, 0, 0, 2),
        (4, false) => mouse(MOUSEEVENTF_XUP, 0, 0, 2),
        _ => {}
    }
}
