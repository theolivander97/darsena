//! Idioma de los mensajes que salen de Rust (errores, logros, nombres de descargas).
//! La interfaz elige el idioma y lo comunica con `set_language`; aquí solo se guarda si es inglés.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set(lang: &str) {
    ENGLISH.store(lang == "en", Ordering::Relaxed);
}

pub fn en() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Texto en el idioma actual: `tr!("español {x}", "english {x}")`, con las mismas variables que `format!`.
#[macro_export]
macro_rules! tr {
    ($es:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::en() { format!($en $(, $arg)*) } else { format!($es $(, $arg)*) }
    };
}
