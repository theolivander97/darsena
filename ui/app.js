"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open: openDialog } = window.__TAURI__.dialog;

const state = {
  settings: null,
  catalog: [],
  byId: new Map(),
  sourceCounts: {},
  coverIndex: {},
  library: [],
  downloads: [],
  running: new Set(),
  view: "store",
  games: new Map(),   // id de juego -> { id, title, console, ports: [...] }
  gameList: [],
  detailGame: null,
  owned: {},        // id de juego -> fecha en que se añadió a la biblioteca
  libGame: null,
  libPort: null,
  user: null,             // usuario de GitHub con sesión iniciada
  reviewSummaries: {},    // id de juego -> { average, count } (resumen público)
  reviews: null,          // reseñas del juego abierto en la ficha
  myRating: 0,            // estrellas elegidas en el formulario de reseña
  info: { featured: [], games: {} },   // sinopsis, tráilers y destacados (catálogo de información)
};

const $ = (sel) => document.querySelector(sel);

// ---------- Utilidades ----------

// Lo que viene de fuentes o del catálogo se trata como texto: escapar siempre antes de meterlo en HTML.
function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function fmtBytes(n) {
  if (!n) return "—";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
}
function fmtDuration(secs) {
  if (secs == null) return "—";
  if (secs < 60) return `${secs} s`;
  const m = Math.floor(secs / 60), h = Math.floor(m / 60);
  return h ? `${h} h ${m % 60} min` : `${m} min`;
}
const fmtDate = (ts) => (ts ? new Date(ts * 1000).toLocaleString(LOCALE()) : t("Nunca"));
const fmtIsoDate = (iso) => (iso ? new Date(iso).toLocaleDateString(LOCALE()) : "");

function toast(msg, kind = "") {
  const el = document.createElement("div");
  el.className = `toast ${kind}`;
  el.textContent = msg;
  $("#toasts").appendChild(el);
  setTimeout(() => el.remove(), kind === "error" ? 7000 : 3500);
}
async function call(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    toast(String(e), "error");
    throw e;
  }
}

// Color por consola para las portadas generadas.
const CONSOLE_HUE = {
  "Nintendo 64": 140, "Super Nintendo": 265, "NES": 0, "GameCube": 255, "Wii": 195, "Wii U": 200,
  "Switch": 355, "Game Boy": 85, "Game Boy Color": 300, "Game Boy Advance": 245, "Nintendo DS": 210, "Nintendo 3DS": 350,
  "Virtual Boy": 0, "PlayStation": 220, "PlayStation 2": 230, "PlayStation 3": 225, "PlayStation 4": 215,
  "PSP": 235, "Xbox": 110, "Xbox 360": 100, "Sega": 210, "Arcade": 30, "PC": 190, "Otros": 200,
};
function coverStyle(g) {
  const h = CONSOLE_HUE[g.console] ?? 200;
  return `background:linear-gradient(160deg,hsl(${h} 55% 34%),hsl(${(h + 40) % 360} 45% 14%))`;
}
// Portadas reales de nuestra base (protocolo cover://, con caché en disco). Si falla, queda la generada.
const COVER_BASE = navigator.userAgent.includes("Windows") ? "http://cover.localhost/" : "cover://localhost/";
const coverSrc = (id) => (state.coverIndex[id]?.v ? `${COVER_BASE}${encodeURIComponent(id)}?v=${state.coverIndex[id].v}` : "");
const logoSrc = (id) => (state.coverIndex[id]?.l ? `${COVER_BASE}logo/${encodeURIComponent(id)}?v=${state.coverIndex[id].l}` : "");
const heroSrc = (id) => (state.coverIndex[id]?.h ? `${COVER_BASE}hero/${encodeURIComponent(id)}?v=${state.coverIndex[id].h}` : "");
// Si la imagen falla, se quita y vuelve a verse la portada generada.
const coverImg = (g, cls) =>
  coverSrc(g.id) ? `<img class="${cls}" src="${coverSrc(g.id)}" alt="" loading="lazy" decoding="async" onerror="this.parentNode.classList.remove('has-img');this.remove()">` : "";

// Cabecera de ficha: portada difuminada de fondo + cápsula a la izquierda, como en Steam.
function heroHtml(g, sub) {
  const src = coverSrc(g.id);
  const wide = heroSrc(g.id);
  return `<div class="hero ${src ? "has-art" : ""} ${wide ? "has-wide" : ""}" style="${coverStyle(g)}">
    ${wide ? `<img class="hero-wide" src="${wide}" alt="" onerror="this.parentNode.classList.remove('has-wide');this.remove()">` : ""}
    ${src ? `<img class="hero-bg" src="${src}" alt="" onerror="this.remove()">` : ""}
    ${src ? `<img class="hero-capsule" src="${src}" alt="" onerror="this.remove()">` : ""}
    <div class="hero-text">${sub}</div>
  </div>`;
}

// Tarjeta de un juego (agrupa todos sus ports).
function coverHtml(gm) {
  const img = coverImg(gm, "cover-img");
  const installed = gm.ports.some((p) => isInstalled(p.id));
  const links = gameLinks(gm);
  const n = gm.ports.length;
  return `<div class="cover ${img ? "has-img" : ""}" style="${coverStyle(gm)}">
    ${img}${img ? `<div class="cover-caption">${esc(gm.title)}</div>` : ""}
    <div class="pills">${installed ? `<span class="pill">${t("INSTALADO")}</span>` : isOwned(gm.id) ? `<span class="pill owned">${t("EN BIBLIOTECA")}</span>` : ""}${n > 1 ? `<span class="pill ports">${t("{n} PORTS", { n })}</span>` : ""}${links ? `<span class="pill links" title="${t("Enlaces en tus fuentes")}">${links} ⇣</span>` : ""}</div>
    <div class="cover-console">${esc(consolesText(gm))}</div>
    <div class="cover-title">${esc(gm.title)}</div>
    ${n > 1 ? `<div class="cover-port">${t("{n} ports", { n })}</div>` : ""}
  </div>`;
}

const consolesOf = (gm) => [...new Set(gm.ports.flatMap((p) => (p.consoles?.length ? p.consoles : [p.console])))];
const consolesText = (gm) => consolesOf(gm).map(consoleName).join(" · ");
const gameLinks = (gm) => gm.ports.reduce((a, p) => a + (state.sourceCounts[p.id] || 0), 0);
// Id del juego al que pertenece un port (para portadas y fichas desde la biblioteca).
const gameOfPort = (portId) => state.byId.get(portId)?.game ?? portId;

const isInstalled = (id) => state.library.some((g) => g.id === id);
const downloadOf = (id) => state.downloads.find((d) => d.key === id);
const repoUrl = (g) => (g.gitlab ? `https://gitlab.com/${g.gitlab}` : `https://github.com/${g.repo}`);
const repoLabel = (g) => (g.gitlab ? t("Ver en GitLab") : t("Ver en GitHub"));
// Port que no se publica en GitHub: solo se instala desde fuentes (torrent o mirror).
const isExternal = (g) => g?.origin === "external" && !g?.gitlab && !g?.branch;

// Pantalla vacía con el barco de Dársena en gris.
const emptyHtml = (title, text = "", extra = "", style = "") =>
  `<div class="empty" ${style ? `style="${style}"` : ""}><img class="empty-mark" src="logo.svg" alt=""><h3>${title}</h3>${text ? `<p class="muted">${text}</p>` : ""}${extra}</div>`;

// ---------- Navegación ----------

