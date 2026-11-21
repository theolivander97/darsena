// Pasa al catálogo los candidatos de GameWave que se han revisado a mano (ver gamewave.mjs).
// El título y la consola de cada repositorio se fijan aquí: los nombres de GameWave mezclan proyecto y juego.
//
// Uso:  node tools\catalog\gamewave-aniadir.mjs      (después: node tools\catalog\actualizar.mjs)

import { readFileSync, writeFileSync } from "node:fs";
import { CATALOG_FILE, loadAll } from "./cargar.mjs";

// repo -> [título del juego, consola]. Solo proyectos con historial real y build de Windows en sus releases.
const REVISADOS = {
  "diasurgical/devilution": ["Diablo", "PC"],
  "TerryCavanagh/VVVVVV": ["VVVVVV", "PC"],
  "OpenSC2K/OpenSC2K": ["SimCity 2000", "PC"],
  "isledecomp/isle": ["LEGO Island", "PC"],
  "alexbatalov/fallout1-ce": ["Fallout", "PC"],
  "alexbatalov/fallout2-ce": ["Fallout 2", "PC"],
  "2ship2harkinian/2ship2harkinian": ["The Legend of Zelda: Majora's Mask", "Nintendo 64"],
  "Keriew/augustus": ["Caesar III", "PC"],
  "OpenDUNE/OpenDUNE": ["Dune II", "PC"],
  "TheSuperHackers/GeneralsGameCode": ["Command & Conquer: Generals", "PC"],
  "dethrace-labs/dethrace": ["Carmageddon", "PC"],
  "kondrak/vkQuake2": ["Quake II", "PC"],
  "riperiperi/FreeSO": ["The Sims Online", "PC"],
  "sonicnext-dev/MarathonRecomp": ["Sonic the Hedgehog (2006)", "Xbox 360"],
  "shinyquagsire23/OpenJKDF2": ["Star Wars Jedi Knight: Dark Forces II", "PC"],
  "afwbkbc/glsmac": ["Sid Meier's Alpha Centauri", "PC"],
  "MorsGames/sm64plus": ["Super Mario 64", "Nintendo 64"],
  "codenamecpp/carnage3d": ["Grand Theft Auto", "PC"],
  "goliathret/EdgeOfTimeRecomp": ["Spider-Man: Edge of Time", "Xbox 360"],
  "tonihele/OpenKeeper": ["Dungeon Keeper II", "PC"],
  "jorio/Bugdom": ["Bugdom", "PC"],
  "jorio/Bugdom2": ["Bugdom 2", "PC"],
  "jorio/OttoMatic": ["Otto Matic", "PC"],
  "jorio/CroMagRally": ["Cro-Mag Rally", "PC"],
  "FWGS/cs16-client": ["Counter-Strike", "PC"],
  "kvark/vange-rs": ["Vangers", "PC"],
  "sergiou87/open-supaplex": ["Supaplex", "PC"],
  "OpenBiohazard2/OpenBiohazard2": ["Resident Evil 2", "PlayStation"],
  "Yokimitsuro/khdays-port": ["Kingdom Hearts 358/2 Days", "Nintendo DS"],
  "RandomityGuy/MBHaxe": ["Marble Blast Gold", "PC"],
  "harvestwhisperer/hm64-decomp": ["Harvest Moon 64", "Nintendo 64"],
  "TheFlyingFoool/DuckGameRebuilt": ["Duck Game", "PC"],
  "eandis/SDOJ-Recomp": ["DoDonPachi SaiDaiOuJou", "Xbox 360"],
  "opentestdriveunlimited/OpenTestDriveUnlimited": ["Test Drive Unlimited", "PC"],
  "Team-Beef-Studios/JKXR": ["Star Wars Jedi Knight II: Jedi Outcast", "PC"],
  "sp00nznet/burnout3": ["Burnout 3: Takedown", "Xbox"],
  "timftw21/burnout2-recompilation": ["Burnout 2: Point of Impact", "Xbox"],
  "Wargus/war1gus": ["Warcraft: Orcs & Humans", "PC"],
  "Endprodukt/cannonball-dx": ["OutRun", "PC"],
  "ZgzInfinity/OutRun": ["OutRun", "PC"],
  "isledecomp/racers": ["LEGO Racers", "PC"],
  "GrognardsFromHell/TemplePlus": ["The Temple of Elemental Evil", "PC"],
  "simutrans/simutrans": ["Simutrans", "PC"],
  "freegish/freegish": ["Gish", "PC"],
  "tim-tim707/SW_RACER_RE": ["Star Wars Episode I: Racer", "PC"],
  "MorsGames/MoonChildFE": ["Moon Child", "PC"],
  "dxx-redux/dxx-redux": ["Descent", "PC"],
  "OpenE2140/OpenE2140": ["Earth 2140", "PC"],
  "Open-BFME/openbfme-godot": ["The Lord of the Rings: The Battle for Middle-earth II", "PC"],
  "ReGlacier/ReHitman": ["Hitman: Codename 47", "PC"],
  "sourcehold/OpenSHC": ["Stronghold Crusader", "PC"],
  "PixieCatSupreme/AnodyneSharp": ["Anodyne", "PC"],
  "salvadorc17/Prince-Monogame": ["Prince of Persia 2: The Shadow and the Flame", "PC"],
  "doyaGu/Ballanced": ["Ballance", "PC"],
  "mstan/SonicTheHedgehog2Recomp": ["Sonic the Hedgehog 2", "Sega"],
  "jbalcomb/ReMoM": ["Master of Magic", "PC"],
  "zlatkok/swos-port": ["Sensible World of Soccer", "PC"],
  "Link4Electronics/Carpocalypse2": ["Carmageddon II: Carpocalypse Now", "PC"],
  "0lvin/heretic2": ["Heretic II", "PC"],
};

const known = new Set(loadAll().map((g) => g.repo?.toLowerCase()).filter(Boolean));
const found = new Map(JSON.parse(readFileSync(new URL("../../catalog/candidatos-gamewave.json", import.meta.url), "utf8")).map((c) => [c.repo.toLowerCase(), c]));
const catalog = JSON.parse(readFileSync(CATALOG_FILE, "utf8"));

let added = 0;
for (const [repo, [title, console_]] of Object.entries(REVISADOS)) {
  const c = found.get(repo.toLowerCase());
  if (!c) { console.log(`  (no sale en los candidatos: ${repo})`); continue; }
  if (known.has(repo.toLowerCase())) continue;
  const [author, port] = repo.split("/");
  catalog.games.push({
    title, port, author, repo, console: console_,
    // Las recompilaciones de consola son recientes y cambian rápido; los remakes de PC con años de historial, no.
    experimental: console_ !== "PC",
    platforms: ["Windows"],
  });
  added++;
}
writeFileSync(CATALOG_FILE, JSON.stringify(catalog));
console.log(`OK: ${added} ports añadidos a catalog.json (falta ejecutar actualizar.mjs)`);
