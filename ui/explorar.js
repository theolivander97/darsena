// Explorar: barra de la tienda (Explorar, Recomendaciones, Categorías, Miscelánea y buscador) y las filas
// «Debido a que jugaste…», «Recomendado para ti» y «Explorar por categoría».
// Se carga después de app.js y comparte sus variables y funciones.

// ---------- Barra de la tienda ----------

// Botón de la esquina (como el de Big Picture de Steam): abre Pantalla grande.
$("#bp-open")?.addEventListener("click", () => openBigPicture());
// La lupa lleva al listado con lo escrito (Intro hace lo mismo).
$("#search-btn")?.addEventListener("click", () => {
  show("store");
  renderStore();
  $("#search").focus();
});

const franchiseOfGame = (gm) => gm.ports.find((p) => p.franchise)?.franchise || "";

// Consolas con más juegos primero: las categorías que se enseñan en el menú y en la fila.
function categoriesByCount() {
  const counts = new Map();
  for (const gm of state.gameList) for (const c of consolesOf(gm)) counts.set(c, (counts.get(c) || 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], "es"));
}

let storeBarKey = "";
function renderStoreBar() {
  const cats = categoriesByCount();
  const key = `${LANG()}|${cats.length}|${state.gameList.length}`;
  const box = $("#store-menus");
  if (!box || key === storeBarKey) return;
  storeBarKey = key;
  const item = (nav, label) => `<button data-nav="${nav}">${label}</button>`;
  const menu = (label, items) => `<div class="tab-wrap"><button class="sb-top" type="button">${label} <span class="sb-caret">▾</span></button><div class="tab-menu">${items}</div></div>`;
  box.innerHTML = [
    menu(t("Explorar"), [
      item("store:sec-featured", t("Destacados y novedades")),
      item("store:sec-popular", t("Más descargados")),
      item("store:sec-recent", t("Añadidos recientemente")),
      item("store:all-title", t("Todos los juegos")),
    ].join("")),
    menu(t("Recomendaciones"), [
      item("store:sec-because", t("Debido a lo que jugaste")),
      item("store:sec-recs", t("Recomendado para ti")),
      item("community:com-mods", t("Mods populares del Taller")),
    ].join("")),
    menu(t("Categorías"), [
      item("store:sec-categories", t("Explorar por categoría")),
      "<hr>",
      ...cats.map(([c, n]) => `<button data-cat="${esc(c)}">${esc(consoleName(c))} <span class="muted small">${n}</span></button>`),
    ].join("")),
    menu(t("Miscelánea"), [
      item("webgames:", t("Web Games")),
      item("community:", t("Comunidad y Taller")),
      item("library:lib-shelves", t("Mi biblioteca")),
      "<hr>",
      item("submit:", t("Subir un port")),
    ].join("")),
  ].join("");
}

// Una categoría: filtra Explorar por esa consola y baja al listado.
document.addEventListener("click", (e) => {
  const c = e.target.closest("[data-cat]");
  if (!c) return;
  const wrap = c.closest(".tab-wrap");
  wrap?.classList.add("menu-off");
  wrap?.addEventListener("mouseleave", () => wrap.classList.remove("menu-off"), { once: true });
  show("store");
  $("#search").value = "";
  $("#only-links").checked = false;
  $("#console-filter").value = c.dataset.cat;
  renderStore();
  requestAnimationFrame(() => $("#store-bar").scrollIntoView({ behavior: "smooth", block: "start" }));
});

// Flechas de la fila de categorías.
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-cat-scroll]");
  if (!b) return;
  const row = b.closest(".cat-row").querySelector(".cat-scroll");
  row.scrollBy({ left: (b.dataset.catScroll === "next" ? 1 : -1) * row.clientWidth * 0.8, behavior: "smooth" });
});

// ---------- Filas ----------

const catHue = (name) => [...name].reduce((h, ch) => (h * 31 + ch.charCodeAt(0)) % 360, 7);