function show(view) {
  state.view = view;
  document.querySelectorAll(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${view}`));
  document.querySelectorAll(".tabs button").forEach((b) =>
    b.classList.toggle("active", b.dataset.view === view || (view === "detail" && b.dataset.view === "store")
      || (b.dataset.tab === "profile" && ["profile", "dev", "submit", "settings"].includes(view))));
  $("#dl-bar")?.classList.toggle("active", view === "downloads");
  $("#dl-bar")?.classList.toggle("active", view === "downloads");
  render();
}

function render() {
  injectCustomGames();
  switch (state.view) {
    case "store": renderStore(); break;
    case "detail": renderDetail(); break;
    case "library": renderLibrary(); break;
    case "downloads": renderDownloads(); break;
    case "settings": renderSettings(); break;
    case "profile": renderProfile(); break;
    case "submit": renderSubmit(); break;
    case "dev": renderDev(); break;
    case "community": renderCommunity(); break;
  }
  renderBadge();
  renderMini();
}

// Menús de las pestañas (al pasar el cursor): "vista:ancla".
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-nav]");
  if (!b) return;
  const [view, anchor] = b.dataset.nav.split(":");
  // El menú se cierra hasta que el cursor salga de la pestaña.
  const wrap = b.closest(".tab-wrap");
  wrap?.classList.add("menu-off");
  wrap?.addEventListener("mouseleave", () => wrap.classList.remove("menu-off"), { once: true });
  if (view === "submit") { openSubmit(); return; }
  if (view === "submitmod") { openModUpload(null); return; }
  if (view === "addgame") { openAddCustomGame(); return; }
  if (view === "bigpicture") { openBigPicture(); return; }
  if (view === "profile") { openProfile(null); return; }
  if (view === "dev") { openDevPanel(); return; }
  if (view === "settings") { openSettings(); return; }
  if (view === "library" && anchor === "home") { state.libGame = null; state.libPort = null; }
  if (view === "store" && anchor) {
    // Las secciones de Explorar solo existen sin búsqueda ni filtros.
    $("#search").value = ""; $("#console-filter").value = ""; $("#only-links").checked = false;
  }
  if (view === "library" && anchor === "lib-shelves") { state.libGame = null; state.libPort = null; }
  show(view);
  if (!anchor || anchor === "home") return;
  requestAnimationFrame(() => {
    const el = document.getElementById(anchor);
    if (!el) return;
    el.scrollIntoView({ behavior: "smooth", block: "start" });
    if (el.tagName === "INPUT") el.focus();
  });
});

// ---------- Explorar ----------

function fillConsoleFilter() {
  const counts = {};
  for (const gm of state.gameList) for (const c of consolesOf(gm)) counts[c] = (counts[c] || 0) + 1;
  const consoles = Object.keys(counts).sort((a, b) => a.localeCompare(b, "es"));
  $("#console-filter").innerHTML = `<option value="">${t("Todas las consolas")} (${state.gameList.length})</option>` +
    consoles.map((c) => `<option value="${esc(c)}">${esc(consoleName(c))} (${counts[c]})</option>`).join("");
}

let storeTimer = 0;
function renderStoreDebounced() {
  clearTimeout(storeTimer);
  storeTimer = setTimeout(renderStore, 80);
}

// ---------- Inicio de Explorar: carrusel de destacados y filas ----------

const fmtCount = (n) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)} M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)} K` : String(n));
// Fechas AAAA-MM-DD como día local (Date.parse las tomaría como medianoche UTC).
const daysAgo = (iso) => (iso ? (Date.now() - new Date(`${iso}T00:00:00`).getTime()) / 864e5 : Infinity);

// Destacados: primero lo añadido en las últimas dos semanas, luego lo más descargado; mejor si tiene hero.
function featuredGames() {
  const hasArt = (gm) => (state.coverIndex[gm.id]?.h ? 2 : 0) + (state.coverIndex[gm.id]?.v ? 1 : 0);
  const recent = state.gameList.filter((gm) => daysAgo(gm.addedAt) <= 14 && hasArt(gm))
    .sort((a, b) => (b.addedAt || "").localeCompare(a.addedAt || "") || hasArt(b) - hasArt(a));
  const popular = state.gameList.filter((gm) => gm.downloads && state.coverIndex[gm.id]?.h)
    .sort((a, b) => b.downloads - a.downloads);
  // Los destacados elegidos a mano (info-overrides.json) van siempre primero y en su orden.
  const out = (state.info?.featured || []).map((id) => state.games.get(id)).filter(Boolean);
  for (const gm of [...recent, ...popular]) {
    if (!out.includes(gm)) out.push(gm);
    if (out.length === 8) break;
  }
  // Sin datos de descargas aún: completa con juegos que tengan hero.
  for (const gm of state.gameList) {
    if (out.length >= 8) break;
    if (state.coverIndex[gm.id]?.h && !out.includes(gm)) out.push(gm);
  }
  return out;
}

let carousel = { items: [], index: 0, timer: 0, paused: false };

function slideHtml(gm) {
  const hero = heroSrc(gm.id);
  const cover = coverSrc(gm.id);
  const n = gm.ports.length;
  const owned = isOwned(gm.id);
  const franchise = gm.ports.find((p) => p.franchise)?.franchise;
  const isNew = daysAgo(gm.addedAt) <= 14;
  const pinned = (state.info?.featured || []).includes(gm.id);
  const syn = synopsisOf(gm.id)?.text;
  const tags = [...consolesOf(gm).map(consoleName), franchise, gm.ports.every((p) => p.experimental) ? t("Experimental") : null].filter(Boolean);
  return `
    <div class="car-art" data-open="${esc(gm.id)}" style="${coverStyle(gm)}">
      ${hero ? `<img class="car-hero" src="${hero}" alt="">`
        : cover ? `<img class="car-blur" src="${cover}" alt=""><img class="car-cover" src="${cover}" alt="">` : ""}
      <div class="car-art-title">${esc(gm.title)}</div>
    </div>
    <div class="car-side">
      <div class="car-side-top">
        ${cover ? `<img class="car-capsule" src="${cover}" alt="" data-open="${esc(gm.id)}">` : ""}
        <div>
          <h3 data-open="${esc(gm.id)}">${esc(gm.title)}</h3>
          <div class="car-meta">${isNew ? `<span class="car-new">${t("NUEVO")}</span>` : ""}${n > 1 ? t("{n} ports de la comunidad", { n }) : t("Port de la comunidad")}</div>
          ${gm.downloads ? `<div class="car-dl">⇩ ${t("{n} descargas en GitHub", { n: fmtCount(gm.downloads) })}</div>` : ""}
        </div>
      </div>
      <div class="car-why">${pinned ? t("<b>Destacado</b> del catálogo") : isNew ? t("<b>Novedad</b> en el catálogo") : gm.downloads ? t("<b>Popular</b> entre la comunidad") : t("<b>Destacado</b> del catálogo")}</div>
      ${syn ? `<p class="car-synopsis">${esc(syn)}</p>` : ""}
      <div class="tags">${tags.map((t) => `<span class="tag">${esc(t)}</span>`).join("")}</div>
      <div class="car-actions">
        ${owned
          ? `<button class="btn play owned" data-goto-lib="${esc(gm.id)}">✓ ${t("EN TU BIBLIOTECA")}</button>`
          : `<button class="btn play" data-add="${esc(gm.id)}">${t("AÑADIR A LA BIBLIOTECA")}</button>`}
      </div>
    </div>`;
}

function showSlide(i) {
  const { items } = carousel;
  if (!items.length) return;
  carousel.index = (i + items.length) % items.length;
  const slide = $("#car-slide");
  if (!slide) return;
  slide.classList.remove("in");
  slide.innerHTML = slideHtml(items[carousel.index]);
  void slide.offsetWidth; // reinicia la animación de entrada
  slide.classList.add("in");
  document.querySelectorAll(".car-dots button").forEach((b, k) => b.classList.toggle("active", k === carousel.index));
}

function rowHtml(title, games, extra, id = "") {
  if (!games.length) return "";
  return `<h2 class="section-title" ${id ? `id="${id}"` : ""}>${title}</h2>
    <div class="row-scroll">${games.map((gm) => `
      <div class="card row-card" data-open="${esc(gm.id)}">${coverHtml(gm)}
        ${extra ? `<div class="row-extra">${extra(gm)}</div>` : ""}
      </div>`).join("")}
    </div>`;
}

function renderHome() {
  const home = $("#home");
  clearInterval(carousel.timer);
  carousel.items = featuredGames();

  const popular = state.gameList.filter((gm) => gm.downloads).sort((a, b) => b.downloads - a.downloads).slice(0, 14);
  const recent = [...state.gameList].filter((gm) => gm.addedAt)
    .sort((a, b) => b.addedAt.localeCompare(a.addedAt) || a.title.localeCompare(b.title, "es")).slice(0, 14);

  home.innerHTML = `
    ${carousel.items.length ? `
    <h2 class="section-title" id="sec-featured">${t("Destacados y novedades")}</h2>
    <div class="carousel" id="carousel">
      <button class="car-nav prev" data-car="prev" aria-label="${t("Anterior")}">‹</button>
      <div class="car-slide" id="car-slide"></div>
      <button class="car-nav next" data-car="next" aria-label="${t("Siguiente")}">›</button>
    </div>
    <div class="car-dots">${carousel.items.map((_, k) => `<button data-car="${k}" aria-label="${t("Destacado")} ${k + 1}"></button>`).join("")}</div>` : ""}
    ${rowHtml(t("Más descargados"), popular, (gm) => `⇩ ${fmtCount(gm.downloads)}`, "sec-popular")}
    ${rowHtml(t("Añadidos recientemente"), recent, (gm) => fmtIsoDate(`${gm.addedAt}T00:00:00`), "sec-recent")}`;

  showSlide(Math.min(carousel.index, carousel.items.length - 1));
  const el = $("#carousel");
  if (el) {
    el.onmouseenter = () => (carousel.paused = true);
    el.onmouseleave = () => (carousel.paused = false);
  }
  carousel.timer = setInterval(() => {
    if (!carousel.paused && state.view === "store" && !document.hidden) showSlide(carousel.index + 1);
  }, 7000);
}

function renderStore() {
  const q = $("#search").value.trim().toLowerCase();
  const consoleSel = $("#console-filter").value;
  const onlyLinks = $("#only-links").checked;
  const noAi = $("#no-ai").checked;
  const sort = $("#sort").value;

  let games = state.gameList.filter((gm) =>
    (!consoleSel || consolesOf(gm).includes(consoleSel)) &&
    (!onlyLinks || gameLinks(gm)) &&
    (!noAi || gm.ports.some((p) => !p.ai || p.ai === "none")) &&
    (!q || gm._search.includes(q)));

  if (sort === "console") games = [...games].sort((a, b) => a.console.localeCompare(b.console, "es") || a.title.localeCompare(b.title, "es"));
  if (sort === "links") games = [...games].sort((a, b) => gameLinks(b) - gameLinks(a));
  if (sort === "downloads") games = [...games].sort((a, b) => (b.downloads || 0) - (a.downloads || 0));
  if (sort === "recent") games = [...games].sort((a, b) => (b.addedAt || "").localeCompare(a.addedAt || ""));

  // El Inicio (carrusel y filas) solo se ve sin búsqueda ni filtros.
  const browsing = !q && !consoleSel && !onlyLinks;
  $("#home").classList.toggle("hidden", !browsing);
  $("#all-title").classList.toggle("hidden", !browsing);
  if (browsing) renderHome();

  const ports = games.reduce((a, gm) => a + gm.ports.length, 0);
  $("#store-count").textContent = t("{a} de {b} juegos · {c} ports", { a: games.length, b: state.gameList.length, c: ports });
  $("#store-grid").innerHTML = games.length
    ? games.map((gm) => `<div class="card" data-open="${esc(gm.id)}">${coverHtml(gm)}</div>`).join("")
    : emptyHtml(t("Sin resultados"), t("Prueba con otro nombre o quita los filtros."), "", "grid-column:1/-1");
}

// ---------- Ficha (desde Explorar) ----------

const isOwned = (gameId) => Object.hasOwn(state.owned, gameId);

function actionButtonHtml(id) {
  if (isInstalled(id)) {
    const running = state.running.has(id);
    const upd = state.updates?.[id];
    const busy = downloadOf(id) && !["error", "completed"].includes(downloadOf(id).status);
    if (busy) return `<button class="btn" data-goto="downloads">${t("Actualizando…")}</button>`;
    return `<button class="btn play" data-play="${esc(id)}" ${running ? "disabled" : ""}>${running ? t("EJECUTANDO") : icon("play") + t("JUGAR")}</button>${upd && !running
      ? `<button class="btn update" data-update="${esc(id)}" title="${t("Hay una versión nueva: {v}", { v: esc(upd.latest) })}">${icon("downloads")}${t("ACTUALIZAR")}</button>` : ""}`;
  }
  const d = downloadOf(id);
  if (d && d.status !== "error" && d.status !== "completed") {
    const pct = d.total ? Math.floor((d.downloaded / d.total) * 100) : 0;
    return `<button class="btn" data-goto="downloads">${t("Descargando…")} ${pct}%</button>`;
  }
  if (isExternal(state.byId.get(id))) {
    return state.sourceCounts[id]
      ? `<button class="btn play" data-install-first="${esc(id)}">${t("INSTALAR")}</button>`
      : `<button class="btn play" disabled title="${t("Este port no está en GitHub: añade una fuente que lo incluya")}">${t("NECESITA UNA FUENTE")}</button>`;
  }
  return `<button class="btn play" data-install-official="${esc(id)}">${t("INSTALAR")}</button>`;
}

function downloadFromText(gm) {
  const ext = gm.ports.filter(isExternal).length;
  return !ext ? t("GitHub oficial") : ext === gm.ports.length ? t("Solo desde fuentes") : t("GitHub y fuentes");
}

// Orden de ports: instalados, luego estables, luego con más enlaces, luego por nombre.
function sortedPorts(gm) {
  return [...gm.ports].sort((a, b) =>
    isInstalled(b.id) - isInstalled(a.id) ||
    a.experimental - b.experimental ||
    (state.sourceCounts[b.id] || 0) - (state.sourceCounts[a.id] || 0) ||
    a.port.localeCompare(b.port, "es"));
}

// La ficha solo informa y añade a la biblioteca; elegir port, instalar y jugar se hace desde la biblioteca.
function renderDetail() {
  const gm = state.games.get(state.detailGame);
  if (!gm) { show("store"); return; }
  const n = gm.ports.length;
  const owned = isOwned(gm.id);
  const platforms = [...new Set(gm.ports.flatMap((p) => p.platforms || []))];
  const franchise = gm.ports.find((p) => p.franchise)?.franchise;
  const experimental = gm.ports.every((p) => p.experimental);
  const aliases = [...new Set(gm.ports.flatMap((p) => p.aliases || []))].filter((a) => a !== gm.title);
  const id = esc(gm.id);
  const info = state.info?.games?.[gm.id];
  const syn = synopsisOf(gm.id);
  const trailers = (info?.trailers || []).filter((t) => t.kind === "youtube" && /^[\w-]{11}$/.test(t.id || ""));

  $("#detail").innerHTML = `
    ${heroHtml(gm, `<div class="hero-console">${esc(consolesText(gm))}</div>
        <h1>${esc(gm.title)}</h1>
        <div class="hero-port">${n > 1 ? t("{n} ports de la comunidad", { n }) : t("Port de la comunidad")}</div>
        ${ratingLine(state.reviewSummaries[gm.id], "hero-rating")}`)}
    <button class="btn ghost back" data-goto="store">← ${t("Volver a Explorar")}</button>
    <div id="dev-banner"></div>
    <div class="action-bar">
      ${owned
        ? `<button class="btn play owned" data-goto-lib="${id}">✓ ${t("EN TU BIBLIOTECA")}</button>`
        : `<button class="btn play" data-add="${id}">${t("AÑADIR A LA BIBLIOTECA")}</button>`}
      <div class="ab-port"><b>${t("Gratis")}</b><span class="muted small">${n > 1 ? t("{n} ports de distintos autores", { n }) : t("1 port")}${experimental ? ` · ${t("experimental")}` : ""}</span></div>
      ${owned ? `<button class="btn ghost" data-goto-lib="${id}">${t("Ir a la biblioteca")}</button>` : ""}
    </div>
    <div class="detail-body">
      <div>
        ${syn ? `<h2 class="section-title first">${t("Acerca del juego")}</h2>
          <p class="synopsis">${esc(syn.text)}</p>
          <div class="muted small synopsis-src">${syn.translated ? `${t(syn.translated === "en" ? "Traducido del inglés" : "Traducido del español")} · `
            : syn.lang !== LANG() ? `${t(syn.lang === "en" ? "Texto en inglés" : "Texto en español")} · ` : ""}${synopsisSource(syn.source)}</div>` : ""}
        ${trailers.length ? `<h2 class="section-title">${trailers.length > 1 ? t("Tráilers") : t("Tráiler")}</h2>
          <div class="trailer" id="trailer">${trailerThumb(trailers[0])}</div>
          ${trailers.length > 1 ? `<div class="trailer-list">${trailers.map((tv, i) =>
            `<button class="btn ghost ${i === 0 ? "on" : ""}" data-trailer="${id}:${i}">${esc(trailerTitle(tv, i))}</button>`).join("")}</div>` : ""}` : ""}
        <div id="author-info"></div>
        ${syn || trailers.length ? `<h2 class="section-title">${t("Sobre el port")}</h2>` : ""}
        <p class="desc">${t("Port comunitario de <b>{title}</b> ({console}).", { title: esc(gm.title), console: esc(consoleName(gm.console)) })}
          ${n > 1 ? t("Hay {n} ports de distintos autores: elegirás cuál instalar desde tu biblioteca.", { n }) : t("Añádelo a tu biblioteca para instalarlo y jugar.")}</p>
        <p class="req">${t("Los ports no incluyen los datos del juego original: normalmente necesitarás tu propia ROM, ISO o archivos del juego.")}</p>
        <div id="req-box"></div>
        <div class="workshop-box" data-game="${id}"></div>
      </div>
      <div class="side-box">
        <dl>
          <dt>${consolesOf(gm).length > 1 ? t("Consolas") : t("Consola")}</dt><dd>${esc(consolesText(gm))}</dd>
          <dt>${t("Plataformas")}</dt><dd>${esc(platforms.join(", ") || "—")}</dd>
          ${franchise ? `<dt>${t("Saga")}</dt><dd>${esc(franchise)}</dd>` : ""}
          <dt>${t("Ports")}</dt><dd>${n}</dd>
          <dt>${t("Descarga")}</dt><dd>${esc(downloadFromText(gm))}</dd>
          ${aiOfGame(gm) ? `<dt>${t("Uso de IA")}</dt><dd>${esc(aiLabel(aiOfGame(gm)))}</dd>` : ""}
        </dl>
        ${aliases.length ? `<div class="tags">${aliases.map((t) => `<span class="tag">${esc(t)}</span>`).join("")}</div>` : ""}
      </div>
    </div>
    <section id="reviews" class="reviews"></section>`;
  renderReviews();
  renderRequirements(gm, []);
  loadAuthorInfo(gm);
  renderWorkshopFor(gm.id);
  if (state.devOf?.[gm.id] === undefined) loadDevOf(gm.id);
  else renderDevBanner(gm.id);
  if (state.reviews?.game !== gm.id) loadReviews(gm.id);
}

// ---------- Sinopsis y tráilers ----------

function synopsisSource(src) {
  if (!src?.name) return "";
  const name = src.name === "Wikipedia" ? "Wikipedia (CC BY-SA 4.0)" : t(src.name);
  return src.url ? `${t("Fuente")}: <a data-url="${esc(src.url)}">${esc(name)}</a>` : `${t("Fuente")}: ${esc(name)}`;
}

// Sinopsis en el idioma de la interfaz si la hay; si no, la que exista.
function synopsisOf(gameId) {
  const i = state.info?.games?.[gameId];
  return (LANG() === "en" ? i?.synopsisEn || i?.synopsis : i?.synopsis || i?.synopsisEn) || null;
}
const trailerTitle = (tr, i) => (LANG() === "en" && tr.titleEn) || tr.title || `${t("Tráiler")} ${i + 1}`;

// Miniatura con botón de reproducir: YouTube solo se carga al pulsar (más ligero y más privado).
const trailerThumb = (tv) =>
  `<button class="trailer-thumb" data-trailer-play="${esc(tv.id)}" style="background-image:url(https://i.ytimg.com/vi/${esc(tv.id)}/hqdefault.jpg)" title="${t("Reproducir")}"><span class="play">▶</span></button>`;
const trailerPlayer = (id) =>
  `<iframe src="https://www.youtube-nocookie.com/embed/${esc(id)}?autoplay=1&rel=0" allow="autoplay; encrypted-media; fullscreen; picture-in-picture" allowfullscreen></iframe>`;

// ---------- Reseñas (GitHub Discussions) ----------

function starsHtml(value) {
  const full = Math.round(value);
  return `<span class="stars">${"★".repeat(full)}<span class="off">${"★".repeat(5 - full)}</span></span>`;
}
const ratingLine = (sum, cls = "") =>
  sum?.count ? `<div class="${cls}">${starsHtml(sum.average)} <span class="muted small">${sum.average.toFixed(1)} · ${t(sum.count === 1 ? "{n} reseña" : "{n} reseñas", { n: sum.count })}</span></div>` : "";

// ¿Eres desarrollador de algún port de este juego? Entonces puedes responder a sus reseñas.
async function loadDevOf(gameId) {
  if (!state.user) return;
  (state.devOf ||= {})[gameId] = await invoke("my_ports_of_game", { game: gameId }).catch(() => []);
  if (state.view === "detail" && state.detailGame === gameId) { renderReviews(); renderDevBanner(gameId); }
}

// Si eres desarrollador de un port de este juego, la ficha te lo dice y te deja editarla.
function renderDevBanner(gameId) {
  const mine = state.devOf?.[gameId] || [];
  const box = $("#dev-banner");
  if (!box) return;
  box.innerHTML = mine.length ? `<div class="dev-banner">${icon("edit")}
    <span>${t("Eres desarrollador de este port. Puedes personalizar su ficha y responder a sus reseñas.")}</span>
    ${mine.map((id) => `<button class="btn primary small-btn" data-edit-page="${esc(id)}">${t("Editar ficha de {port}", { port: esc(state.byId.get(id)?.port || id) })}</button>`).join("")}
  </div>` : "";
}

async function loadReviews(gameId) {
  state.reviews = { game: gameId, loading: true };
  renderReviews();
  let data;
  try {
    data = await invoke("get_reviews", { game: gameId });
  } catch (e) {
    data = { error: String(e) };
  }
  if (state.reviews?.game !== gameId) return;
  setReviews(gameId, data);
}

function setReviews(gameId, data) {
  state.reviews = { game: gameId, ...data };
  const mine = data.reviews?.find((r) => r.mine);
  state.myRating = mine?.rating || 0;
  if (data.count != null) state.reviewSummaries[gameId] = { average: data.average, count: data.count };
  if (state.view === "detail" && state.detailGame === gameId) {
    document.querySelector("#detail .hero-rating")?.remove();
    document.querySelector("#detail .hero-port")?.insertAdjacentHTML("afterend", ratingLine(state.reviewSummaries[gameId], "hero-rating"));
  }
  renderReviews();
}

function renderReviews() {
  const box = $("#reviews");
  const r = state.reviews;
  if (!box || !r || r.game !== state.detailGame) return;
  if (r.loading) { box.innerHTML = `<h2>${t("Reseñas")}</h2><p class="muted">${t("Cargando reseñas…")}</p>`; return; }
  const list = r.reviews || [];
  const mine = list.find((x) => x.mine);

  const form = state.user
    ? `<div class="review-form">
        <div class="row">
          <b>${mine ? t("Tu reseña") : t("Escribe una reseña")}</b>
          <span class="star-pick">${[1, 2, 3, 4, 5].map((i) => `<button type="button" data-star="${i}" class="${i <= state.myRating ? "on" : ""}" title="${t("{i} de 5", { i })}">★</button>`).join("")}</span>
        </div>
        <textarea id="review-text" maxlength="4000" placeholder="${t("¿Qué tal funciona el port? Rendimiento, fallos, mejoras…")}">${esc(mine?.text || "")}</textarea>
        <div class="row">
          <span class="muted small">${t("Se publica en GitHub como <b>{login}</b>.", { login: esc(state.user.login) })}</span>
          <span style="flex:1"></span>
          ${mine ? `<button class="btn ghost" data-review-delete="${esc(r.game)}">${t("Borrar")}</button>` : ""}
          <button class="btn primary" data-review-submit="${esc(r.game)}">${mine ? t("Actualizar reseña") : t("Publicar reseña")}</button>
        </div>
      </div>`
    : `<div class="review-form row"><span class="muted">${t("Inicia sesión con GitHub para escribir tu reseña.")}</span>
        <span style="flex:1"></span><button class="btn primary" data-login>${t("Iniciar sesión con GitHub")}</button></div>`;

  box.innerHTML = `
    <h2>${t("Reseñas")} ${r.discussionUrl ? `<a class="muted small" data-url="${esc(r.discussionUrl)}" style="cursor:pointer">${t("Ver en GitHub")} ↗</a>` : ""}</h2>
    ${r.count ? `<div class="reviews-summary"><span class="big">${r.average.toFixed(1)}</span><div>${starsHtml(r.average)}<div class="muted small">${t(r.count === 1 ? "{n} reseña" : "{n} reseñas", { n: r.count })}</div></div></div>` : ""}
    ${form}
    ${r.error ? `<p class="muted">${t("No se pudieron cargar las reseñas:")} ${esc(r.error)}</p>` : ""}
    ${!list.length && !r.error ? `<p class="muted">${t("Todavía no hay reseñas. ¡Sé el primero!")}</p>` : ""}
    ${list.map((x) => {
      const isDev = (state.devOf?.[r.game] || []).length > 0;
      const myReply = (x.replies || []).find((y) => y.mine);
      return `<div class="review ${x.mine ? "mine" : ""}">
      <img src="${esc(x.avatar)}" alt="" onerror="this.style.visibility='hidden'">
      <div>
        <div class="who"><a data-profile="${esc(x.mine ? "" : x.user)}" title="${t("Ver su perfil")}">${esc(x.user)}</a> ${starsHtml(x.rating)} <span class="muted small">${fmtIsoDate(x.date)}</span>${x.mine ? `<span class="tag">${t("TÚ")}</span>` : ""}</div>
        ${x.text ? `<div class="text">${esc(x.text)}</div>` : ""}
        ${(x.replies || []).map((y) => `<div class="dev-reply">
          <img src="${esc(y.avatar)}" alt="" onerror="this.style.visibility='hidden'">
          <div><div class="who"><b>${esc(y.user)}</b> <span class="tag dev">${t("DESARROLLADOR")}</span> <span class="muted small">${fmtIsoDate(y.date)}</span></div>
          <div class="text">${esc(y.text)}</div></div>
        </div>`).join("")}
        ${isDev && x.id ? `<div class="reply-box" data-reply-box="${esc(x.id)}">
          <button class="btn ghost small-btn" data-reply-open="${esc(x.id)}">${icon("reply")}${myReply ? t("Editar tu respuesta") : t("Responder como desarrollador")}</button>
          <div class="reply-form hidden">
            <textarea maxlength="4000" placeholder="${t("Gracias por tu reseña…")}">${esc(myReply?.text || "")}</textarea>
            <button class="btn primary small-btn" data-reply-send="${esc(x.id)}">${t("Publicar respuesta")}</button>
          </div>
        </div>` : ""}
      </div>
    </div>`;
    }).join("")}`;
}

// ---------- Perfil y logros ----------

// Tu perfil (login null) o el de otro jugador de Dársena (desde su Gist).
async function openProfile(login) {
  state.profileOf = login;
  state.profile = null;
  state.profileError = null;
  show("profile");
  try {
    state.profile = login ? await invoke("get_user_profile", { login }) : await invoke("get_profile");
    if (login && !state.profile) state.profileError = t("{login} todavía no ha guardado su perfil de Dársena.", { login });
  } catch (e) {
    state.profileError = String(e);
  }
  if (state.profileOf === login) renderProfile();
}

async function syncProfile() {
  try { await invoke("sync_profile"); } catch {}
}

const fmtHours = (secs) => (secs >= 3600 ? `${(secs / 3600).toFixed(secs >= 36000 ? 0 : 1)} h` : secs < 60 ? "0 min" : fmtDuration(secs));

function renderProfile() {
  const box = $("#profile");
  if (!box || state.view !== "profile") return;
  const p = state.profile;
  const mine = !state.profileOf;
  if (!p) {
    box.innerHTML = state.profileError
      ? emptyHtml(t("Perfil no disponible"), esc(state.profileError))
      : `<div class="muted">${t("Cargando perfil…")}</div>`;
    return;
  }
  const unlocked = p.achievements.filter((a) => a.unlockedAt);
  const name = p.login || t("Tu perfil");
  const sync = !mine ? (p.gistUrl ? `<button class="btn ghost" data-url="${esc(p.gistUrl)}">${t("Ver su Gist")} ↗</button>` : "")
    : state.user ? `<span class="muted small">${p.gistUrl ? t("Se guarda en tu Gist público de GitHub.") : t("Se guarda en tu Gist público de GitHub al sincronizar.")}</span>
        ${p.gistUrl ? `<button class="btn ghost" data-url="${esc(p.gistUrl)}">${t("Ver Gist")} ↗</button>` : ""}
        <button class="btn ghost" data-profile-sync>${t("Sincronizar ahora")}</button>`
    : `<span class="muted small">${t("Inicia sesión con GitHub para guardar tu perfil y tus logros, y verlos desde cualquier PC.")}</span>
        <button class="btn primary" data-login>${t("Iniciar sesión")}</button>`;

  box.innerHTML = `
    <div class="profile-head">
      ${p.avatar ? `<img class="profile-avatar" src="${esc(p.avatar)}" alt="">` : `<div class="profile-avatar guest">${icon("user")}</div>`}
      <div class="profile-name">
        <h1>${esc(name)}</h1>
        <div class="row">${sync}</div>
      </div>
    </div>
    <div class="stats profile-stats">
      <div>${t("TIEMPO DE JUEGO")}<b>${fmtHours(p.totalSecs)}</b></div>
      <div>${t("JUEGOS JUGADOS")}<b>${p.gamesPlayed}</b></div>
      <div>${t("LOGROS")}<b>${unlocked.length} / ${p.achievements.length}</b></div>
      <div>${t("RESEÑAS")}<b>${p.reviews}</b></div>
    </div>
    <h2 class="section-title">${t("Logros")}</h2>
    <div class="ach-grid">${p.achievements.map((a) => `
      <div class="ach ${a.unlockedAt ? "on" : ""}" title="${esc(a.description)}">
        <div class="ach-icon">${a.icon}</div>
        <div><div class="ach-name">${esc(a.name)}</div>
          <div class="muted small">${esc(a.description)}${a.unlockedAt ? ` · ${fmtDate(a.unlockedAt).split(",")[0]}` : ""}</div></div>
      </div>`).join("")}
    </div>
    <h2 class="section-title">${t("Juegos")}</h2>
    ${p.games.length ? `<div class="profile-games">${p.games.map((g) => {
      const gm = state.games.get(g.game) || { id: g.game, title: g.title, console: g.console };
      return `<div class="pg" data-open="${esc(g.game)}">
        <div class="ph" style="${coverStyle(gm)}">${coverImg(gm, "ph-img")}</div>
        <div class="pg-title">${esc(gm.title)}<div class="muted small">${esc(consoleName(gm.console))}</div></div>
        <div class="pg-time"><b>${fmtHours(g.playtimeSecs)}</b><div class="muted small">${g.lastPlayed ? t("última vez {date}", { date: fmtDate(g.lastPlayed).split(",")[0] }) : ""}</div></div>
      </div>`;
    }).join("")}</div>` : `<p class="muted">${mine ? t("Aún no has jugado a ningún port desde Dársena.") : t("Todavía no ha jugado a nada.")}</p>`}`;
}

// ---------- Cuenta de GitHub ----------

function renderAccount() {
  const u = state.user;
  // La pestaña de arriba lleva tu nombre, como en Steam.
  const tab = $("#tab-profile-name");
  if (tab) {
    if (u) { tab.removeAttribute("data-i18n"); tab.textContent = u.login.toUpperCase(); }
    else { tab.dataset.i18n = "PERFIL"; tab.textContent = t("PERFIL"); }
  }
  $("#account").innerHTML = u
    ? `<button class="account-btn" data-account title="${t("Cuenta de GitHub")}"><img class="avatar" src="${esc(u.avatarUrl)}" alt=""><span>${esc(u.login)}</span> ▾</button>`
    : `<button class="account-btn" data-account title="${t("Tu perfil")}"><span class="avatar guest">${icon("user")}</span><span>${t("Perfil")}</span> ▾</button>
       <button class="btn ghost" data-login>${t("Iniciar sesión")}</button>`;
}

function toggleAccountMenu(force) {
  let menu = $("#account-menu");
  if (menu || force === false) { menu?.remove(); return; }
  menu = document.createElement("div");
  menu.id = "account-menu";
  menu.className = "account-menu";
  const item = (attrs, ico, label) => `<button ${attrs}>${icon(ico)}${label}</button>`;
  menu.innerHTML = state.user
    ? `${item('data-profile=""', "star", t("Mi perfil y logros"))}
      ${item("data-submit", "plus", t("Subir un port"))}
      ${item("data-dev-panel", "chart", t("Panel del desarrollador"))}
      ${item('data-settings=""', "settings", t("Ajustes"))}
      <hr>
      ${item(`data-url="${esc(state.user.htmlUrl)}"`, "github", t("Ver mi perfil de GitHub"))}
      ${item("data-logout", "close", t("Cerrar sesión"))}`
    : `${item('data-profile=""', "star", t("Mi perfil y logros"))}
      ${item('data-settings=""', "settings", t("Ajustes"))}
      <hr>
      ${item("data-login", "github", t("Iniciar sesión con GitHub"))}`;
  document.body.appendChild(menu);
}

// Device Flow: GitHub da un código, el usuario lo introduce en github.com/login/device y aquí se espera la autorización.
let loginRun = 0;
async function login() {
  const dlg = $("#login-dialog");
  const run = ++loginRun;
  $("#login-code").textContent = "········";
  $("#login-status").textContent = t("Pidiendo código a GitHub…");
  $("#login-open").disabled = true;
  if (!dlg.open) dlg.showModal();
  let code;
  try {
    code = await invoke("auth_start");
  } catch (e) {
    $("#login-status").textContent = `${t("No se pudo iniciar sesión:")} ${e}`;
    return;
  }
  if (run !== loginRun || !dlg.open) return;
  $("#login-code").textContent = code.userCode;
  $("#login-open").disabled = false;
  $("#login-open").onclick = async () => {
    try { await navigator.clipboard.writeText(code.userCode); } catch {}
    await call("open_url", { url: code.verificationUri });
    $("#login-status").textContent = t("Código copiado. Esperando a que autorices en GitHub…");
  };
  $("#login-status").textContent = t("Esperando a que autorices en GitHub…");

  const deadline = Date.now() + code.expiresIn * 1000;
  while (run === loginRun && dlg.open && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, ((code.interval || 5) + 1) * 1000));
    if (run !== loginRun || !dlg.open) return;
    try {
      const user = await invoke("auth_poll", { deviceCode: code.deviceCode });
      if (user) {
        state.user = user;
        dlg.close();
        toast(t("Sesión iniciada como {login}", { login: user.login }), "ok");
        renderAccount();
        syncProfile();
        if (state.view === "detail") loadReviews(state.detailGame);
        if (state.view === "submit") openSubmit();
        return;
      }
    } catch (e) {
      $("#login-status").textContent = String(e);
      return;
    }
  }
  if (run === loginRun && dlg.open) $("#login-status").textContent = t("El código caducó. Cierra y vuelve a intentarlo.");
}
$("#login-dialog").addEventListener("close", () => { loginRun++; });

