// Lee el catálogo completo: los ports (catalog/catalog.json) y los juegos web (catalog/webgames.json).
// Las herramientas que solo consultan el catálogo (portadas, sinopsis, descubrir) usan esto y ven todos los juegos.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
export const CATALOG_FILE = join(ROOT, "catalog", "catalog.json");
export const WEBGAMES_FILE = join(ROOT, "catalog", "webgames.json");

const games = (file) => (existsSync(file) ? JSON.parse(readFileSync(file, "utf8")).games ?? [] : []);

/** Ports y juegos web juntos. */
export const loadAll = () => [...games(CATALOG_FILE), ...games(WEBGAMES_FILE)];
