//! Web seeds (BEP 19). Torrents como los de archive.org no tienen pares: solo listan servidores HTTP
//! (`url-list`) con los mismos archivos. librqbit los ignora, así que aquí se leen del `.torrent`, se bajan
//! con la descarga multiconexión de http_multi y después se comprueban los hashes de las piezas del torrent,
//! de modo que lo descargado es tan fiable como si viniera de pares.

use anyhow::Context;
use sha1::{Digest, Sha1};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Lo que hace falta de un `.torrent` para bajarlo desde sus web seeds.
#[derive(Clone)]
pub struct Plan {
    pub name: String,
    /// Torrent de un solo archivo (no tiene carpeta raíz).
    pub single: bool,
    /// (componentes de ruta, longitud) en el orden del torrent.
    pub files: Vec<(Vec<String>, u64)>,
    pub bases: Vec<String>,
    piece_len: u64,
    /// SHA-1 de cada pieza, de 20 bytes en 20.
    pieces: Vec<u8>,
}

enum V {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<V>),
    Dict(Vec<(Vec<u8>, V)>),
}

fn parse(b: &[u8], p: &mut usize, depth: usize) -> Option<V> {
    if depth > 32 {
        return None;
    }
    match *b.get(*p)? {
        b'i' => {
            let end = b[*p..].iter().position(|&c| c == b'e')? + *p;
            let n = std::str::from_utf8(&b[*p + 1..end]).ok()?.parse().ok()?;
            *p = end + 1;
            Some(V::Int(n))
        }
        b'l' => {
            *p += 1;
            let mut items = Vec::new();
            while *b.get(*p)? != b'e' {
                items.push(parse(b, p, depth + 1)?);
            }
            *p += 1;
            Some(V::List(items))
        }
        b'd' => {
            *p += 1;
            let mut items = Vec::new();
            while *b.get(*p)? != b'e' {
                let V::Bytes(k) = parse(b, p, depth + 1)? else { return None };
                items.push((k, parse(b, p, depth + 1)?));
            }
            *p += 1;
            Some(V::Dict(items))
        }
        b'0'..=b'9' => {
            let colon = b[*p..].iter().position(|&c| c == b':')? + *p;
            let n: usize = std::str::from_utf8(&b[*p..colon]).ok()?.parse().ok()?;
            let start = colon + 1;
            let data = b.get(start..start.checked_add(n)?)?.to_vec();
            *p = start + n;
            Some(V::Bytes(data))
        }
        _ => None,
    }
}

impl V {
    fn get(&self, key: &str) -> Option<&V> {
        match self {
            V::Dict(items) => items.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v),
            _ => None,
        }
    }
    fn int(&self) -> Option<u64> {
        match self {
            V::Int(n) => u64::try_from(*n).ok(),
            _ => None,
        }
    }
    fn text(&self) -> Option<String> {
        match self {
            V::Bytes(b) => String::from_utf8(b.clone()).ok(),
            _ => None,
        }
    }
}

/// Un nombre de archivo o carpeta del torrent: nunca debe salir de la carpeta de descarga.
fn safe_component(c: &str) -> bool {
    !c.is_empty()
        && c != "."
        && c != ".."
        && c.len() <= 255
        && !c.contains(['/', '\\', ':', '\0', '*', '?', '"', '<', '>', '|'])
        && !c.ends_with(['.', ' '])
}