async function loadAccount() {
  try { state.user = await invoke("auth_user"); } catch { state.user = null; }
  renderAccount();
  if (state.user) syncProfile();
  if (state.view === "detail" && state.user) loadReviews(state.detailGame);
}

async function loadOptions(g) {
  const id = g.id;
  const external = isExternal(g);
  const [official, sources] = await Promise.allSettled([
    external ? Promise.reject("externo") : invoke("get_official_build", { id }),
    invoke("get_source_options", { id }),
  ]);
  if (state.view !== "library" || state.libPort !== id || !$("#options")) return;
  const dis = downloadOf(id) && !["error", "completed"].includes(downloadOf(id).status) ? "disabled" : "";

  const off = official.status === "fulfilled" ? official.value : null;
  let html = external ? `<div class="option">
    <div><div class="opt-title">${t("Port externo")} <span class="tag warn">${t("No está en GitHub")}</span></div>
      <div class="muted small">${t("Su autor no lo publica en GitHub: se instala desde una fuente que incluya el port (torrent o mirror).")}</div></div>
    ${g.homepage ? `<button class="btn ghost" data-url="${esc(g.homepage)}">${t("Página del proyecto")}</button>` : ""}
  </div>` : `<div class="option">
    <div><div class="opt-title">${t("GitHub oficial")} ${off ? `· ${esc(off.version)}` : ""}
      ${off?.prerelease ? `<span class="tag">pre-release</span>` : ""}</div>
      <div class="muted small">${off ? `${esc(off.assetName)} · ${fmtBytes(off.size)} · ${fmtIsoDate(off.publishedAt)}`
        : esc(String(official.reason))}</div></div>
    <button class="btn primary" data-install-official="${esc(id)}" ${off ? dis : "disabled"}>${t("Instalar")}</button>
  </div>`;

  const opts = sources.status === "fulfilled" ? sources.value : [];
  html += opts.map((o) => `<div class="option">
    <div><div class="opt-title">${esc(o.label || o.sourceName)}
      ${o.links.map((l) => `<span class="tag">${esc(l.kind.toUpperCase())}</span>`).join(" ")}</div>
      <div class="muted small">${t("Fuente")}: ${esc(o.sourceName)}${o.size ? ` · ${fmtBytes(o.size)}` : ""}${o.uploadDate ? ` · ${esc(o.uploadDate)}` : ""}${o.links.length > 1 ? ` · ${t("{n} mirrors", { n: o.links.length })}` : ""}</div></div>
    <button class="btn" data-install-source="${esc(id)}" data-src="${esc(o.sourceUrl)}" data-idx="${o.index}" ${dis}>${t("Instalar")}</button>
  </div>`).join("");
  if (!opts.length) html += `<div class="muted small" style="margin-top:8px">${external
    ? t("Ninguna de tus fuentes trae este port. Añade una fuente que lo incluya en su lista <code>downloads</code>.")
    : t("Tus fuentes no tienen enlaces para este port.")}</div>`;
  $("#options").innerHTML = html;
}

// Datos de juego de tus fuentes para un port instalado (torrent, magnet o enlace directo).
async function loadDataOptions(g, inst) {
  const opts = await invoke("get_data_options", { id: g.id }).catch(() => []);
  if (state.view !== "library" || state.libPort !== g.id || !$("#data-options")) return;
  const task = downloadOf(`data:${g.id}`);
  const busy = task && !["error", "completed"].includes(task.status);
  const html = opts.map((o) => {
    // La carpeta Datos es del juego: cuenta como instalado si lo instaló cualquiera de sus ports.
    const gm = state.games.get(g.game);
    const done = state.library.filter((x) => gm?.ports.some((p) => p.id === x.id))
      .some((x) => (x.data || []).some((d) => d.sourceUrl === o.sourceUrl && d.index === o.index));
    const compat = o.compatible === true ? `<span class="tag ok">${t("Compatible")}</span>`
      : o.compatible === false ? `<span class="tag warn">${t("No compatible")}</span>` : `<span class="tag">${t("Contenido sin indicar")}</span>`;
    const info = [`${t("Fuente")}: ${esc(o.sourceName)}`, o.contains ? t("contiene {what}", { what: esc(o.contains.toUpperCase()) }) : "", o.size ? fmtBytes(o.size) : "", o.sha1 ? t("SHA-1 comprobado") : "",
      o.format === "zip" ? t("se extrae") : "", o.dest ? t("en {dest}", { dest: `<code>${esc(o.dest)}</code>` }) : ""].filter(Boolean).join(" · ");
    return `<div class="option">
      <div><div class="opt-title">${esc(o.label)}
        ${[...new Set(o.links.map((l) => l.kind.toUpperCase()))].map((k) => `<span class="tag">${esc(k)}</span>`).join(" ")}
        ${compat}${done ? ` <span class="tag ok">${t("Instalado")}</span>` : ""}</div>
        <div class="muted small">${info}${o.links.length > 1 ? ` · ${t("{n} mirrors", { n: o.links.length })}` : ""}</div>
        ${zipCapable(o) ? `<label class="check small" style="margin-top:6px"><input type="checkbox" data-extract="${esc(o.sourceUrl)}|${o.index}" ${o.format === "zip" ? "checked" : ""}> ${t("Extraer el .zip al instalar")}</label>` : ""}</div>
      <button class="btn ${done || o.compatible === false ? "" : "primary"}" data-install-data="${esc(g.id)}" data-src="${esc(o.sourceUrl)}" data-idx="${o.index}" ${busy || o.compatible === false ? "disabled" : ""}
        title="${o.compatible === false ? t("Este port necesita {what}", { what: esc(needsText(g)) }) : ""}">
        ${busy ? t("Descargando…") : done ? t("Reinstalar") : t("Instalar datos")}</button>
    </div>`;
  }).join("");
  $("#data-options").innerHTML = html || `<div class="muted small">${t("Tus fuentes no tienen datos para este juego. Puedes añadir una fuente con tus copias (lista <code>data</code>: torrent, magnet o enlace directo).")}</div>`;
}

// ---------- Instalación ----------

let pendingInstall = null;