// Fila grande de categorías: un mosaico con las portadas de sus juegos y el nombre encima.
function categoryRowHtml() {
  const cats = categoriesByCount().slice(0, 18);
  if (!cats.length) return "";
  const tiles = cats.map(([c, n]) => {
    const covers = state.gameList.filter((gm) => consolesOf(gm).includes(c)).filter((gm) => coverSrc(gm.id)).slice(0, 6);
    const hue = catHue(c);
    return `<button class="cat-tile" data-cat="${esc(c)}" style="--h:${hue}">
      <span class="cat-mosaic">${covers.map((gm) => `<img src="${coverSrc(gm.id)}" alt="" loading="lazy" onerror="this.remove()">`).join("")}</span>
      <span class="cat-label">${esc(consoleName(c))}</span>
      <span class="cat-count">${t("{n} juegos", { n })}</span>
    </button>`;
  }).join("");
  return `<h2 class="section-title" id="sec-categories">${t("Explorar por categoría")}</h2>
    <div class="cat-row">
      <button class="car-nav prev" data-cat-scroll="prev" aria-label="${t("Anterior")}">‹</button>
      <div class="cat-scroll">${tiles}</div>
      <button class="car-nav next" data-cat-scroll="next" aria-label="${t("Siguiente")}">›</button>
    </div>`;
}

// Juegos parecidos a `seed` que todavía no tienes: misma saga puntúa más que la misma consola.
function similarTo(seed, limit = 14) {
  const fr = franchiseOfGame(seed);
  const seedConsoles = new Set(consolesOf(seed));
  return state.gameList
    .filter((gm) => gm.id !== seed.id && !isOwned(gm.id))
    .map((gm) => {
      let score = 0;
      if (fr && franchiseOfGame(gm) === fr) score += 6;
      if (consolesOf(gm).some((c) => seedConsoles.has(c))) score += 3;
      return { gm, score };
    })
    .filter((x) => x.score >= 3)
    .sort((a, b) => b.score - a.score || (b.gm.downloads || 0) - (a.gm.downloads || 0))
    .slice(0, limit)
    .map((x) => x.gm);
}

// «Debido a que jugaste X» (los dos juegos con más horas) y «Recomendado para ti» (según tus consolas).
function recommendationRowsHtml() {
  const played = [...(state.homeProfile?.games || [])]
    .sort((a, b) => (b.playtimeSecs || 0) - (a.playtimeSecs || 0))
    .map((p) => state.games.get(p.game))
    .filter(Boolean)
    .slice(0, 2);
  let html = "";
  played.forEach((seed, i) => {
    const similar = similarTo(seed);
    if (similar.length) html += rowHtml(t("Debido a que jugaste {title}", { title: esc(seed.title) }), similar, null, i === 0 ? "sec-because" : "");
  });
  // Consolas de los juegos que ya tienes o has jugado.
  const mine = new Set([...Object.keys(state.owned), ...(state.homeProfile?.games || []).map((p) => p.game)]);
  const counts = new Map();
  for (const id of mine) {
    const gm = state.games.get(id);
    if (gm) for (const c of consolesOf(gm)) counts.set(c, (counts.get(c) || 0) + 1);
  }
  const top = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  if (top) {
    const recs = state.gameList
      .filter((gm) => !mine.has(gm.id) && consolesOf(gm).includes(top))
      .sort((a, b) => (b.downloads || 0) - (a.downloads || 0))
      .slice(0, 14);
    if (recs.length) html += rowHtml(t("Recomendado para ti: más de {console}", { console: esc(consoleName(top)) }), recs, null, "sec-recs");
  }
  return html;
}

// Las filas que dependen de tu actividad se pintan al llegar tu perfil (es local, tarda un instante).
function exploreRows() {
  loadHomeProfile().then(() => {
    const box = $("#home-recs");
    if (box && state.view === "store") box.innerHTML = recommendationRowsHtml();
  });
  return `<div id="home-recs">${recommendationRowsHtml()}</div>${categoryRowHtml()}`;
}