/// Plan de web seeds de un `.torrent`, o None si no tiene (o no se puede usar).
pub fn plan_from_torrent(bytes: &[u8]) -> Option<Plan> {
    let mut p = 0;
    let root = parse(bytes, &mut p, 0)?;
    let info = root.get("info")?;
    let name = info.get("name")?.text()?;
    if !safe_component(&name) {
        return None;
    }

    let urls: Vec<String> = match root.get("url-list")? {
        V::Bytes(_) => root.get("url-list")?.text().into_iter().collect(),
        V::List(l) => l.iter().filter_map(V::text).collect(),
        _ => return None,
    };
    let bases: Vec<String> = urls
        .into_iter()
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .take(6)
        .collect();
    if bases.is_empty() {
        return None;
    }

    let piece_len = info.get("piece length")?.int()?;
    let V::Bytes(pieces) = info.get("pieces")? else { return None };
    if piece_len == 0 || pieces.is_empty() || pieces.len() % 20 != 0 {
        return None;
    }

    let (single, files) = match info.get("files") {
        None => (true, vec![(vec![name.clone()], info.get("length")?.int()?)]),
        Some(V::List(list)) => {
            let mut out = Vec::new();
            for f in list {
                let V::List(parts) = f.get("path")? else { return None };
                let comps: Vec<String> = parts.iter().filter_map(V::text).collect();
                if comps.len() != parts.len() || comps.is_empty() || !comps.iter().all(|c| safe_component(c)) {
                    return None;
                }
                out.push((comps, f.get("length")?.int()?));
            }
            (false, out)
        }
        _ => return None,
    };
    if files.is_empty() || files.len() > 2000 {
        return None;
    }
    Some(Plan { name, single, files, bases, piece_len, pieces: pieces.clone() })
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl Plan {
    pub fn total(&self) -> u64 {
        self.files.iter().map(|(_, l)| l).sum()
    }

    /// URL de un archivo del torrent en un servidor web seed (BEP 19).
    pub fn url(&self, base: &str, comps: &[String]) -> String {
        if self.single {
            return if base.ends_with('/') { format!("{base}{}", enc(&self.name)) } else { base.to_string() };
        }
        let base = if base.ends_with('/') { base.to_string() } else { format!("{base}/") };
        format!("{base}{}/{}", enc(&self.name), comps.iter().map(|c| enc(c)).collect::<Vec<_>>().join("/"))
    }

    /// Dónde queda un archivo dentro de `root`: igual que lo dejaría un cliente torrent.
    pub fn local_path(&self, root: &Path, comps: &[String]) -> PathBuf {
        if self.single {
            return root.join(&self.name);
        }
        let mut p = root.join(&self.name);
        p.extend(comps);
        p
    }

    /// Comprueba los hashes de todas las piezas sobre los archivos ya descargados en `root`.
    pub fn verify(&self, root: &Path) -> anyhow::Result<()> {
        let mut hasher = Sha1::new();
        let (mut in_piece, mut idx) = (0u64, 0usize);
        let mut buf = vec![0u8; 1 << 20];
        let check = |hasher: &mut Sha1, idx: &mut usize| -> anyhow::Result<()> {
            let got = std::mem::take(hasher).finalize();
            let want = self.pieces.get(*idx * 20..*idx * 20 + 20).context(tr!("el torrent tiene menos piezas de las esperadas", "the torrent has fewer pieces than expected"))?;
            anyhow::ensure!(got.as_slice() == want, tr!("la pieza {} no coincide con el torrent: la descarga está corrupta", "piece {} doesn't match the torrent: the download is corrupt", *idx));
            *idx += 1;
            Ok(())
        };
        for (comps, len) in &self.files {
            let path = self.local_path(root, comps);
            let mut f = std::fs::File::open(&path).with_context(|| tr!("falta {}", "{} is missing", path.display()))?;
            anyhow::ensure!(f.metadata()?.len() == *len, tr!("{} no tiene el tamaño esperado", "{} doesn't have the expected size", path.display()));
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                let mut chunk = &buf[..n];
                while !chunk.is_empty() {
                    let take = chunk.len().min((self.piece_len - in_piece) as usize);
                    hasher.update(&chunk[..take]);
                    in_piece += take as u64;
                    chunk = &chunk[take..];
                    if in_piece == self.piece_len {
                        check(&mut hasher, &mut idx)?;
                        in_piece = 0;
                    }
                }
            }
        }
        if in_piece > 0 {
            check(&mut hasher, &mut idx)?;
        }
        anyhow::ensure!(idx * 20 == self.pieces.len(), tr!("el torrent tiene más piezas de las descargadas", "the torrent has more pieces than were downloaded"));
        Ok(())
    }
}

/// Archivos de metadatos que archive.org añade a cada torrent y que no son parte del port.
const IA_META: &[&str] = &["_meta.xml", "_meta.sqlite", "_files.xml", "_reviews.xml", "__ia_thumb.jpg", "_archive.torrent"];

/// Deja la carpeta de un port lista para instalar: quita los metadatos de archive.org y, si todo
/// está dentro de una única carpeta raíz (lo normal en un torrent), sube su contenido un nivel.
pub fn tidy_layout(dest: &Path) -> std::io::Result<()> {
    fn clean(dir: &Path, depth: usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                if depth < 2 {
                    clean(&path, depth + 1);
                }
            } else if e.file_name().to_str().is_some_and(|n| IA_META.iter().any(|s| n.ends_with(s))) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    clean(dest, 0);

    let visible: Vec<_> = std::fs::read_dir(dest)?
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    if let [only] = visible.as_slice() {
        if only.path().is_dir() {
            // Se aparta primero: un archivo de dentro puede llamarse igual que la carpeta.
            let tmp = dest.join(".raiz-temporal");
            std::fs::rename(only.path(), &tmp)?;
            for child in std::fs::read_dir(&tmp)?.flatten() {
                std::fs::rename(child.path(), dest.join(child.file_name()))?;
            }
            std::fs::remove_dir(&tmp)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bencode_torrent(name: &str, data: &[u8], url_list: &str) -> Vec<u8> {
        let pieces: Vec<u8> = data.chunks(4).flat_map(|c| Sha1::digest(c).to_vec()).collect();
        let mut t = Vec::new();
        t.extend(format!("d8:url-list{}:{url_list}4:infod6:lengthi{}e4:name{}:{name}12:piece lengthi4e6:pieces{}:", url_list.len(), data.len(), name.len(), pieces.len()).bytes());
        t.extend(&pieces);
        t.extend(b"ee");
        t
    }

    #[test]
    fn plan_verifica_y_rechaza() {
        let data = b"0123456789abcdef!!";
        let t = bencode_torrent("juego.zip", data, "https://ejemplo.org/dl/");
        let plan = plan_from_torrent(&t).expect("plan");
        assert!(plan.single && plan.total() == data.len() as u64);
        assert_eq!(plan.url(&plan.bases[0], &plan.files[0].0), "https://ejemplo.org/dl/juego.zip");

        let dir = std::env::temp_dir().join(format!("darsena-ws-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("juego.zip"), data).unwrap();
        plan.verify(&dir).unwrap();
        std::fs::write(dir.join("juego.zip"), b"0123456789abcdeX!!").unwrap();
        assert!(plan.verify(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);

        // Un nombre con ".." no se acepta nunca.
        assert!(plan_from_torrent(&bencode_torrent("..", data, "https://ejemplo.org/")).is_none());
        // Sin web seeds, no hay plan.
        assert!(plan_from_torrent(b"d4:infod6:lengthi1e4:name1:a12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee").is_none());
    }

    #[test]
    fn url_multiarchivo() {
        let plan = Plan { name: "mi item".into(), single: false, files: vec![], bases: vec![], piece_len: 1, pieces: vec![0; 20] };
        assert_eq!(plan.url("https://archive.org/download", &["a b.zip".into()]), "https://archive.org/download/mi%20item/a%20b.zip");
    }
}