// Mismo criterio que el launcher para nombrar carpetas en Windows ("A: B" -> "A - B").
function windowsName(s) {
  const out = s.replace(/:/g, " -").replace(/[<>"/\\|?*]/g, " ").replace(/\s+/g, " ").trim().replace(/[. ]+$/, "");
  return out || "Juego";
}

// ¿Estos datos son un .zip? Entonces se ofrece la casilla «extraer el .zip» (por defecto, lo que diga la fuente).
const zipCapable = (o) => o?.format === "zip" || /\.zip$/i.test(o?.dest || "");

// Lo que acepta un port: "ISO · RVZ" + nota.
const needsText = (g) => (g.needs || []).map((n) => n.toUpperCase()).join(" · ");

function updateInstallLayout() {
  const g = state.byId.get(pendingInstall?.id);
  if (!g) return;
  const root = $("#install-path").value.replace(/[\\/]+$/, "");
  const game = windowsName(state.games.get(g.game)?.title ?? g.title);
  $("#install-layout").innerHTML = t("Se instalará en {path} · tus datos irán a {data}", {
    path: `<code>${esc(root)}\\${esc(game)}\\${esc(windowsName(g.port))}</code>`, data: `<code>${esc(game)}\\Datos</code>` });
}

async function openInstall(id, source) {
  const g = state.byId.get(id);
  if (!g) return;
  pendingInstall = { id, source };
  $("#install-title").textContent = t("Instalar {title}", { title: state.games.get(g.game)?.title ?? g.title });
  $("#install-from").textContent = t("Port: {port} (por {author})", { port: g.port, author: t(g.author) }) + " · " + (source ? t("desde la fuente seleccionada") : t("desde GitHub oficial"));
  $("#install-path").value = state.settings.installDir;
  updateInstallLayout();
  $("#install-needs").innerHTML = g.needs?.length
    ? t("Este port no incluye el juego: necesita tus datos en formato {what}.", { what: `<b>${esc(needsText(g))}</b>${g.needsNote ? ` (${esc(g.needsNote)})` : ""}` })
    : t("Los ports no incluyen los datos del juego original: normalmente te pedirán tu propia copia al iniciarlos.");

  // Datos de tus fuentes que sirven para este port (los incompatibles no se ofrecen).
  const opts = (await invoke("get_data_options", { id }).catch(() => [])).filter((o) => o.compatible !== false);
  const box = $("#install-data");
  box.classList.toggle("hidden", !opts.length);
  $("#install-with-data").checked = opts.length > 0;
  pendingInstall.dataOpts = opts;
  $("#install-data-select").innerHTML = opts.map((o) =>
    `<option value="${esc(o.sourceUrl)}|${o.index}">${esc(o.label)}${o.contains ? ` · ${esc(o.contains.toUpperCase())}` : ""}${o.size ? ` · ${fmtBytes(o.size)}` : ""} — ${esc(o.sourceName)}</option>`).join("");
  syncExtractRow();
  $("#install-dialog").showModal();
}

// Muestra la casilla de extraer solo si los datos elegidos son un .zip.
function syncExtractRow() {
  const [sourceUrl, index] = $("#install-data-select").value.split(/\|(?=\d+$)/);
  const o = (pendingInstall?.dataOpts || []).find((x) => x.sourceUrl === sourceUrl && String(x.index) === index);
  const show = zipCapable(o);
  $("#install-extract-row").classList.toggle("hidden", !show);
  if (show) $("#install-extract").checked = o.format === "zip";
}
$("#install-data-select").addEventListener("change", syncExtractRow);

$("#install-browse").addEventListener("click", async () => {
  const dir = await openDialog({ directory: true, defaultPath: $("#install-path").value });
  if (dir) { $("#install-path").value = dir; updateInstallLayout(); }
});

$("#install-dialog").addEventListener("close", async () => {
  if ($("#install-dialog").returnValue !== "ok" || !pendingInstall) return;
  const { id, source } = pendingInstall;
  pendingInstall = null;
  let data = null;
  if (!$("#install-data").classList.contains("hidden") && $("#install-with-data").checked) {
    const [sourceUrl, index] = $("#install-data-select").value.split(/\|(?=\d+$)/);
    data = { sourceUrl, index: Number(index), extract: $("#install-extract-row").classList.contains("hidden") ? null : $("#install-extract").checked };
  }
  try {
    await call("install_game", { id, source: source || null, installDir: $("#install-path").value, data });
    toast(data ? t("Descargando el port y después tus datos") : t("Descarga iniciada"), "ok");
    state.downloads = await invoke("list_downloads");
    render();
  } catch {}
});

// ---------- Biblioteca ----------

// Juegos de la biblioteca (añadidos o con algún port instalado), por título.
function libraryGames() {
  return Object.keys(state.owned)
    .map((id) => state.games.get(id))
    .filter(Boolean)
    .sort((a, b) => a.title.localeCompare(b.title, "es"));
}

const installedPortOf = (gm) => gm.ports.find((p) => isInstalled(p.id));

function renderLibrary() {
  const list = $("#lib-list");
  const games = libraryGames();
  if (!games.length) {
    list.innerHTML = `<div class="muted" style="padding:16px">${t("Tu biblioteca está vacía.")}</div>`;
    $("#lib-detail").innerHTML = emptyHtml(t("Tu biblioteca está vacía"), t("Añade juegos desde Explorar para instalarlos y jugar."),
      `<button class="btn primary" data-goto="store">${icon("explore")}${t("Ir a Explorar")}</button>`);
    return;
  }
  // Sin juego elegido (o si ya no está en la biblioteca) se ve la página principal.
  if (state.libGame && !games.some((gm) => gm.id === state.libGame)) { state.libGame = null; state.libPort = null; }
  renderLibSidebar(games);
  if (!state.libGame) { renderLibraryHome(games); return; }

  const gm = state.games.get(state.libGame);
  const ports = sortedPorts(gm);
  if (!ports.some((p) => p.id === state.libPort)) state.libPort = (installedPortOf(gm) ?? ports[0]).id;
  const g = state.byId.get(state.libPort);
  const inst = state.library.find((x) => x.id === g.id);
  const anyInstalled = !!installedPortOf(gm);

  // Aquí sí se ve el proyecto de cada port: es donde se elige qué instalar.
  const selector = ports.length > 1 ? `
    <h3 class="section-title">${t("Ports disponibles")} <span class="muted small">· ${ports.length}</span></h3>
    <div class="port-tabs">${ports.map((p) => `
      <button class="port-tab ${p.id === g.id ? "active" : ""}" data-port="${esc(p.id)}">
        <span class="pt-name">${esc(p.port)}</span>
        <span class="pt-author">${t("por {author}", { author: esc(t(p.author)) })}</span>
        <span class="pt-badges">${p.ai && p.ai !== "none" ? `<span class="tag ai" title="${esc(aiLabel(p.ai))}">${t("IA")}</span>` : ""}${isInstalled(p.id) ? `<span class="tag ok">${t("Instalado")}</span>` : ""}${p.experimental ? `<span class="tag warn">${t("Experimental")}</span>` : ""}${isExternal(p) ? `<span class="tag">${t("Solo fuentes")}</span>` : ""}${state.sourceCounts[p.id] ? `<span class="tag">${state.sourceCounts[p.id]} ⇣</span>` : ""}</span>
      </button>`).join("")}
    </div>` : "";

  const installedBlock = inst ? `
    ${inst.exe ? "" : `<p class="req">${t("No se detectó el ejecutable. Usa «Elegir ejecutable» para indicarlo.")}</p>`}
    <div id="cloud-box" data-cloud="${esc(inst.id)}"></div>
    <details class="lib-details">
      <summary>${t("Detalles de la instalación")}</summary>
      <dl>
        <dt>${t("Versión")}</dt><dd>${esc(inst.version || "—")}</dd>
        <dt>${t("Carpeta")}</dt><dd>${esc(inst.path)}</dd>
        <dt>${t("Ejecutable")}</dt><dd>${esc(inst.exe || "—")}</dd>
        <dt>${t("Origen")}</dt><dd>${esc(t(inst.origin))}</dd>
        <dt>${t("Instalado")}</dt><dd>${fmtDate(inst.installedAt)}</dd>
      </dl>
    </details>
    ${inst.linked ? `<p class="muted small" style="margin-top:10px">${t("Vinculado a una instalación que ya tenías: Dársena no toca sus archivos.")}</p>` : `
    <h3 class="section-title">${t("Datos del juego")} <span class="muted small">· ${t("necesita {what}", { what: esc(needsText(g) || "—") })}</span>
      <button class="btn ghost small-btn" data-data-folder="${esc(g.id)}">${t("Abrir carpeta Datos")}</button></h3>
    ${g.needsNote ? `<p class="muted small">${esc(g.needsNote)}</p>` : ""}
    <div id="data-options"><div class="muted">${t("Cargando…")}</div></div>`}` : `
    <p class="req">${t("Este port no incluye el juego.")} ${g.needs?.length ? t("Necesita tus datos en formato {what}.", { what: `<b>${esc(needsText(g))}</b>${g.needsNote ? ` (${esc(g.needsNote)})` : ""}` }) : t("Consulta el README del proyecto para saber qué datos necesita.")}</p>
    <h3 class="section-title">${t("Opciones de descarga de {port}", { port: esc(g.port) })}</h3>
    <div id="options"><div class="muted">${t("Cargando…")}</div></div>
    <div class="option locate">
      <div><div class="opt-title">${t("¿Ya lo tienes instalado?")}</div>
        <div class="muted small">${t("Si ya tienes este port en tu PC, elige su ejecutable y podrás jugarlo desde Dársena sin descargar nada.")}</div></div>
      <button class="btn" data-locate="${esc(g.id)}">${icon("file")}${t("Elegir ejecutable…")}</button>
    </div>`;

  const lastPlayed = inst?.lastPlayed || playedOf(gm.id)?.lastPlayed;
  // El tiempo es del juego entero (suma de todos sus ports); el del port instalado es solo una parte.
  const played = Math.max(playedOf(gm.id)?.playtimeSecs ?? 0, inst?.playtimeSecs ?? 0);
  loadHomeProfile();
  $("#lib-detail").innerHTML = `
    ${libHeroHtml(gm)}
    <div class="lib-bar" id="lib-action">${actionButtonHtml(g.id)}
      ${inst ? `
        <div class="lib-stat"><span>${t("ÚLTIMA SESIÓN")}</span><b>${lastPlayed ? fmtIsoDate(lastPlayed * 1000) : t("Nunca")}</b></div>
        <div class="lib-stat clock">${icon("clock")}<div><span>${t("TIEMPO DE JUEGO")}</span><b>${fmtHours(played)}</b></div></div>`
      : `<div class="lib-stat"><span>${t("TAMAÑO DE DESCARGA")}</span><b id="lib-size">—</b></div>
        ${lastPlayed ? `<div class="lib-stat"><span>${t("ÚLTIMA SESIÓN")}</span><b>${fmtIsoDate(lastPlayed * 1000)}</b></div>` : ""}`}
      <div class="lib-stat port"><span>${t("PORT")}</span><b>${esc(g.port)}</b><em>${g.custom ? t("Añadido a mano") : t("por {author}", { author: esc(t(g.author)) })}${g.experimental ? ` · <span class="exp">${t("EXPERIMENTAL")}</span>` : ""}</em></div>
      <span style="flex:1"></span>
      <button class="sq-btn" data-gear="${esc(gm.id)}" title="${t("Opciones")}">${icon("settings")}</button>
      <button class="sq-btn" data-open="${esc(gm.id)}" title="${t("Ver ficha")}">${icon("info")}</button>
      <button class="sq-btn fav-btn ${isFavorite(gm.id) ? "on" : ""}" data-fav="${esc(gm.id)}" title="${isFavorite(gm.id) ? t("Quitar de favoritos") : t("Añadir a favoritos")}">${icon("star")}</button>
    </div>
    ${selector}
    ${installedBlock}
    ${anyInstalled ? "" : `<div class="row" style="margin-top:24px">
      <button class="btn ghost danger" data-remove-lib="${esc(gm.id)}">${icon("trash")}${t("Quitar de la biblioteca")}</button></div>`}
    <div class="workshop-box" data-game="${esc(gm.id)}"></div>`;
  renderWorkshopFor(gm.id);
  renderCloudBox();
  if (!inst) {
    loadOptions(g);
    if (!isExternal(g)) invoke("get_official_build", { id: g.id }).then((b) => { const el = $("#lib-size"); if (el && b?.size) el.textContent = fmtBytes(b.size); }).catch(() => {});
  } else if (!inst.linked) loadDataOptions(g, inst);
  decorateWithAuthor(gm, g);
}

// Cabecera grande de la biblioteca: arte panorámico y el logo del juego encima (o su nombre).
function libHeroHtml(gm) {
  const hero = heroSrc(gm.id);
  const cover = coverSrc(gm.id);
  const logo = logoSrc(gm.id);
  return `<div class="lib-hero" id="lib-hero" style="${coverStyle(gm)}">
    ${hero ? `<img class="lib-hero-img" src="${hero}" alt="">` : cover ? `<img class="lib-hero-blur" src="${cover}" alt="">` : ""}
    ${logo ? `<img class="lib-logo" src="${logo}" alt="" onerror="this.nextElementSibling.classList.remove('hidden');this.remove()">` : ""}
    <h1 class="lib-title ${logo ? "hidden" : ""}">${esc(gm.title)}</h1>
  </div>`;
}

// Si el autor del port puso fondo o logo en su darsena.json, sustituyen a los de SteamGridDB.
async function decorateWithAuthor(gm, port) {
  if (isExternal(port)) return;
  const a = await invoke("get_author_info", { id: port.id }).catch(() => null);
  const hero = $("#lib-hero");
  if (!a || !hero || state.libPort !== port.id) return;
  if (a.background) {
    hero.querySelector(".lib-hero-img, .lib-hero-blur")?.remove();
    hero.insertAdjacentHTML("afterbegin", `<img class="lib-hero-img" src="${esc(a.background)}" alt="">`);
  }
  if (a.logo) {
    hero.querySelector(".lib-logo")?.remove();
    hero.querySelector(".lib-title")?.classList.add("hidden");
    hero.insertAdjacentHTML("beforeend", `<img class="lib-logo" src="${esc(a.logo)}" alt="">`);
  }
}

// ---------- Biblioteca: barra lateral y página principal ----------

const isFavorite = (gameId) => (state.settings?.favorites || []).includes(gameId);

function libItemHtml(gm) {
  const inst = installedPortOf(gm);
  const running = gm.ports.some((p) => state.running.has(p.id));
  return `<div class="lib-item ${gm.id === state.libGame ? "active" : ""} ${inst ? "" : "not-installed"}" data-lib="${esc(gm.id)}">
    <div class="ph" style="${coverStyle(gm)}">${coverImg(gm, "ph-img")}</div>
    <span class="name">${esc(gm.title)}</span>
    ${running ? `<span class="running">●</span>` : ""}
  </div>`;
}

// La cabecera (página principal y buscador) se crea una vez para no perder el foco al escribir; la lista se repinta.
function renderLibSidebar(games) {
  const list = $("#lib-list");
  if (!$("#lib-search")) {
    list.innerHTML = `<button class="lib-home" data-lib-home><span class="lib-home-ico">${icon("home")}</span><span class="lib-home-txt"></span></button>
      <div class="lib-search"><input id="lib-search" type="search" autocomplete="off"></div>
      <div id="lib-items"></div>`;
    $("#lib-search").addEventListener("input", (e) => { state.libSearch = e.target.value; renderLibSidebar(libraryGames()); });
  }
  list.querySelector(".lib-home-txt").textContent = t("Página principal");
  list.querySelector(".lib-home").classList.toggle("active", !state.libGame);
  $("#lib-search").placeholder = t("Buscar en tu biblioteca");

  const q = (state.libSearch || "").trim().toLowerCase();
  const shown = q ? games.filter((gm) => gm._search.includes(q)) : games;
  const favs = shown.filter((gm) => isFavorite(gm.id));
  const rest = shown.filter((gm) => !isFavorite(gm.id));
  const collapsed = (state.libCollapsed ||= {});
  const group = (key, title, items) => (items.length ? `
    <button class="lib-group" data-lib-group="${key}"><span>${collapsed[key] ? "+" : "−"}</span> ${title} <span class="muted">(${items.length})</span></button>
    ${collapsed[key] ? "" : items.map(libItemHtml).join("")}` : "");
  $("#lib-items").innerHTML = group("fav", t("FAVORITOS"), favs) + group("all", favs.length ? t("SIN CATEGORÍA") : t("JUEGOS"), rest)
    || `<div class="muted small" style="padding:12px 16px">${t("Sin resultados")}</div>`;
}

// "Hoy", "Esta semana", "agosto", "agosto de 2025"…
function whenLabel(ms) {
  if (!ms) return "";
  const d = new Date(ms), now = new Date();
  const days = (now - d) / 864e5;
  if (d.toDateString() === now.toDateString()) return t("Hoy");
  if (days < 7) return t("Esta semana");
  return d.toLocaleDateString(LOCALE(), d.getFullYear() === now.getFullYear() ? { month: "long" } : { month: "long", year: "numeric" });
}

// Carátula vertical; `attr` decide qué hace al pulsarla (abrir en la biblioteca o la ficha).
function capsuleHtml(gm, attr = `data-lib="${esc(gm.id)}"`) {
  const img = coverImg(gm, "cap-img");
  const n = gm.ports.length;
  return `<div class="cap ${img ? "has-img" : ""} ${installedPortOf(gm) || !isOwned(gm.id) ? "" : "not-installed"}" ${attr} style="${coverStyle(gm)}">
    ${img}<div class="cap-title">${esc(gm.title)}</div>
    ${isFavorite(gm.id) ? `<span class="cap-fav" title="${t("Favorito")}">★</span>` : ""}
    ${n > 1 ? `<span class="cap-badge" title="${t("{n} ports", { n })}">${n}</span>` : ""}
  </div>`;
}

// Tarjeta ancha con la imagen hero (o la portada) y un pie con información.
function wideHtml(gm, attr, foot) {
  const hero = heroSrc(gm.id);
  const cover = coverSrc(gm.id);
  return `<div class="wide" ${attr} style="${coverStyle(gm)}">
    ${hero ? `<img class="wide-img" src="${hero}" alt="" onerror="this.remove()">`
      : cover ? `<img class="wide-blur" src="${cover}" alt=""><img class="wide-cover" src="${cover}" alt="">` : ""}
    ${hero || cover ? "" : `<div class="wide-name">${esc(gm.title)}</div>`}
    <div class="wide-foot">${foot}</div>
  </div>`;
}

const shelfHead = (title, extra = "") =>
  `<div class="shelf-head"><h3>${title}</h3><span class="shelf-line"></span>${extra}</div>`;

// Estanterías que se pueden elegir (id -> nombre y juegos).
function shelfKinds(games) {
  const kinds = [
    ["favorites", t("Favoritos"), (gm) => isFavorite(gm.id)],
    ["installed", t("Instalados"), (gm) => !!installedPortOf(gm)],
    ["not-installed", t("Sin instalar"), (gm) => !installedPortOf(gm)],
    ["experimental", t("Ports experimentales"), (gm) => gm.ports.some((p) => p.experimental)],
    ["several", t("Con varios ports"), (gm) => gm.ports.length > 1],
  ];
  for (const c of [...new Set(games.map((gm) => gm.console))].sort()) kinds.push([`console:${c}`, consoleName(c), (gm) => gm.console === c]);
  return kinds;
}

const playedOf = (gameId) => state.homeProfile?.games?.find((g) => g.game === gameId);

function renderLibraryHome(games) {
  const box = $("#lib-detail");
  const kinds = shelfKinds(games);

  // Jugados recientemente: según el perfil (tiempo por juego), los de tu biblioteca.
  const recent = (state.homeProfile?.games || [])
    .filter((g) => g.lastPlayed && isOwned(g.game) && state.games.get(g.game))
    .sort((a, b) => b.lastPlayed - a.lastPlayed).slice(0, 10);
  const recentHtml = recent.length ? `
    ${shelfHead(t("Jugados recientemente"))}
    <div class="cap-row">${recent.map((g, i) => {
      const gm = state.games.get(g.game);
      const inst = installedPortOf(gm);
      const when = `<div class="cap-when">${esc(whenLabel(g.lastPlayed * 1000))}</div>`;
      if (i) return `<div>${when}${capsuleHtml(gm)}</div>`;
      return `<div>${when}${wideHtml(gm, `data-lib="${esc(gm.id)}"`, `
        ${inst ? `<button class="wide-play" data-play="${esc(inst.id)}" title="${t("Jugar")}">▶</button>` : ""}
        <div class="wide-info"><div>${t("Últimas dos semanas: {time}", { time: fmtHours(g.lastTwoWeeksSecs || 0) })}</div>
        <div>${t("Total: {time}", { time: fmtHours(g.playtimeSecs) })}</div></div>`)}</div>`;
    }).join("")}</div>` : "";

  // Jugar a continuación: lo que tienes en la biblioteca y aún no has probado; si no hay, lo más popular de Dársena.
  const unplayed = games.filter((gm) => !playedOf(gm.id))
    .sort((a, b) => (state.owned[b.id] || 0) - (state.owned[a.id] || 0)).slice(0, 8);
  const popular = state.gameList.filter((gm) => !isOwned(gm.id) && gm.downloads)
    .sort((a, b) => b.downloads - a.downloads).slice(0, 8);
  const next = unplayed.length ? unplayed : popular;
  const nextAttr = (gm) => (unplayed.length ? `data-lib="${esc(gm.id)}"` : `data-open="${esc(gm.id)}"`);
  const nextHtml = next.length ? `
    ${shelfHead(t("Jugar a continuación"))}
    <p class="muted small shelf-sub">${unplayed.length ? t("Juegos de tu biblioteca que aún no has probado") : t("Los más descargados de Dársena que aún no tienes")}</p>
    <div class="cap-row">${next.map((gm, i) => (i ? capsuleHtml(gm, nextAttr(gm)) : wideHtml(gm, nextAttr(gm), `
      <span class="wide-play ghost">⇩</span>
      <div class="wide-info"><b>${unplayed.length ? t("AÚN SIN PROBAR") : t("MÁS POPULARES")}</b>
      <div>${unplayed.length ? (installedPortOf(gm) ? t("Instalado y listo para jugar") : t("En tu biblioteca")) : t("entre la comunidad de Dársena")}</div></div>`))).join("")}</div>` : "";

  // Estanterías elegidas por el usuario.
  const shelves = state.settings?.shelves || [];
  const shelvesHtml = shelves.map((kind, i) => {
    const k = kinds.find((x) => x[0] === kind);
    const items = k ? games.filter(k[2]) : [];
    const select = `<select class="shelf-select" data-shelf-kind="${i}">
      <option value="">${t("Elegir una estantería")}</option>
      ${kinds.map(([id, name]) => `<option value="${esc(id)}" ${id === kind ? "selected" : ""}>${esc(name)}</option>`).join("")}
    </select>`;
    return `<div class="shelf">
      <div class="shelf-head">${select}${k ? `<span class="muted">(${items.length})</span>` : ""}<span class="shelf-line"></span>
        <button class="btn ghost small-btn" data-shelf-remove="${i}" title="${t("Quitar estantería")}">✕</button></div>
      ${!k ? `<p class="muted shelf-empty">${t("Elige qué quieres mostrar en esta estantería. ¡También puedes cambiarla o quitarla desde aquí!")}</p>`
        : items.length ? `<div class="cap-row">${items.map((gm) => capsuleHtml(gm)).join("")}</div>`
        : `<p class="muted shelf-empty">${t("No hay juegos en esta estantería.")}</p>`}
    </div>`;
  }).join("");

  // Todos los juegos, con orden.
  const sort = state.libAllSort || "alpha";
  const played = (gm) => playedOf(gm.id);
  const all = [...games].sort(
    sort === "recent" ? (a, b) => (played(b)?.lastPlayed || 0) - (played(a)?.lastPlayed || 0) || a.title.localeCompare(b.title, "es")
    : sort === "playtime" ? (a, b) => (played(b)?.playtimeSecs || 0) - (played(a)?.playtimeSecs || 0) || a.title.localeCompare(b.title, "es")
    : sort === "added" ? (a, b) => (state.owned[b.id] || 0) - (state.owned[a.id] || 0)
    : (a, b) => a.title.localeCompare(b.title, "es"));
  const sorts = [["alpha", t("Orden alfabético")], ["recent", t("Jugado recientemente")], ["playtime", t("Tiempo de juego")], ["added", t("Añadido a la biblioteca")]];
  const allHtml = `
    ${shelfHead(`${t("Todos los juegos")} <span class="muted">(${games.length})</span>`, `<span class="muted small">${t("ORDENAR POR")}</span>
      <select class="shelf-select" id="lib-all-sort">${sorts.map(([v, n]) => `<option value="${v}" ${v === sort ? "selected" : ""}>${n}</option>`).join("")}</select>`)}
    <div class="cap-grid">${all.map((gm) => capsuleHtml(gm)).join("")}</div>`;

  box.innerHTML = `<div class="lib-home-page">
    <section id="home-news">${newsHtml(games)}</section>
    ${recentHtml}
    ${nextHtml}
    <div id="lib-shelves">${shelvesHtml}</div>
    <div class="shelf-add"><button class="btn ghost" data-shelf-add>＋ ${t("Añadir estantería")}</button>
      <button class="btn ghost" data-nav="addgame:">＋ ${t("Añadir un juego que no está en Dársena")}</button></div>
    ${allHtml}
  </div>`;
  loadHomeProfile();
  loadNews(games);
}

// El perfil local (tiempo por juego) se pide al entrar; si cambió, se repinta la página principal.
async function loadHomeProfile() {
  if (state.homeProfileAt && Date.now() - state.homeProfileAt < 30000) return;
  state.homeProfileAt = Date.now();
  const p = await invoke("get_profile").catch(() => null);
  if (!p) return;
  const changed = JSON.stringify(p.games) !== JSON.stringify(state.homeProfile?.games);
  state.homeProfile = p;
  if (changed && state.view === "library") renderLibrary();
}

// ---------- Novedades: versiones publicadas de los ports de tu biblioteca ----------

// Port del que se miran las novedades: el instalado o, si no hay, el primero de GitHub.
const newsPortOf = (gm) => installedPortOf(gm)?.id ?? sortedPorts(gm).find((p) => !isExternal(p))?.id;

function newsHtml(games) {
  const items = games.map((gm) => {
    const id = newsPortOf(gm);
    const b = id && state.news?.get(id);
    return b ? { gm, id, b } : null;
  }).filter(Boolean).sort((x, y) => (y.b.publishedAt || "").localeCompare(x.b.publishedAt || "")).slice(0, 12);
  const loading = state.newsLoading && !items.length;
  if (!items.length && !loading) return "";
  return `${shelfHead(t("Novedades"))}
    ${loading ? `<p class="muted small">${t("Buscando versiones nuevas de tus ports…")}</p>` : `<div class="cap-row news-row">${items.map(({ gm, id, b }) => {
      const inst = state.library.find((x) => x.id === id);
      const update = inst && inst.version && inst.version !== b.version;
      const port = state.byId.get(id);
      const excerpt = mdExcerpt(b.notes);
      return `<div class="news" data-news="${esc(id)}">
        <div class="cap-when">${esc(whenLabel(Date.parse(b.publishedAt)))}</div>
        <div class="news-art">${wideHtml(gm, "", update ? `<span class="tag ok">${t("ACTUALIZACIÓN DISPONIBLE")}</span>` : "")}
          <div class="news-hover"><div class="news-kind">${t("NUEVA VERSIÓN")}</div>
            <div class="news-excerpt">${excerpt ? esc(excerpt) : t("El autor no escribió notas para esta versión.")}</div></div></div>
        <div class="news-title">${esc(port.port)} ${esc(b.version)}${b.prerelease ? ` <span class="tag">pre-release</span>` : ""}</div>
        <div class="news-game"><span class="news-chip">${esc(gm.title)}</span></div>
      </div>`;
    }).join("")}</div>`}`;
}

// Pide la última versión de cada port (GitHub las guarda en caché 15 min) y repinta solo las novedades.
async function loadNews(games) {
  state.news ||= new Map();
  const todo = games.map(newsPortOf).filter((id) => id && !state.news.has(id)).slice(0, 24);
  if (!todo.length || state.newsLoading) return;
  state.newsLoading = true;
  const paint = () => { const el = $("#home-news"); if (el && state.view === "library" && !state.libGame) el.innerHTML = newsHtml(libraryGames()); };
  paint();
  for (let i = 0; i < todo.length; i += 6) {
    await Promise.all(todo.slice(i, i + 6).map(async (id) => {
      state.news.set(id, await invoke("get_official_build", { id }).catch(() => null));
    }));
  }
  state.newsLoading = false;
  paint();
}

// Novedades en el orden en que se muestran (para pasar de una a otra en la ventana).
function newsList() {
  return libraryGames().map((gm) => {
    const id = newsPortOf(gm);
    const b = id && state.news?.get(id);
    return b ? { gm, id, b } : null;
  }).filter(Boolean).sort((x, y) => (y.b.publishedAt || "").localeCompare(x.b.publishedAt || "")).slice(0, 12);
}

// Texto plano de las notas (para el resumen al pasar el cursor).
function mdExcerpt(md, max = 170) {
  if (!md) return "";
  const text = md.replace(/<!--[\s\S]*?-->/g, "").replace(/```[\s\S]*?```/g, " ").replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1").replace(/^\s*(#{1,6}|[-*+]|\d+[.)]|>)\s+/gm, "").replace(/[*_`~|]+/g, "")
    .replace(/\s+/g, " ").trim();
  return text.length > max ? `${text.slice(0, max).replace(/\s\S*$/, "")}…` : text;
}

// Markdown de GitHub -> HTML seguro: primero se escapa todo y luego se aplican las marcas más comunes.
function mdToHtml(md) {
  const inline = (s) => esc(s)
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, (m, txt, url) => `<a data-url="${url}">${txt}</a>`)
    .replace(/(^|[\s(])(https?:\/\/[^\s<)]+)/g, (m, pre, url) => `${pre}<a data-url="${url}">${url}</a>`)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>").replace(/__([^_]+)__/g, "<b>$1</b>")
    .replace(/(^|\W)\*([^*\s][^*]*)\*/g, "$1<i>$2</i>");
  const out = [];
  let list = null, code = null;
  const close = () => { if (list) { out.push(`</${list}>`); list = null; } };
  for (const raw of md.replace(/<!--[\s\S]*?-->/g, "").replace(/\r/g, "").split("\n")) {
    if (code !== null) {
      if (/^\s*```/.test(raw)) { out.push(`<pre>${esc(code)}</pre>`); code = null; } else code += `${raw}\n`;
      continue;
    }
    if (/^\s*```/.test(raw)) { close(); code = ""; continue; }
    const line = raw.trimEnd();
    let m;
    if ((m = line.match(/^(#{1,6})\s+(.*)/))) { close(); out.push(`<h${Math.min(m[1].length + 2, 6)}>${inline(m[2])}</h${Math.min(m[1].length + 2, 6)}>`); }
    else if ((m = line.match(/^\s*[-*+]\s+(.*)/))) { if (list !== "ul") { close(); out.push("<ul>"); list = "ul"; } out.push(`<li>${inline(m[1])}</li>`); }
    else if ((m = line.match(/^\s*\d+[.)]\s+(.*)/))) { if (list !== "ol") { close(); out.push("<ol>"); list = "ol"; } out.push(`<li>${inline(m[1])}</li>`); }
    else if (/^\s*(---|\*\*\*)\s*$/.test(line)) { close(); out.push("<hr>"); }
    else if (!line.trim()) close();
    else { close(); out.push(`<p>${inline(line.replace(/^>\s?/, ""))}</p>`); }
  }
  close();
  if (code !== null) out.push(`<pre>${esc(code)}</pre>`);
  return out.join("");
}

// Ventana con las notas de una versión, como las noticias de Steam.
function openNews(portId) {
  const list = newsList();
  const k = list.findIndex((x) => x.id === portId);
  if (k < 0) return;
  const { gm, id, b } = list[k];
  const port = state.byId.get(id);
  const hero = heroSrc(gm.id) || coverSrc(gm.id);
  const date = b.publishedAt ? new Date(b.publishedAt).toLocaleDateString(LOCALE(), { weekday: "short", day: "numeric", month: "long", year: "numeric" }) : "";
  $("#news-modal")?.remove();
  const box = document.createElement("div");
  box.id = "news-modal";
  box.className = "news-modal";
  box.innerHTML = `<div class="news-dialog">
      <div class="news-banner" style="${coverStyle(gm)}">${hero ? `<img src="${hero}" alt="">` : ""}</div>
      <div class="news-head">
        <div class="news-meta"><span class="news-kind">${t("NUEVA VERSIÓN")}</span> ${t("SE PUBLICÓ EL")} ${esc(date)}</div>
        <h2>${esc(b.name || `${port.port} ${b.version}`)}</h2>
        <div class="muted small">${esc(gm.title)} · ${esc(port.port)} ${esc(b.version)}${b.prerelease ? " · pre-release" : ""}</div>
      </div>
      <div class="news-body">${b.notes ? mdToHtml(b.notes) : `<p class="muted">${t("El autor no escribió notas para esta versión.")}</p>`}</div>
      <div class="news-foot">
        <button class="btn primary" data-lib="${esc(gm.id)}">${icon("library")}${t("Ver en la biblioteca")}</button>
        ${b.page ? `<button class="btn ghost" data-url="${esc(b.page)}">${icon("github")}${t("Ver en GitHub")}</button>` : ""}
      </div>
    </div>
    <div class="news-side">
      <button class="news-round" data-news-close title="${t("Cerrar")}">${icon("close")}</button>
      <button class="news-sq" ${k > 0 ? `data-news-step="${esc(list[k - 1].id)}"` : "disabled"} title="${t("Anterior")}">▲</button>
      <button class="news-sq" ${k < list.length - 1 ? `data-news-step="${esc(list[k + 1].id)}"` : "disabled"} title="${t("Siguiente")}">▼</button>
    </div>`;
  document.body.appendChild(box);
  box.addEventListener("click", (e) => {
    if (e.target === box || e.target.closest("[data-news-close]")) box.remove();
    else if (e.target.closest("[data-lib]")) box.remove();
  });
}
document.addEventListener("keydown", (e) => { if (e.key === "Escape") $("#news-modal")?.remove(); });

async function toggleFavorite(gameId) {
  const value = !isFavorite(gameId);
  await call("set_favorite", { game: gameId, value });
  const favs = (state.settings.favorites || []).filter((g) => g !== gameId);
  state.settings.favorites = value ? [...favs, gameId] : favs;
  toast(value ? tr("Añadido a favoritos") : tr("Quitado de favoritos"), "ok");
  render();
}

async function saveShelves(shelves) {
  state.settings.shelves = shelves;
  await call("set_shelves", { shelves });
  renderLibrary();
}

document.addEventListener("change", (e) => {
  const el = e.target;
  if (el.matches("[data-shelf-kind]")) {
    const shelves = [...(state.settings.shelves || [])];
    shelves[Number(el.dataset.shelfKind)] = el.value;
    saveShelves(shelves);
  } else if (el.id === "lib-all-sort") {
    state.libAllSort = el.value;
    renderLibrary();
  }
});

// ---------- Descargas ----------

const STATUS_TEXT = {
  starting: "Conectando…", downloading: "Descargando", paused: "En pausa",
  extracting: "Extrayendo e instalando…", completed: "Completado", error: "Error",
};
// Lo que Rust describe en español (la vía de descarga, el prefijo de los datos) se traduce aquí.
const viaText = (v) => (LANG() === "en" ? v.replace(/(\d+) conexiones/, "$1 connections") : v);
const taskTitle = (s) => (LANG() === "en" ? s.replace(/^Datos · /, "Data · ") : s);

function renderBadge() {
  const active = state.downloads.filter((d) => !["completed", "error"].includes(d.status)).length;
  const badge = $("#dl-badge");
  if (!badge) return;
  badge.textContent = active;
  badge.classList.toggle("hidden", !active);
}

// Velocidad total de descarga, una muestra por segundo (los últimos 3 minutos), y el máximo de la sesión.
const DL_SAMPLES = 180;
const dlStats = { samples: [], peak: 0, sharing: null };
setInterval(() => {
  const bps = state.downloads.filter((d) => d.status === "downloading").reduce((a, d) => a + (d.speedBps || 0), 0);
  dlStats.samples.push(bps);
  if (dlStats.samples.length > DL_SAMPLES) dlStats.samples.shift();
  dlStats.peak = Math.max(dlStats.peak, bps);
  if (state.view === "downloads") renderDlHeaderStats();
}, 1000);
// Lo que se comparte (siembra) se mira cada pocos segundos mientras la página está abierta.
setInterval(async () => {
  if (state.view !== "downloads") return;
  dlStats.sharing = await invoke("get_sharing_stats").catch(() => null);
  renderDlHeaderStats();
}, 4000);

const fmtRate = (bps) => `${fmtBytes(bps) === "—" ? "0 B" : fmtBytes(bps)}/s`;
const isActive = (d) => ["starting", "downloading", "extracting"].includes(d.status);
const gameOfTask = (d) => state.games.get(gameOfPort(d.key.replace(/^data:/, "")));

function dlInfo(d) {
  const pct = d.total ? (d.downloaded / d.total) * 100 : 0;
  return [
    `${t(STATUS_TEXT[d.status])} · ${esc(viaText(d.via || d.kind.toUpperCase()))}`,
    d.total ? `${fmtBytes(d.downloaded)} / ${fmtBytes(d.total)} (${pct.toFixed(1)}%)` : "",
    d.status === "downloading" && d.speedBps ? fmtRate(d.speedBps) : "",
    d.etaSecs && d.status === "downloading" ? t("quedan {time}", { time: fmtDuration(d.etaSecs) }) : "",
  ].filter(Boolean).join(" · ");
}

function dlActions(d) {
  const k = esc(d.key);
  const isData = d.key.startsWith("data:");
  if (d.status === "completed") return `${isData ? "" : `<button class="btn primary" data-play="${k}">${icon("play")}${t("Jugar")}</button>`}<button class="btn ghost" data-cancel="${k}" title="${t("Quitar de la lista")}">${icon("close")}</button>`;
  if (d.status === "error") return `<button class="btn ghost" data-cancel="${k}" title="${t("Quitar de la lista")}">${icon("close")}</button>`;
  return `${d.status === "paused" ? `<button class="btn" data-resume="${k}" title="${t("Reanudar")}">${icon("play")}</button>`
      : `<button class="btn" data-pause="${k}" title="${t("Pausar")}" ${d.status !== "downloading" ? "disabled" : ""}>${icon("pause")}</button>`}
    <button class="btn danger" data-cancel="${k}" title="${t("Cancelar")}" ${d.status === "extracting" ? "disabled" : ""}>${icon("close")}</button>`;
}

const barClass = (d) => (d.status === "completed" ? "done" : d.status === "paused" ? "paused" : d.status === "error" ? "failed"
  : (d.status === "starting" || d.status === "extracting" || !d.total) ? "indeterminate" : "");

function dlRowHtml(d) {
  const gm = gameOfTask(d);
  const pct = d.total ? (d.downloaded / d.total) * 100 : 0;
  const art = gm ? (heroSrc(gm.id) || coverSrc(gm.id)) : "";
  return `<div class="dl-row ${d.status}">
    <div class="dl-thumb" style="${gm ? coverStyle(gm) : ""}" ${gm ? `data-lib="${esc(gm.id)}"` : ""}>${art ? `<img src="${art}" alt="" onerror="this.remove()">` : ""}</div>
    <div class="dl-body">
      <div class="dl-title">${esc(taskTitle(d.title))}</div>
      <div class="bar ${barClass(d)}"><div style="width:${d.status === "completed" ? 100 : pct}%"></div></div>
      <div class="dl-info">${dlInfo(d)}</div>
      ${d.error ? `<div class="err">${esc(d.error)}</div>` : ""}
    </div>
    <div class="dl-actions">${dlActions(d)}</div>
  </div>`;
}

function renderDownloads() {
  renderBadge();
  if (state.view !== "downloads") return;
  const list = $("#dl-list");
  const all = state.downloads;
  const active = all.filter(isActive);
  const current = active.find((d) => d.status === "downloading") || active[0] || all.find((d) => d.status === "paused");
  const paused = all.filter((d) => d.status === "paused" && d !== current);
  const others = active.filter((d) => d !== current);
  const done = all.filter((d) => d.status === "completed");
  const failed = all.filter((d) => d.status === "error");

  const gm = current && gameOfTask(current);
  const hero = gm ? heroSrc(gm.id) || coverSrc(gm.id) : "";
  const pct = current?.total ? (current.downloaded / current.total) * 100 : 0;
  const head = `<div class="dl-head">
    ${hero ? `<img class="dl-head-art" src="${hero}" alt="" onerror="this.remove()">` : ""}
    <canvas id="dl-graph"></canvas>
    <div class="dl-head-main">${current ? `
      <div class="dl-head-title" ${gm ? `data-lib="${esc(gm.id)}"` : ""}>${esc(taskTitle(current.title))}</div>
      <div class="dl-head-status">${dlInfo(current)}</div>
      <div class="bar ${barClass(current)}"><div style="width:${pct}%"></div></div>
      <div class="dl-actions">${dlActions(current)}</div>`
      : `<div class="dl-idle"><img class="empty-mark" src="logo.svg" alt=""><div class="dl-head-title muted">${t("No hay ninguna descarga en curso")}</div></div>`}
    </div>
    <div class="dl-stats" id="dl-stats"></div>
  </div>`;

  const section = (title, items, empty) => `
    ${shelfHead(`${title} <span class="muted">(${items.length})</span>`)}
    ${items.length ? items.map(dlRowHtml).join("") : empty ? `<p class="muted dl-empty">${empty}</p>` : ""}`;

  list.innerHTML = head + `<div class="dl-sections">
    ${section(t("A continuación"), [...others, ...paused], t("No hay más descargas en cola"))}
    ${failed.length ? section(t("Con errores"), failed) : ""}
    ${done.length ? section(t("Completadas"), done) : ""}
  </div>`;
  renderDlHeaderStats();
}

// Solo los números y la gráfica: se actualizan cada segundo sin repintar la página.
function renderDlHeaderStats() {
  const box = $("#dl-stats");
  if (!box) return;
  const now = dlStats.samples.at(-1) || 0;
  const total = state.downloads.reduce((a, d) => a + (isActive(d) || d.status === "paused" ? d.downloaded : 0), 0);
  const s = dlStats.sharing;
  box.innerHTML = `
    <div><span class="k net">${t("RED")}</span><b>${fmtRate(now)}</b></div>
    <div><span class="k peak">${t("MÁXIMO")}</span><b>${fmtRate(dlStats.peak)}</b></div>
    <div><span class="k got">${t("EN CURSO")}</span><b>${fmtBytes(total) === "—" ? "0 B" : fmtBytes(total)}</b></div>
    ${s?.ports ? `<div><span class="k up">${t("COMPARTIENDO")}</span><b>${t(s.ports === 1 ? "{n} port" : "{n} ports", { n: s.ports })} · ${fmtBytes(s.uploadedBytes)}</b></div>` : ""}`;
  drawDlGraph();
}

function drawDlGraph() {
  const c = $("#dl-graph");
  if (!c) return;
  const w = c.clientWidth, h = c.clientHeight, dpr = window.devicePixelRatio || 1;
  if (!w || !h) return;
  if (c.width !== w * dpr) { c.width = w * dpr; c.height = h * dpr; }
  const g = c.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  const data = dlStats.samples;
  const max = Math.max(dlStats.peak, 1);
  const x = (i) => w - (data.length - 1 - i) * (w / (DL_SAMPLES - 1));
  const y = (v) => h - 6 - (v / max) * (h * 0.7);
  if (data.length < 2) return;
  const accent = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || "#ffc20e";
  g.beginPath();
  g.moveTo(x(0), h);
  data.forEach((v, i) => g.lineTo(x(i), y(v)));
  g.lineTo(x(data.length - 1), h);
  g.closePath();
  const grad = g.createLinearGradient(0, 0, 0, h);
  grad.addColorStop(0, "rgba(255,194,14,.35)");
  grad.addColorStop(1, "rgba(255,194,14,0)");
  g.fillStyle = grad;
  g.fill();
  g.beginPath();
  data.forEach((v, i) => (i ? g.lineTo(x(i), y(v)) : g.moveTo(x(i), y(v))));
  g.strokeStyle = accent;
  g.lineWidth = 1.5;
  g.stroke();
}
window.addEventListener("resize", () => { if (state.view === "downloads") drawDlGraph(); });

function renderMini() {
  const el = $("#dl-bar");
  if (!el) return;
  const active = state.downloads.filter((d) => ["downloading", "starting", "extracting"].includes(d.status));
  if (!active.length) {
    const paused = state.downloads.filter((d) => d.status === "paused").length;
    el.classList.remove("busy");
    el.innerHTML = `${icon("downloads")}<span>${paused ? t("Descargas en pausa: {n}", { n: paused }) : t("Gestionar descargas")}</span>`;
    return;
  }
  const down = active.reduce((a, d) => a + d.downloaded, 0);
  const total = active.reduce((a, d) => a + d.total, 0);
  const speed = active.reduce((a, d) => a + (d.speedBps || 0), 0);
  const first = active[0];
  const label = active.length > 1 ? t("{n} descargas", { n: active.length }) : taskTitle(first.title);
  el.classList.add("busy");
  el.innerHTML = `${icon("downloads")}<span class="dl-bar-text"><b>${esc(label)}</b> · ${total ? `${Math.floor((down / total) * 100)}%` : t(STATUS_TEXT[first.status])} · ${fmtRate(speed)}</span>
    <span class="bar"><span style="width:${total ? (down / total) * 100 : 0}%"></span></span>`;
}

// ---------- Fuentes ----------

function renderSources() {
  const list = $("#source-list");
  const sources = state.settings?.sources || [];
  if (!sources.length) {
    list.innerHTML = emptyHtml(t("Sin fuentes"), t("Pega arriba la URL de una fuente JSON."));
    return;
  }
  list.innerHTML = sources.map((s) => `
    <div class="src-item">
      <input type="checkbox" data-toggle="${esc(s.url)}" ${s.enabled ? "checked" : ""} title="${t("Activar/desactivar")}">
      <div>
        <div class="name">${esc(s.name)} <span class="muted small">· ${t("{n} enlaces", { n: s.matched })}${s.unmatched ? ` (${t("{n} sin juego en el catálogo", { n: s.unmatched })})` : ""} · ${t("actualizada {date}", { date: fmtDate(s.lastFetch) })}</span></div>
        <div class="url">${esc(s.url)}</div>
        ${s.error ? `<div class="err">${t("Último error:")} ${esc(s.error)}</div>` : ""}
      </div>
      <button class="btn danger" data-remove-source="${esc(s.url)}">${t("Eliminar")}</button>
    </div>`).join("");
}

$("#source-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const input = $("#source-url");
  const btn = e.submitter;
  btn.disabled = true;
  try {
    const entry = await call("add_source", { url: input.value });
    toast(t("Fuente \"{name}\" añadida: {n} enlaces", { name: entry.name, n: entry.matched }), "ok");
    input.value = "";
    await reload();
  } catch {} finally { btn.disabled = false; }
});

$("#refresh-sources").addEventListener("click", async (e) => {
  e.target.disabled = true;
  try {
    await call("refresh_sources");
    await reload();
    toast(t("Fuentes actualizadas"), "ok");
  } catch {} finally { e.target.disabled = false; }
});

// ---------- Ajustes ----------

// Abre Ajustes en una sección: general, library, downloads, sources o advanced.
function openSettings(tab) {
  if (tab) state.setTab = tab;
  show("settings");
  $("#app").scrollTop = 0;
}

function renderSettings() {
  const tab = state.setTab || "general";
  document.querySelectorAll("[data-set-tab]").forEach((b) => b.classList.toggle("active", b.dataset.setTab === tab));
  document.querySelectorAll(".settings-pane").forEach((p) => p.classList.toggle("active", p.dataset.pane === tab));
  if (tab === "sources") renderSources();
  $("#language").value = state.settings.language || "";
  $("#close-to-tray").checked = state.settings.closeToTray !== false;
  $("#discord-presence").checked = state.settings.discordPresence !== false;
  $("#show-nsfw").checked = !!state.settings.showNsfw;
  $("#cloud-saves").checked = state.settings.cloudSaves !== false;
  $("#auto-update").checked = state.settings.autoUpdatePorts !== false;
  $("#install-dir").value = state.settings.installDir;
  renderDrives();
  $("#share").checked = state.settings.shareDownloads;
  $("#upload-limit").value = String(state.settings.uploadLimitKbps ?? 1024);
  renderSharingInfo();
  $("#gh-token").value = state.settings.githubToken || "";
  $("#covers-origin").value = state.settings.coversOrigin || "";
}

// Discos del equipo, como en Steam: en cuál está tu biblioteca y cuánto espacio queda.
async function renderDrives() {
  const drives = await invoke("list_drives").catch(() => []);
  const box = $("#drives");
  if (!box || state.view !== "settings") return;
  const current = (state.settings.installDir || "").toUpperCase();
  box.innerHTML = drives.map((d) => {
    const used = d.total ? ((d.total - d.free) / d.total) * 100 : 0;
    const here = current.startsWith(d.root.toUpperCase());
    return `<div class="drive ${here ? "on" : ""}">
      <div class="drive-ico">${icon("disk")}</div>
      <div class="drive-body">
        <div class="drive-name"><b>${esc(d.root.slice(0, 2))}</b> ${esc(d.label || t("Disco local"))}${here ? ` <span class="tag ok">${t("EN USO")}</span>` : ""}</div>
        <div class="bar ${used > 90 ? "full" : ""}"><div style="width:${used}%"></div></div>
        <div class="muted small">${t("{free} libres de {total}", { free: fmtBytes(d.free), total: fmtBytes(d.total) })}</div>
      </div>
      ${here ? "" : `<button class="btn" data-drive="${esc(d.root)}">${t("Usar este disco")}</button>`}
    </div>`;
  }).join("");
}

$("#drives").addEventListener("click", async (e) => {
  const b = e.target.closest("[data-drive]");
  if (!b) return;
  b.disabled = true;
  try {
    state.settings.installDir = await call("use_library_drive", { root: b.dataset.drive });
    toast(t("Los juegos nuevos se instalarán en {path}", { path: state.settings.installDir }), "ok");
    renderSettings();
  } catch { b.disabled = false; }
});

$("#auto-update").addEventListener("change", async (e) => {
  try {
    await call("set_auto_update_ports", { value: e.target.checked });
    state.settings.autoUpdatePorts = e.target.checked;
  } catch { e.target.checked = !e.target.checked; }
});

$("#discord-presence").addEventListener("change", async (e) => {
  try {
    await call("set_discord_presence", { value: e.target.checked });
    state.settings.discordPresence = e.target.checked;
  } catch { e.target.checked = !e.target.checked; }
});

$("#close-to-tray").addEventListener("change", async (e) => {
  try {
    await call("set_close_to_tray", { value: e.target.checked });
    state.settings.closeToTray = e.target.checked;
  } catch { e.target.checked = !e.target.checked; }
});

$("#language").addEventListener("change", async (e) => {
  const preference = e.target.value || null;
  await applyLanguage(preference);
  state.settings.language = preference;
  fillConsoleFilter();
  renderAccount();
  render();
});

$("#save-covers").addEventListener("click", async () => {
  try {
    await call("set_covers_origin", { origin: $("#covers-origin").value });
    state.settings.coversOrigin = $("#covers-origin").value.trim() || null;
    toast(t("Origen de portadas guardado (se aplica a las portadas que no estén en caché)"), "ok");
  } catch {}
});

$("#change-dir").addEventListener("click", async () => {
  const dir = await openDialog({ directory: true, defaultPath: state.settings.installDir });
  if (!dir) return;
  try {
    await call("set_install_dir", { path: dir });
    state.settings.installDir = dir;
    renderSettings();
  } catch {}
});

$("#save-token").addEventListener("click", async () => {
  try {
    await call("set_github_token", { token: $("#gh-token").value });
    state.settings.githubToken = $("#gh-token").value.trim() || null;
    toast(t("Token guardado"), "ok");
  } catch {}
});

async function renderSharingInfo() {
  const s = await invoke("get_sharing_stats").catch(() => null);
  if (!s || state.view !== "settings") return;
  $("#share-info").textContent = !state.settings.shareDownloads ? t("Ahora no compartes nada.")
    : s.ports ? [t(s.ports === 1 ? "Compartiendo {n} port" : "Compartiendo {n} ports", { n: s.ports }),
        t("{size} subidos en esta sesión", { size: fmtBytes(s.uploadedBytes) }),
        t(s.peers === 1 ? "{n} par conectado" : "{n} pares conectados", { n: s.peers })].join(" · ")
    : t("Todavía no compartes ningún port: se empieza a compartir al descargar uno por torrent.");
}

$("#share").addEventListener("change", async (e) => {
  const value = e.target.checked;
  if (!value && !confirm(t("¿Dejar de compartir? Se borrarán las copias que se estaban compartiendo; los juegos instalados no se tocan."))) {
    e.target.checked = true;
    return;
  }
  try {
    await call("set_share_downloads", { value });
    state.settings.shareDownloads = value;
    state.library = await invoke("get_library");
  } catch { e.target.checked = !value; }
  renderSharingInfo();
});

$("#upload-limit").addEventListener("change", async (e) => {
  try {
    await call("set_upload_limit", { kbps: Number(e.target.value) });
    state.settings.uploadLimitKbps = Number(e.target.value);
    toast(t("Límite de subida guardado"), "ok");
  } catch {}
});

// Mientras Ajustes está abierto, el resumen de lo compartido se actualiza solo.
setInterval(() => { if (state.view === "settings") renderSharingInfo(); }, 3000);

// ---------- Eventos globales (delegación) ----------

document.addEventListener("click", async (e) => {
  const t = e.target.closest("[data-view],[data-goto],[data-open],[data-install-official],[data-install-source],[data-install-first],[data-play],[data-lib],[data-folder],[data-pick-exe],[data-uninstall],[data-pause],[data-resume],[data-cancel],[data-remove-source],[data-toggle],[data-url],[data-port],[data-install-data],[data-data-folder],[data-add],[data-goto-lib],[data-remove-lib],[data-car],[data-login],[data-account],[data-logout],[data-star],[data-review-submit],[data-review-delete],[data-profile],[data-profile-sync],[data-trailer],[data-trailer-play],[data-lib-home],[data-lib-group],[data-shelf-add],[data-shelf-remove],[data-fav],[data-locate],[data-shortcut],[data-news],[data-news-step]");
  if (!t?.matches("[data-account]")) toggleAccountMenu(false);
  closeCtxMenu();
  if (!t || t.disabled) return;
  const d = t.dataset;
  try {
    if (d.view) show(d.view);
    else if (d.goto) show(d.goto);
    else if (d.open) { state.detailGame = d.open; show("detail"); }
    else if (d.port) { state.libPort = d.port; renderLibrary(); }
    else if (t.matches("[data-lib-home]")) { state.libGame = null; state.libPort = null; renderLibrary(); }
    else if (d.libGroup) { state.libCollapsed[d.libGroup] = !state.libCollapsed[d.libGroup]; renderLibSidebar(libraryGames()); }
    else if (t.matches("[data-shelf-add]")) await saveShelves([...(state.settings.shelves || []), ""]);
    else if (d.shelfRemove) await saveShelves((state.settings.shelves || []).filter((_, i) => i !== Number(d.shelfRemove)));
    else if (d.fav) await toggleFavorite(d.fav);
    else if (d.news) openNews(d.news);
    else if (d.newsStep) openNews(d.newsStep);
    else if (d.shortcut) {
      const where = await call("create_shortcut", { id: d.shortcut });
      toast(tr("Acceso directo creado en el escritorio: {path}", { path: where.split("\\").pop() }), "ok");
    }
    else if (d.locate) {
      const exe = await openDialog({ filters: [{ name: tr("Ejecutable"), extensions: ["exe", "bat", "cmd"] }] });
      if (exe) {
        await call("locate_game", { id: d.locate, exe });
        toast(tr("Juego vinculado: ya puedes jugarlo"), "ok");
        await reload();
      }
    }
    else if (d.dataFolder) await call("open_data_folder", { id: d.dataFolder });
    else if (d.installData) {
      const box = [...document.querySelectorAll("[data-extract]")].find((x) => x.dataset.extract === `${d.src}|${d.idx}`);
      await call("install_data", { id: d.installData, sourceUrl: d.src, index: Number(d.idx), extract: box ? box.checked : null });
      toast(tr("Descargando datos del juego…"), "ok");
      state.downloads = await invoke("list_downloads");
      renderLibrary();
    }
    else if (d.car) {
      const i = d.car === "prev" ? carousel.index - 1 : d.car === "next" ? carousel.index + 1 : Number(d.car);
      showSlide(i);
    }
    else if (d.add) {
      await call("add_to_library", { game: d.add });
      state.owned = await invoke("get_owned");
      toast(tr("Añadido a tu biblioteca"), "ok");
      if (state.view === "detail") renderDetail();
      else if (state.view === "store") showSlide(carousel.index);
      else render();
    }
    else if (d.gotoLib) { state.libGame = d.gotoLib; state.libPort = null; show("library"); }
    else if (d.removeLib) {
      const gm = state.games.get(d.removeLib);
      if (confirm(tr("¿Quitar {title} de tu biblioteca?", { title: gm?.title }))) {
        await call("remove_from_library", { game: d.removeLib });
        state.owned = await invoke("get_owned");
        renderLibrary();
      }
    }
    else if (d.installOfficial) openInstall(d.installOfficial, null);
    else if (d.installFirst) {
      const [o] = await call("get_source_options", { id: d.installFirst });
      if (o) openInstall(d.installFirst, { sourceUrl: o.sourceUrl, index: o.index });
      else toast(tr("Ninguna de tus fuentes trae este port"), "error");
    }
    else if (d.installSource) openInstall(d.installSource, { sourceUrl: d.src, index: Number(d.idx) });
    else if (d.play) await playWithCloud(d.play);
    else if (d.lib) { state.libGame = d.lib; state.libPort = null; renderLibrary(); }
    else if (d.folder) await call("open_game_folder", { id: d.folder });
    else if (d.url) await call("open_url", { url: d.url });
    else if (d.pickExe) {
      const g = state.library.find((x) => x.id === d.pickExe);
      const exe = await openDialog({ defaultPath: g.path, filters: [{ name: tr("Ejecutable"), extensions: ["exe", "bat", "cmd"] }] });
      if (exe) {
        await call("set_game_exe", { id: g.id, exe });
        state.library = await invoke("get_library");
        renderLibrary();
      }
    }
    else if (d.uninstall) {
      const g = state.library.find((x) => x.id === d.uninstall);
      const ask = g?.linked ? tr("¿Quitar {title} de Dársena? Tus archivos no se borran.", { title: g?.title })
        : tr("¿Desinstalar {title}? Se borrará su carpeta:", { title: g?.title }) + `\n${g?.path}`;
      if (confirm(ask)) {
        await call("uninstall_game", { id: d.uninstall });
        toast(tr("Juego desinstalado"), "ok");
        await reload();
      }
    }
    else if (d.trailerPlay) $("#trailer").innerHTML = trailerPlayer(d.trailerPlay);
    else if (d.trailer) {
      const [gameId, idx] = d.trailer.split(":");
      const list = (state.info?.games?.[gameId]?.trailers || []).filter((x) => x.kind === "youtube");
      if (list[Number(idx)]) $("#trailer").innerHTML = trailerPlayer(list[Number(idx)].id);
      document.querySelectorAll(".trailer-list button").forEach((b) => b.classList.toggle("on", b === t));
    }
    else if (t.matches("[data-profile]")) openProfile(d.profile || null);
    else if (t.matches("[data-profile-sync]")) {
      t.disabled = true;
      t.textContent = tr("Sincronizando…");
      try { state.profile = await call("sync_profile"); toast(tr("Perfil sincronizado con tu Gist"), "ok"); } catch {}
      renderProfile();
    }
    else if (t.matches("[data-login]")) login();
    else if (t.matches("[data-account]")) toggleAccountMenu();
    else if (t.matches("[data-logout]")) {
      await call("auth_logout");
      state.user = null;
      renderAccount();
      toast(tr("Sesión cerrada"), "ok");
      if (state.view === "detail") loadReviews(state.detailGame);
    }
    else if (d.star) {
      state.myRating = Number(d.star);
      document.querySelectorAll(".star-pick button").forEach((b) => b.classList.toggle("on", Number(b.dataset.star) <= state.myRating));
    }
    else if (d.reviewSubmit) {
      if (!state.myRating) { toast(tr("Elige de 1 a 5 estrellas"), "error"); return; }
      const text = $("#review-text").value;
      t.disabled = true;
      t.textContent = tr("Publicando…");
      try {
        setReviews(d.reviewSubmit, await call("submit_review", { game: d.reviewSubmit, rating: state.myRating, text }));
        toast(tr("Reseña publicada"), "ok");
      } catch { t.disabled = false; t.textContent = tr("Reintentar"); }
    }
    else if (d.reviewDelete) {
      if (confirm(tr("¿Borrar tu reseña?"))) {
        setReviews(d.reviewDelete, await call("delete_review", { game: d.reviewDelete }));
        toast(tr("Reseña borrada"), "ok");
      }
    }
    else if (d.pause) await call("pause_download", { id: d.pause });
    else if (d.resume) await call("resume_download", { id: d.resume });
    else if (d.cancel) await call("cancel_download", { id: d.cancel });
    else if (d.removeSource) {
      if (confirm(tr("¿Eliminar esta fuente? Sus enlaces desaparecerán (los juegos instalados se conservan)."))) {
        await call("remove_source", { url: d.removeSource });
        await reload();
      }
    }
    else if (d.toggle) {
      await call("toggle_source", { url: d.toggle, enabled: t.checked });
      await reload();
    }
    if (d.pause || d.resume || d.cancel) {
      state.downloads = await invoke("list_downloads");
      render();
    }
  } catch {}
});

$("#search").addEventListener("input", renderStoreDebounced);
$("#console-filter").addEventListener("change", renderStore);
$("#sort").addEventListener("change", renderStore);
$("#only-links").addEventListener("change", renderStore);
$("#no-ai").addEventListener("change", (e) => {
  try { localStorage.setItem("darsena.noAi", e.target.checked ? "1" : ""); } catch {}
  renderStore();
});
try { $("#no-ai").checked = localStorage.getItem("darsena.noAi") === "1"; } catch {}

listen("downloads", (ev) => {
  state.downloads = ev.payload;
  renderDownloads();
  renderMini();
  if (state.view === "library") {
    const bar = $("#lib-action");
    if (bar && state.libPort) bar.firstElementChild.outerHTML = actionButtonHtml(state.libPort);
  }
});

listen("library-changed", async () => {
  [state.library, state.owned] = await Promise.all([invoke("get_library"), invoke("get_owned")]);
  render();
});

listen("achievements", (ev) => {
  for (const a of ev.payload) toast(`${a.icon} ${t("Logro desbloqueado:")} ${a.name}`, "ok achievement");
});

listen("profile-changed", async () => {
  if (state.view === "profile" && !state.profileOf) {
    state.profile = await invoke("get_profile").catch(() => state.profile);
    renderProfile();
  }
});

listen("game-state", (ev) => {
  const [id, running] = ev.payload;
  running ? state.running.add(id) : state.running.delete(id);
  if (!running) uploadSavesAfterPlaying(id);
  if (state.view === "library" || state.view === "detail") render();
});

async function reload() {
  const [settings, sourceCounts, library, downloads, running, owned] = await Promise.all([
    invoke("get_settings"), invoke("get_source_counts"), invoke("get_library"),
    invoke("list_downloads"), invoke("get_running"), invoke("get_owned"),
  ]);
  Object.assign(state, { settings, sourceCounts, library, downloads, running: new Set(running), owned });
  render();
}

// Carga (o recarga, tras actualizar) el catálogo y el índice de portadas, y agrupa los ports por juego.
async function loadCatalog() {
  [state.catalog, state.coverIndex, state.info] = await Promise.all([
    invoke("get_catalog"), invoke("get_cover_index"), invoke("get_game_info").catch(() => ({ featured: [], games: {} })),
  ]);
  state.byId = new Map();
  state.games = new Map();
  for (const g of state.catalog) {
    g._search = [g.title, g.port, g.author, g.franchise, ...(g.aliases || [])].join(" ").toLowerCase();
    state.byId.set(g.id, g);
    // Agrupa los ports por juego; el título del juego es el más corto de sus ports ("Diablo" antes que "Diablo (1996)").
    const gm = state.games.get(g.game) ?? { id: g.game, title: g.title, console: g.console, ports: [] };
    gm.ports.push(g);
    if (g.title.length < gm.title.length) gm.title = g.title;
    state.games.set(g.game, gm);
  }
  for (const gm of state.games.values()) {
    gm._search = gm.ports.map((p) => p._search).join(" ");
    gm.downloads = gm.ports.reduce((a, p) => a + (p.downloads || 0), 0);
    gm.addedAt = gm.ports.map((p) => p.addedAt).filter(Boolean).sort().pop() || null;
  }
  state.gameList = [...state.games.values()].sort((a, b) => a.title.localeCompare(b.title, "es"));
  fillConsoleFilter();
}

// ---------- Ventana de comprobación al abrir ----------

const withTimeout = (p, ms) => Promise.race([p, new Promise((_, rej) => setTimeout(() => rej(new Error(t("sin respuesta"))), ms))]);

function splashStep(id, status, text) {
  const el = $(`#splash-${id}`);
  el.className = `splash-step ${status}`;
  el.querySelector(".txt").innerHTML = text;
}

// Pregunta si instalar la versión nueva. Resuelve true (actualizar) o false (más tarde).
function askAppUpdate(upd) {
  const box = $("#splash-update");
  box.classList.remove("hidden");
  box.querySelector(".upd-title").textContent = t("Dársena {version} está disponible (tienes la {current})", { version: upd.version, current: upd.current });
  box.querySelector(".upd-notes").textContent = upd.notes || "";
  return new Promise((resolve) => {
    box.querySelector("[data-upd=now]").onclick = () => resolve(true);
    box.querySelector("[data-upd=later]").onclick = () => { box.classList.add("hidden"); resolve(false); };
  });
}

async function installAppUpdate() {
  const box = $("#splash-update");
  box.querySelector(".upd-actions").classList.add("hidden");
  const bar = box.querySelector(".upd-progress");
  bar.classList.remove("hidden");
  await listen("app-update-progress", (ev) => {
    const [done, total] = ev.payload;
    bar.querySelector(".bar > div").style.width = total ? `${(done / total) * 100}%` : "30%";
    bar.querySelector(".upd-bytes").textContent = total ? t("{a} de {b}", { a: fmtBytes(done), b: fmtBytes(total) }) : fmtBytes(done);
  });
  splashStep("app", "busy", t("Descargando e instalando la actualización… Dársena se reiniciará sola."));
  // Si va bien, la app se reinicia aquí mismo y no se vuelve de esta llamada.
  await invoke("install_app_update");
}

// Aplica el idioma elegido (o el del sistema) a la interfaz y a los mensajes que vienen de Rust.
async function applyLanguage(preference) {
  const effective = setLang(preference);
  await invoke("set_language", { preference: preference || null, effective }).catch(() => {});
}

async function startup() {
  fillIcons();
  state.settings = await invoke("get_settings").catch(() => null);
  await applyLanguage(state.settings?.language);
  const version = await invoke("get_app_version");
  $("#splash-version").textContent = `v${version}`;

  // 1) Versión nueva de la app
  splashStep("app", "busy", t("Buscando actualizaciones de Dársena…"));
  try {
    const upd = await withTimeout(invoke("check_app_update"), 10000);
    if (upd) {
      splashStep("app", "warn", t("Hay una versión nueva: {version}", { version: `<b>${esc(upd.version)}</b>` }));
      if (await askAppUpdate(upd)) {
        try {
          await installAppUpdate();
          return;
        } catch (e) {
          splashStep("app", "error", `${t("No se pudo actualizar:")} ${esc(String(e))}`);
        }
      } else {
        splashStep("app", "done", t("Actualización pospuesta (sigues en la v{version})", { version: esc(version) }));
      }
    } else {
      splashStep("app", "done", t("Dársena está al día (v{version})", { version: esc(version) }));
    }
  } catch (e) {
    splashStep("app", "error", t("No se pudo comprobar si hay versiones nuevas"));
  }

  // 2) Catálogo
  splashStep("catalog", "busy", t("Comprobando el catálogo de juegos…"));
  let added = [];
  try {
    const r = await withTimeout(invoke("check_catalog_update"), 20000);
    added = r.added;
    if (!r.updated) splashStep("catalog", "done", t("Catálogo al día · {n} juegos", { n: r.games }));
    else if (!added.length) splashStep("catalog", "done", t("Catálogo actualizado · {n} juegos", { n: r.games }));
    else {
      const list = [...new Set(added)].slice(0, 4).map(esc).join(", ");
      splashStep("catalog", "new", `<b>${t(added.length === 1 ? "{n} juego nuevo" : "{n} juegos nuevos", { n: added.length })}</b>: ${list}${added.length > 4 ? "…" : ""}`);
    }
  } catch (e) {
    splashStep("catalog", "error", t("Sin conexión con GitHub: se usa el catálogo guardado"));
  }

  await loadCatalog();
  await reload();
  renderAccount();
  loadAccount();
  invoke("get_review_summaries").then((s) => { state.reviewSummaries = s; }).catch(() => {});
  checkPortUpdates(true);
  // Si llegaron juegos nuevos, da tiempo a leerlo antes de entrar.
  await new Promise((r) => setTimeout(r, added.length ? 2200 : 600));
  $("#splash").classList.add("leaving");
  setTimeout(() => $("#splash").remove(), 450);
}

startup();

// ---------- Menú contextual (clic derecho) ----------
// Solo sobre un juego: en el resto de la app no sale nada (ni el menú del navegador).

function closeCtxMenu() {
  $("#ctx-menu")?.remove();
}

function ctxMenuHtml(gm) {
  const inst = installedPortOf(gm);
  const owned = isOwned(gm.id);
  // Port de referencia: el instalado, el elegido en la biblioteca o el primero.
  const port = (inst && state.byId.get(inst.id))
    || (state.libGame === gm.id && state.byId.get(state.libPort))
    || sortedPorts(gm)[0];
  const item = (attrs, label, cls = "", ico = "") => `<button class="${cls}" ${attrs}>${ico ? icon(ico) : `<span class="ico"></span>`}${label}</button>`;
  const id = esc(gm.id);
  const items = [];
  if (inst) {
    const running = state.running.has(inst.id);
    items.push(item(`data-play="${esc(inst.id)}" ${running ? "disabled" : ""}`, running ? t("Ejecutando…") : t("Jugar"), "strong", "play"));
  } else if (owned) {
    items.push(item(`data-goto-lib="${id}"`, t("Instalar…"), "strong", "downloads"));
  } else {
    items.push(item(`data-add="${id}"`, t("Añadir a la biblioteca"), "strong", "plus"));
  }
  if (owned) items.push(item(`data-fav="${id}"`, isFavorite(gm.id) ? t("Quitar de favoritos") : t("Añadir a favoritos"), "", "star"));
  items.push("<hr>");
  items.push(item(`data-open="${id}"`, t("Ver ficha"), "", "info"));
  if (owned && state.view !== "library") items.push(item(`data-goto-lib="${id}"`, t("Ver en la biblioteca"), "", "library"));
  if (port && !isExternal(port)) items.push(item(`data-url="${esc(repoUrl(port))}"`, repoLabel(port), "", "github"));
  if (inst && state.updates?.[inst.id]) items.unshift(item(`data-update="${esc(inst.id)}"`, t("Actualizar a {v}", { v: esc(state.updates[inst.id].latest) }), "strong", "downloads"));
  if (port?.homepage && port.homepage !== repoUrl(port)) items.push(item(`data-url="${esc(port.homepage)}"`, t("Página del proyecto"), "", "external"));
  if (inst) {
    items.push(item(`data-folder="${esc(inst.id)}"`, t("Abrir carpeta"), "", "folder"));
    items.push(item(`data-pick-exe="${esc(inst.id)}"`, t("Elegir ejecutable…"), "", "file"));
    items.push(item(`data-shortcut="${esc(inst.id)}"`, t("Crear acceso directo"), "", "shortcut"));
    items.push("<hr>");
    items.push(item(`data-uninstall="${esc(inst.id)}"`, state.library.find((x) => x.id === inst.id)?.linked ? t("Quitar vínculo…") : t("Desinstalar…"), "danger", "trash"));
  } else if (owned) {
    items.push("<hr>");
    items.push(item(`data-remove-lib="${id}"`, t("Quitar de la biblioteca"), "danger", "trash"));
  }
  return `<div class="ctx-title">${esc(gm.title)}</div>${items.join("")}`;
}

// Botón ⚙ de la biblioteca: abre el mismo menú que el clic derecho, debajo del botón.
document.addEventListener("click", (e) => {
  const b = e.target.closest("[data-gear]");
  if (!b) return;
  e.stopPropagation();
  const open = $("#ctx-menu");
  closeCtxMenu();
  if (open?.dataset.gear === b.dataset.gear) return;
  const gm = state.games.get(b.dataset.gear);
  if (!gm) return;
  const menu = document.createElement("div");
  menu.id = "ctx-menu";
  menu.className = "ctx-menu";
  menu.dataset.gear = gm.id;
  menu.innerHTML = ctxMenuHtml(gm);
  document.body.appendChild(menu);
  const r = b.getBoundingClientRect();
  const m = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(r.right - m.width, window.innerWidth - m.width - 8)}px`;
  menu.style.top = `${Math.min(r.bottom + 6, window.innerHeight - m.height - 8)}px`;
}, true);

// Menú de un port concreto (las pestañas de «Ports disponibles»): su proyecto.
function portCtxMenuHtml(p) {
  const item = (attrs, label, ico) => `<button ${attrs}>${icon(ico)}${label}</button>`;
  const items = [];
  const page = p.homepage || (!isExternal(p) ? repoUrl(p) : null);
  if (page) items.push(item(`data-url="${esc(page)}"`, t("Página del proyecto"), "external"));
  if (!isExternal(p) && repoUrl(p) !== page) items.push(item(`data-url="${esc(repoUrl(p))}"`, repoLabel(p), "github"));
  if (!isExternal(p) && !p.gitlab) items.push(item(`data-url="${esc(repoUrl(p))}/releases"`, t("Ver versiones (releases)"), "downloads"));
  if (isInstalled(p.id)) items.push(item(`data-folder="${esc(p.id)}"`, t("Abrir carpeta"), "folder"));
  return `<div class="ctx-title">${esc(p.port)} · ${esc(t(p.author))}</div>${items.join("")}`;
}

document.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  closeCtxMenu();
  const tab = e.target.closest(".port-tab[data-port]");
  const tabPort = tab && state.byId.get(tab.dataset.port);
  if (tabPort) {
    const menu = document.createElement("div");
    menu.id = "ctx-menu";
    menu.className = "ctx-menu";
    menu.innerHTML = portCtxMenuHtml(tabPort);
    document.body.appendChild(menu);
    const r = menu.getBoundingClientRect();
    menu.style.left = `${Math.min(e.clientX, window.innerWidth - r.width - 8)}px`;
    menu.style.top = `${Math.min(e.clientY, window.innerHeight - r.height - 8)}px`;
    return;
  }
  const el = e.target.closest("[data-lib], .card[data-open], .car-art[data-open], .car-capsule[data-open], .car-side h3[data-open], .pg[data-open]");
  const gm = el && state.games.get(el.dataset.lib || el.dataset.open);
  if (!gm) return;
  const menu = document.createElement("div");
  menu.id = "ctx-menu";
  menu.className = "ctx-menu";
  menu.innerHTML = ctxMenuHtml(gm);
  document.body.appendChild(menu);
  // Dentro de la ventana aunque se haga clic cerca de un borde.
  const { innerWidth: w, innerHeight: h } = window;
  const r = menu.getBoundingClientRect();
  menu.style.left = `${Math.min(e.clientX, w - r.width - 8)}px`;
  menu.style.top = `${Math.min(e.clientY, h - r.height - 8)}px`;
});

document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeCtxMenu(); });
window.addEventListener("blur", closeCtxMenu);
document.addEventListener("scroll", closeCtxMenu, true);

// ---------- Subir un port (para sus autores) ----------

const SUBMIT_FORMATS = ["iso", "carpeta", "rvz", "chd", "bin", "cue", "z64", "sfc", "gba", "nds", "3ds", "xex", "pkg", "zip"];
const submitState = { repos: null, check: null, mine: null, checking: false };

function openSubmit() {
  show("submit");
  if (!state.user) return;
  if (!submitState.repos) invoke("my_repos").then((r) => { submitState.repos = r; renderSubmit(); }).catch(() => { submitState.repos = []; renderSubmit(); });
  invoke("my_submissions").then((m) => { submitState.mine = m; renderSubmit(); }).catch(() => {});
}

function renderSubmit() {
  const box = $("#submit");
  if (!box || state.view !== "submit") return;
  if (!state.user) {
    box.innerHTML = emptyHtml(t("Subir un port"), t("Inicia sesión con GitHub para subir un port del que seas autor."),
      `<button class="btn primary" data-login>${icon("github")}${t("Iniciar sesión con GitHub")}</button>`);
    return;
  }
  const c = submitState.check;
  const ok = c && !c.problems.length;
  const consoles = Object.keys(CONSOLE_HUE);
  const repoOptions = (submitState.repos || []).map((r) =>
    `<option value="${esc(r.fullName)}" ${r.inCatalog ? "disabled" : ""}>${esc(r.fullName)}${r.inCatalog ? ` · ${t("ya en el catálogo")}` : ""}</option>`).join("");
  const mine = submitState.mine || [];
  const statusTag = (s) => s === "approved" ? `<span class="tag ok">${t("APROBADO")}</span>`
    : s === "rejected" ? `<span class="tag warn">${t("NO APROBADO")}</span>` : `<span class="tag">${t("PENDIENTE DE REVISIÓN")}</span>`;

  box.innerHTML = `
    <div class="submit-page">
      <h2>${t("Subir un port")}</h2>
      <p class="muted">${t("¿Has creado un port? Súbelo para que el autor de Dársena lo revise. Si cumple los estándares de calidad, entrará en el catálogo para todos.")}</p>
      <ul class="submit-rules">
        <li>${t("Solo puedes subir repositorios de los que seas <b>dueño o colaborador</b>.")}</li>
        <li>${t("Necesita una <b>release con build de Windows</b> (.zip o .exe) en GitHub, o que su aplicación (.exe o .bat) esté en la raíz del repositorio.")}</li>
        <li>${t("El port <b>no puede incluir archivos del juego original</b>: cada jugador usa su propia copia.")}</li>
        <li>${t("Puedes personalizar la ficha con un <code>darsena.json</code> en tu repositorio.")} <a data-url="https://github.com/theolivander97/darsena-launcher/wiki/Subir-un-port">${t("Cómo hacerlo")}</a></li>
      </ul>

      <h3 class="section-title">1. ${t("Tu repositorio")}</h3>
      <div class="row">
        <select id="sub-repo-list"><option value="">${submitState.repos ? t("Elige uno de tus repositorios…") : t("Cargando tus repositorios…")}</option>${repoOptions}</select>
        <span class="muted small">${t("o")}</span>
        <input id="sub-repo" type="text" placeholder="${t("usuario/repositorio")}" value="${esc(c?.fullName || "")}" autocomplete="off">
        <button class="btn" id="sub-check" ${submitState.checking ? "disabled" : ""}>${submitState.checking ? t("Comprobando…") : t("Comprobar")}</button>
      </div>
      ${c ? `<div class="sub-check ${ok ? "ok" : "bad"}">
        <div class="sub-check-head"><b>${esc(c.fullName)}</b> <span class="muted small">★ ${c.stars}</span>
          <a class="muted small" data-url="${esc(c.htmlUrl)}">${t("Ver en GitHub")} ↗</a></div>
        ${c.description ? `<div class="muted small">${esc(c.description)}</div>` : ""}
        <ul>
          <li class="${c.authorOk ? "yes" : "no"}">${c.authorOk ? t("Eres autor de este repositorio") : t("No eres autor de este repositorio")}</li>
          <li class="${c.build || c.appInRepo ? "yes" : "no"}">${c.build ? t("Build de Windows: {v} ({a})", { v: esc(c.build[0]), a: esc(c.build[1]) })
            : c.appInRepo ? t("Aplicación en el repositorio: {exe}", { exe: esc(c.appInRepo.exe) }) + (c.appInRepo.setupExe ? ` · ${t("la primera vez se prepara con {exe}", { exe: esc(c.appInRepo.setupExe) })}` : "")
            : t("Sin build de Windows en sus releases")}</li>
          <li class="${c.inCatalog ? "no" : "yes"}">${c.inCatalog ? t("Ya está en el catálogo") : t("Todavía no está en el catálogo")}</li>
          <li class="${c.hasDarsenaJson ? "yes" : "info"}">${c.hasDarsenaJson ? t("Tiene darsena.json: su ficha se personalizará sola") : t("Sin darsena.json (opcional)")}</li>
        </ul>
        ${c.problems.map((p) => `<div class="err">${esc(p)}</div>`).join("")}
      </div>` : ""}

      <fieldset class="sub-form" ${ok ? "" : "disabled"}>
        <h3 class="section-title">2. ${t("El juego")}</h3>
        <div class="sub-grid">
          <label>${t("Nombre del juego original")}<input id="sub-title" type="text" placeholder="${t("p. ej. Star Fox 64 (no el nombre del proyecto)")}"></label>
          <label>${t("Consola original")}<select id="sub-console"><option value="">—</option>${consoles.map((x) => `<option value="${esc(x)}">${esc(consoleName(x))}</option>`).join("")}</select></label>
          <label>${t("Saga (opcional)")}<input id="sub-franchise" type="text"></label>
          <label>${t("Otros nombres (opcional, separados por comas)")}<input id="sub-aliases" type="text"></label>
        </div>
        <h3 class="section-title">3. ${t("El port")}</h3>
        <div class="sub-label">${t("Qué datos del juego original necesita")}</div>
        <div class="sub-checks">${SUBMIT_FORMATS.map((f) => `<label class="check small"><input type="checkbox" data-sub-need="${f}"> ${f === "carpeta" ? t("CARPETA") : f.toUpperCase()}</label>`).join("")}</div>
        <label class="sub-wide">${t("Aclaración sobre los datos (región, versión exacta…)")}<input id="sub-needs-note" type="text" maxlength="300"></label>
        <div class="sub-label">${t("Funciona en")}</div>
        <div class="sub-checks">${["Windows", "Linux", "macOS"].map((p) => `<label class="check small"><input type="checkbox" data-sub-platform="${p}" ${p === "Windows" ? "checked disabled" : ""}> ${p}</label>`).join("")}
          <label class="check small"><input type="checkbox" id="sub-experimental"> ${t("Es una beta o está en desarrollo (experimental)")}</label></div>
        <label class="sub-wide">${t("¿Has usado IA para desarrollar el port?")}<select id="sub-ai">
          <option value="none">${t("Sin IA")}</option>
          <option value="assisted">${t("Hecho con ayuda de IA")}</option>
          <option value="generated">${t("Generado en gran parte con IA")}</option>
        </select></label>
        <h3 class="section-title">4. ${t("La ficha (opcional)")}</h3>
        <label class="sub-wide">${t("Sinopsis del juego")}<textarea id="sub-synopsis" maxlength="2000" placeholder="${t("De qué trata el juego. Si no la pones, se busca en Wikipedia.")}"></textarea></label>
        <label class="sub-wide">${t("Tráiler del juego en YouTube")}<input id="sub-trailer" type="text" placeholder="https://www.youtube.com/watch?v=…"></label>
        <label class="sub-wide">${t("Notas para el revisor")}<textarea id="sub-notes" maxlength="2000" placeholder="${t("Estado del port, qué funciona, cómo probarlo…")}"></textarea></label>
        <label class="check sub-confirm"><input type="checkbox" id="sub-confirm"> ${t("Confirmo que soy autor o colaborador de este port y que no incluye archivos con copyright del juego original.")}</label>
        <button class="btn primary" id="sub-send">${icon("check")}${t("Subir para revisión")}</button>
      </fieldset>

      ${shelfHead(`${t("Mis ports subidos")} <span class="muted">(${mine.length})</span>`)}
      ${mine.length ? mine.map((m) => `<div class="sub-item">
        <div><b>${esc(m.title)}</b> <span class="muted small">· ${esc(m.repo)} · ${fmtIsoDate(m.createdAt)}</span></div>
        ${statusTag(m.status)}
        ${m.reply ? `<div class="sub-reply">${mdToHtml(m.reply)}</div>` : ""}
        <a class="muted small" data-url="${esc(m.url)}">${t("Ver en GitHub")} ↗</a>
      </div>`).join("") : `<p class="muted">${submitState.mine ? t("Todavía no has subido ningún port.") : t("Cargando…")}</p>`}
    </div>`;

  // Lo que ya se había escrito se conserva al repintar.
  for (const [id, v] of Object.entries(submitState.draft || {})) {
    const el = document.getElementById(id);
    if (el) el.type === "checkbox" ? (el.checked = v) : (el.value = v);
  }
  if (c && !$("#sub-title").value && c.description) {
    // Sugerencia: nada, el nombre del juego lo pone el autor (el del repo suele ser el del proyecto).
  }
}

function submitDraft() {
  const d = {};
  document.querySelectorAll(".sub-form input, .sub-form select, .sub-form textarea").forEach((el) => {
    if (el.id) d[el.id] = el.type === "checkbox" ? el.checked : el.value;
  });
  submitState.draft = d;
}

document.addEventListener("input", (e) => { if (e.target.closest(".sub-form")) submitDraft(); });
document.addEventListener("change", (e) => {
  if (e.target.id === "sub-repo-list" && e.target.value) { $("#sub-repo").value = e.target.value; checkSubmitRepo(); }
  else if (e.target.closest(".sub-form")) submitDraft();
});
document.addEventListener("click", (e) => {
  if (e.target.closest("#sub-check")) checkSubmitRepo();
  else if (e.target.closest("#sub-send")) sendSubmission();
});

async function checkSubmitRepo() {
  const repo = $("#sub-repo").value.trim();
  if (!repo) return;
  submitDraft();
  submitState.checking = true;
  renderSubmit();
  try { submitState.check = await call("check_repo", { repo }); } catch { submitState.check = null; }
  submitState.checking = false;
  renderSubmit();
}

async function sendSubmission() {
  const c = submitState.check;
  if (!c || c.problems.length) return;
  if (!$("#sub-confirm").checked) { toast(t("Marca la casilla de confirmación"), "error"); return; }
  const list = (v) => v.split(",").map((x) => x.trim()).filter(Boolean);
  const submission = {
    repo: c.fullName,
    title: $("#sub-title").value.trim(),
    console: $("#sub-console").value,
    franchise: $("#sub-franchise").value.trim() || null,
    aliases: list($("#sub-aliases").value),
    needs: [...document.querySelectorAll("[data-sub-need]:checked")].map((x) => x.dataset.subNeed),
    needsNote: $("#sub-needs-note").value.trim() || null,
    platforms: [...document.querySelectorAll("[data-sub-platform]:checked")].map((x) => x.dataset.subPlatform),
    experimental: $("#sub-experimental").checked,
    ai: $("#sub-ai").value,
    synopsis: $("#sub-synopsis").value.trim() || null,
    trailer: $("#sub-trailer").value.trim() || null,
    notes: $("#sub-notes").value.trim() || null,
  };
  if (!submission.title || !submission.console) { toast(t("Falta el nombre del juego o la consola"), "error"); return; }
  const btn = $("#sub-send");
  btn.disabled = true;
  try {
    await call("submit_port", { submission });
    toast(t("Port subido. Aquí verás cuándo esté revisado."), "ok");
    submitState.check = null;
    submitState.draft = null;
    submitState.mine = await invoke("my_submissions").catch(() => submitState.mine);
    renderSubmit();
  } catch { btn.disabled = false; }
}

// ---------- Ficha: lo que cuenta el autor del port (darsena.json de su repositorio) ----------

async function loadAuthorInfo(gm) {
  const box = $("#author-info");
  if (!box) return;
  const ports = gm.ports.filter((p) => !isExternal(p));
  const results = await Promise.all(ports.map(async (p) => [p, await invoke("get_author_info", { id: p.id }).catch(() => null)]));
  if (state.view !== "detail" || state.detailGame !== gm.id || !$("#author-info")) return;
  const pick = (v) => (v ? v[LANG()] || v.es || v.en : "");
  // Lo que el autor decora: su sinopsis (si su port es el único del juego) y la imagen de fondo de la cabecera.
  const solo = gm.ports.length === 1 ? results.find(([, a]) => a)?.[1] : null;
  if (solo?.synopsis && $(".synopsis")) {
    $(".synopsis").textContent = pick(solo.synopsis);
    const src = $(".synopsis-src");
    if (src) src.textContent = t("Escrita por el autor del port");
  }
  renderRequirements(gm, results);
  const bg = results.find(([, a]) => a?.background)?.[1]?.background;
  const hero = $("#detail .hero");
  if (bg && hero) {
    hero.querySelector(".hero-wide")?.remove();
    hero.classList.add("has-wide");
    hero.insertAdjacentHTML("afterbegin", `<img class="hero-wide" src="${esc(bg)}" alt="">`);
  }
  $("#author-info").innerHTML = results.filter(([, a]) => a).map(([p, a]) => `
    <h2 class="section-title">${t("Del autor del port")} <span class="muted small">· ${esc(p.port)} ${t("por {author}", { author: esc(p.author) })}</span></h2>
    ${a.description ? `<div class="author-desc">${mdToHtml(pick(a.description))}</div>` : ""}
    ${a.screenshots?.length ? `<div class="shots">${a.screenshots.map((u) => `<img src="${esc(u)}" alt="" loading="lazy" data-shot="${esc(u)}" onerror="this.remove()">`).join("")}</div>` : ""}
    ${a.trailers?.length ? `<div class="shots">${a.trailers.map((id) => `<button class="trailer-thumb small" data-author-trailer="${esc(id)}" style="background-image:url(https://i.ytimg.com/vi/${esc(id)}/hqdefault.jpg)"><span class="play">▶</span></button>`).join("")}</div>` : ""}
    ${a.needsNote ? `<p class="req">${esc(pick(a.needsNote))}</p>` : ""}
    ${a.website ? `<button class="btn ghost" data-url="${esc(a.website)}">${icon("external")}${t("Web del port")}</button>` : ""}
    ${a.support?.length ? `<div class="support-box"><span>${t("Apoyar al autor")}</span>${a.support.map((u) => `<button class="btn support" data-url="${esc(u)}">${esc(supportName(u))}</button>`).join("")}</div>` : ""}`).join("");
}

// Nombre legible de un enlace de apoyo según su dominio.
function supportName(u) {
  const host = (() => { try { return new URL(u).hostname.replace(/^www\./, ""); } catch { return u; } })();
  const known = { "ko-fi.com": "Ko-fi", "patreon.com": "Patreon", "github.com": "GitHub Sponsors", "paypal.com": "PayPal", "paypal.me": "PayPal", "buymeacoffee.com": "Buy Me a Coffee", "liberapay.com": "Liberapay", "itch.io": "itch.io" };
  return known[host] || host;
}

// Captura o tráiler del autor a pantalla completa.
document.addEventListener("click", (e) => {
  const shot = e.target.closest("[data-shot]");
  const tr = e.target.closest("[data-author-trailer]");
  if (!shot && !tr) return;
  const box = document.createElement("div");
  box.className = "lightbox";
  box.innerHTML = shot ? `<img src="${esc(shot.dataset.shot)}" alt="">`
    : `<div class="lightbox-video">${trailerPlayer(tr.dataset.authorTrailer)}</div>`;
  box.addEventListener("click", () => box.remove());
  document.body.appendChild(box);
});
document.addEventListener("keydown", (e) => { if (e.key === "Escape") document.querySelector(".lightbox")?.remove(); });


document.addEventListener("click", (e) => {
  if (e.target.closest("[data-submit]")) { toggleAccountMenu(false); openSubmit(); }
  const set = e.target.closest("[data-settings],[data-set-tab]");
  if (set) { toggleAccountMenu(false); openSettings(set.dataset.setTab || set.dataset.settings || null); }
});

// Responder a una reseña como desarrollador.
document.addEventListener("click", async (e) => {
  const open = e.target.closest("[data-reply-open]");
  if (open) {
    const form = open.parentElement.querySelector(".reply-form");
    form.classList.toggle("hidden");
    form.querySelector("textarea")?.focus();
    return;
  }
  const send = e.target.closest("[data-reply-send]");
  if (!send) return;
  const text = send.parentElement.querySelector("textarea").value.trim();
  if (!text) return;
  send.disabled = true;
  try {
    const game = state.reviews.game;
    setReviews(game, await call("reply_review", { game, review: send.dataset.replySend, text }));
    toast(t("Respuesta publicada"), "ok");
  } catch { send.disabled = false; }
});

// ---------- Panel del desarrollador: tus ports del catálogo ----------

const devState = { ports: null, reviews: {}, error: null };

async function openDevPanel() {
  show("dev");
  if (!state.user) return;
  try {
    devState.ports = await invoke("my_dev_ports");
    devState.error = null;
  } catch (e) { devState.error = String(e); devState.ports = []; }
  renderDev();
  // Reseñas de cada juego (en paralelo): para ver las que aún no tienen respuesta.
  const games = [...new Set(devState.ports.map((id) => state.byId.get(id)?.game).filter(Boolean))];
  await Promise.all(games.map(async (g) => { devState.reviews[g] = await invoke("get_reviews", { game: g }).catch(() => null); }));
  renderDev();
}

function renderDev() {
  const box = $("#dev");
  if (!box || state.view !== "dev") return;
  if (!state.user) {
    box.innerHTML = emptyHtml(t("Panel del desarrollador"), t("Inicia sesión con GitHub para ver los ports del catálogo que son tuyos."),
      `<button class="btn primary" data-login>${icon("github")}${t("Iniciar sesión con GitHub")}</button>`);
    return;
  }
  if (!devState.ports) { box.innerHTML = `<p class="muted">${t("Cargando…")}</p>`; return; }
  if (!devState.ports.length) {
    box.innerHTML = emptyHtml(t("Panel del desarrollador"),
      t("Ningún port del catálogo es de tu cuenta ni de tus organizaciones de GitHub. Si has creado un port, súbelo para que se revise."),
      `<button class="btn primary" data-submit>${icon("plus")}${t("Subir un port")}</button>`);
    return;
  }
  const cards = devState.ports.map((id) => {
    const p = state.byId.get(id);
    if (!p) return "";
    const gm = state.games.get(p.game);
    const rv = devState.reviews[p.game];
    const sum = state.reviewSummaries[p.game] || (rv?.count ? { average: rv.average, count: rv.count } : null);
    const pending = (rv?.reviews || []).filter((x) => !(x.replies || []).length);
    return `<div class="dev-card">
      <div class="dev-art" style="${coverStyle(gm)}" data-open="${esc(gm.id)}">${coverImg(gm, "ph-img")}</div>
      <div class="dev-main">
        <h3>${esc(gm.title)} <span class="muted small">· ${esc(p.port)}</span></h3>
        <div class="dev-stats">
          <div><span>${t("DESCARGAS")}</span><b>${fmtCount(p.downloads || 0)}</b></div>
          <div><span>${t("VALORACIÓN")}</span><b>${sum ? `${sum.average.toFixed(1)} ★ · ${t(sum.count === 1 ? "{n} reseña" : "{n} reseñas", { n: sum.count })}` : "—"}</b></div>
          <div><span>${t("SIN RESPONDER")}</span><b class="${pending.length ? "warn" : ""}">${rv ? pending.length : "…"}</b></div>
        </div>
        ${pending.slice(0, 3).map((x) => `<div class="dev-pending" data-open-reviews="${esc(gm.id)}">
          ${starsHtml(x.rating)} <b>${esc(x.user)}</b> <span class="muted">${esc((x.text || "").slice(0, 120))}${(x.text || "").length > 120 ? "…" : ""}</span></div>`).join("")}
        <div class="row">
          <button class="btn primary" data-edit-page="${esc(id)}">${icon("edit")}${t("Editar ficha")}</button>
          <button class="btn" data-open-reviews="${esc(gm.id)}">${icon("reply")}${t("Ver y responder reseñas")}</button>
          <button class="btn ghost" data-url="${esc(repoUrl(p))}">${icon("github")}${t("Repositorio")}</button>
        </div>
      </div>
    </div>`;
  }).join("");
  box.innerHTML = `<div class="dev-page">
    <h2>${t("Panel del desarrollador")}</h2>
    <p class="muted">${t("Tus ports del catálogo de Dársena: cómo les va, las reseñas que esperan respuesta y su ficha.")}</p>
    ${devState.error ? `<p class="err">${esc(devState.error)}</p>` : ""}
    ${cards}
  </div>`;
}

// Abre la ficha del juego directamente en sus reseñas.
function openReviewsOf(gameId) {
  state.detailGame = gameId;
  show("detail");
  setTimeout(() => $("#reviews")?.scrollIntoView({ behavior: "smooth", block: "start" }), 300);
}

// ---------- Editor de la ficha (genera el darsena.json del repositorio) ----------

async function openPageEditor(portId) {
  const p = state.byId.get(portId);
  if (!p) return;
  const a = (await invoke("get_author_info", { id: portId }).catch(() => null)) || {};
  const pick = (v, l) => (v ? v[l] || "" : "");
  $("#page-editor")?.remove();
  const box = document.createElement("div");
  box.id = "page-editor";
  box.className = "news-modal";
  box.innerHTML = `<div class="news-dialog editor">
      <div class="news-head">
        <div class="news-meta"><span class="news-kind">${t("EDITAR FICHA")}</span> ${esc(p.repo)}</div>
        <h2>${esc(state.games.get(p.game)?.title || p.title)} · ${esc(p.port)}</h2>
        <div class="muted small">${t("Esto genera el archivo darsena.json de tu repositorio. Dársena lo lee y actualiza la ficha sola cada media hora.")}</div>
      </div>
      <div class="news-body ed-form">
        <div class="ed-grid">
          <label>${t("Descripción del port (español)")}<textarea data-ed="description.es" placeholder="${t("Qué hace tu port: mejoras, estado, controles… Admite Markdown sencillo.")}">${esc(pick(a.description, "es"))}</textarea></label>
          <label>${t("Descripción del port (inglés)")}<textarea data-ed="description.en">${esc(pick(a.description, "en"))}</textarea></label>
          <label>${t("Sinopsis del juego (español, opcional)")}<textarea data-ed="synopsis.es" placeholder="${t("Si la pones, sustituye a la de Wikipedia cuando tu port es el único del juego.")}">${esc(pick(a.synopsis, "es"))}</textarea></label>
          <label>${t("Sinopsis del juego (inglés, opcional)")}<textarea data-ed="synopsis.en">${esc(pick(a.synopsis, "en"))}</textarea></label>
        </div>
        <label>${t("Imagen de fondo grande (https://…, 1920×620 o similar)")}<input type="text" data-ed="background" value="${esc(a.background || "")}"></label>
        <label>${t("Logo del juego con fondo transparente (https://… .png)")}<input type="text" data-ed="logo" value="${esc(a.logo || "")}"></label>
        <label>${t("Capturas (una URL https:// por línea, máximo 8)")}<textarea data-ed="screenshots" class="short">${esc((a.screenshots || []).join("\n"))}</textarea></label>
        <label>${t("Tráilers de YouTube (uno por línea, máximo 3)")}<textarea data-ed="trailers" class="short">${esc((a.trailers || []).map((id) => `https://www.youtube.com/watch?v=${id}`).join("\n"))}</textarea></label>
        <label>${t("Web del port (opcional)")}<input type="text" data-ed="website" value="${esc(a.website || "")}"></label>
        <label>${t("Enlaces para apoyarte: Ko-fi, Patreon, PayPal… (uno por línea, máximo 4)")}<textarea data-ed="support" class="short">${esc((a.support || []).join("\n"))}</textarea></label>
        <div class="ed-grid">${["minimum", "recommended"].map((lv) => `<div class="ed-req">
          <div class="muted small"><b>${lv === "minimum" ? t("Requisitos mínimos") : t("Requisitos recomendados")}</b></div>
          ${REQ_FIELDS.map(([k, label]) => `<label>${t(label)}<input type="text" data-ed="req.${lv}.${k}" value="${esc(pick(a.requirements?.[lv]?.[k], LANG()) || pick(a.requirements?.[lv]?.[k], "es"))}"></label>`).join("")}
        </div>`).join("")}</div>
        <label>${t("Aclaración sobre los datos que pide (español)")}<input type="text" data-ed="needsNote.es" value="${esc(pick(a.needsNote, "es"))}"></label>
        <label>${t("Aclaración sobre los datos que pide (inglés)")}<input type="text" data-ed="needsNote.en" value="${esc(pick(a.needsNote, "en"))}"></label>
        <div class="ed-preview"><div class="muted small">darsena.json</div><pre id="ed-json"></pre></div>
      </div>
      <div class="news-foot">
        <button class="btn primary" data-ed-github="${esc(portId)}" data-ed-exists="${a.description || a.screenshots || a.background ? "1" : ""}">${icon("github")}${t("Copiar y guardar en GitHub")}</button>
        <button class="btn ghost" data-ed-copy>${t("Copiar darsena.json")}</button>
      </div>
    </div>
    <div class="news-side"><button class="news-round" data-ed-close title="${t("Cerrar")}">${icon("close")}</button></div>`;
  document.body.appendChild(box);
  const refresh = () => { $("#ed-json").textContent = JSON.stringify(editorJson(), null, 2); };
  box.addEventListener("input", refresh);
  box.addEventListener("click", (e) => { if (e.target === box || e.target.closest("[data-ed-close]")) box.remove(); });
  refresh();
}

function editorJson() {
  const v = (k) => document.querySelector(`[data-ed="${k}"]`)?.value.trim() || "";
  const pair = (k) => (v(`${k}.es`) || v(`${k}.en`) ? { es: v(`${k}.es`) || v(`${k}.en`), en: v(`${k}.en`) || v(`${k}.es`) } : undefined);
  const lines = (k) => v(k).split(/\n+/).map((x) => x.trim()).filter(Boolean);
  const out = {
    description: pair("description"),
    synopsis: pair("synopsis"),
    background: v("background") || undefined,
    logo: v("logo") || undefined,
    screenshots: lines("screenshots").slice(0, 8),
    trailers: lines("trailers").slice(0, 3),
    website: v("website") || undefined,
    support: lines("support").slice(0, 4),
    requirements: (() => {
      const out = {};
      for (const lv of ["minimum", "recommended"]) {
        const set = Object.fromEntries(REQ_FIELDS.map(([k]) => [k, v(`req.${lv}.${k}`)]).filter(([, x]) => x));
        if (Object.keys(set).length) out[lv] = set;
      }
      return Object.keys(out).length ? out : undefined;
    })(),
    needsNote: pair("needsNote"),
  };
  for (const k of Object.keys(out)) if (out[k] === undefined || (Array.isArray(out[k]) && !out[k].length)) delete out[k];
  return out;
}

document.addEventListener("click", async (e) => {
  const edit = e.target.closest("[data-edit-page]");
  if (edit) { openPageEditor(edit.dataset.editPage); return; }
  const rev = e.target.closest("[data-open-reviews]");
  if (rev) { openReviewsOf(rev.dataset.openReviews); return; }
  if (e.target.closest("[data-dev-panel]")) { toggleAccountMenu(false); openDevPanel(); return; }
  if (e.target.closest("[data-ed-copy]")) {
    await navigator.clipboard.writeText(JSON.stringify(editorJson(), null, 2)).catch(() => {});
    toast(t("darsena.json copiado"), "ok");
    return;
  }
  const gh = e.target.closest("[data-ed-github]");
  if (gh) {
    const p = state.byId.get(gh.dataset.edGithub);
    const json = JSON.stringify(editorJson(), null, 2);
    await navigator.clipboard.writeText(json).catch(() => {});
    let branch = "main";
    try { branch = await invoke("repo_default_branch", { repo: p.repo }); } catch {}
    // Si ya existe, se abre su editor (el contenido está copiado: pegar y guardar); si no, se crea ya relleno.
    const url = gh.dataset.edExists
      ? `https://github.com/${p.repo}/edit/${encodeURIComponent(branch)}/darsena.json`
      : `https://github.com/${p.repo}/new/${encodeURIComponent(branch)}?filename=darsena.json&value=${encodeURIComponent(json)}`;
    await call("open_url", { url });
    toast(gh.dataset.edExists ? t("Copiado. En GitHub, sustituye el contenido de darsena.json y pulsa «Commit changes».")
      : t("En GitHub, revisa el archivo y pulsa «Commit changes»."), "ok");
  }
});

// ---------- Actualizaciones de los ports instalados ----------

// id del port -> { installed, latest }. Si "actualizar solos" está activado, se aplican al abrir Dársena.
async function checkPortUpdates(atStartup = false) {
  const list = await invoke("port_updates").catch(() => []);
  state.updates = Object.fromEntries(list.map((u) => [u.id, u]));
  if (atStartup && list.length && state.settings?.autoUpdatePorts !== false) {
    const todo = list.filter((u) => !state.running.has(u.id));
    for (const u of todo) await invoke("update_game", { id: u.id }).catch(() => {});
    if (todo.length) toast(t(todo.length === 1 ? "Actualizando {n} port a su última versión" : "Actualizando {n} ports a su última versión", { n: todo.length }), "ok");
  }
  render();
}

document.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-update]");
  if (!b) return;
  closeCtxMenu();
  b.disabled = true;
  try {
    await call("update_game", { id: b.dataset.update });
    toast(t("Actualizando… tus partidas y ajustes se conservan"), "ok");
    state.downloads = await invoke("list_downloads");
    render();
  } catch { b.disabled = false; }
});

// Al terminar una actualización, la versión nueva ya no está pendiente.
listen("library-changed", async () => {
  if (!state.updates || !Object.keys(state.updates).length) return;
  const lib = await invoke("get_library").catch(() => state.library);
  let changed = false;
  for (const g of lib) {
    if (state.updates[g.id] && g.version === state.updates[g.id].latest) { delete state.updates[g.id]; changed = true; }
  }
  if (changed) render();
});

// ---------- Uso de IA ----------

const AI_LABELS = { none: "Sin IA", assisted: "Hecho con ayuda de IA", generated: "Generado en gran parte con IA" };
const aiLabel = (v) => t(AI_LABELS[v] || v);
// El de la ficha: si todos los ports del juego lo declaran igual, ese; si no, el del primero que lo declare.
const aiOfGame = (gm) => {
  const vals = [...new Set(gm.ports.map((p) => p.ai).filter(Boolean))];
  return vals.length === 1 ? vals[0] : vals.find((v) => v !== "none") || null;
};
