// Pantalla grande: vista a pantalla completa, pensada para el sofá, que se maneja con mando o teclado.
// Se carga después de app.js y comparte sus variables y funciones (todo es global, como en app.js).
//
// Estructura: un menú lateral con las secciones (Inicio, Biblioteca, Explorar, Comunidad, Descargas, Ajustes) y, en cada
// una, una cuadrícula de elementos enfocables. `bp.grid` es una lista de filas y cada fila una lista de elementos con su
// acción; el foco es (bp.r, bp.c). Mover el foco solo cambia clases del DOM: no se repinta la pantalla entera.
//
// Mando: cruceta o stick para moverse, A aceptar, B atrás, X jugar, Y menú, LB/RB cambiar de sección,
// Select + Start (o el botón central) para salir. Teclado: flechas, Intro, Esc, Espacio (jugar), F1 o Tab (menú), Q y E.

const BP_SECTIONS = [
  ["home", "Inicio", "home"],
  ["library", "Biblioteca", "library"],
  ["explore", "Explorar", "explore"],
  ["community", "Comunidad", "community"],
  ["downloads", "Descargas", "downloads"],
  ["settings", "Ajustes", "settings"],
];

const bpPlayed = (id) => playedOf(id);
const bp = { open: false, section: "home", detail: null, menu: false, menuIdx: 0, grid: [], r: 0, c: 0, saved: null, filter: "all", homeTab: "featured", poll: 0, clock: 0, pad: false };

const bpClock = () => new Date().toLocaleTimeString(LOCALE(), { hour: "2-digit", minute: "2-digit" });
const bpCols = () => Math.max(3, Math.floor((window.innerWidth - 140) / 220));
const bpArt = (gm) => heroSrc(gm.id) || coverSrc(gm.id);

// Sonidos cortos hechos con Web Audio (sin archivos): mover, aceptar, atrás, menú, cambiar de sección y error.
let bpAudio = null;
const bpSoundOn = () => { try { return localStorage.getItem("darsena.bpSound") !== "0"; } catch { return true; } };
const BP_TONES = {
  move: [[640, 0, 0.035]],
  select: [[520, 0, 0.06], [780, 0.06, 0.1]],
  back: [[560, 0, 0.06], [360, 0.06, 0.1]],
  menu: [[420, 0, 0.05], [620, 0.05, 0.05], [920, 0.1, 0.09]],
  section: [[700, 0, 0.05], [940, 0.05, 0.07]],
  error: [[190, 0, 0.14]],
};
function bpSound(kind) {
  if (!bpSoundOn()) return;
  try {
    bpAudio ||= new AudioContext();
    if (bpAudio.state === "suspended") bpAudio.resume();
    const t0 = bpAudio.currentTime;
    for (const [freq, at, dur] of BP_TONES[kind] || []) {
      const osc = bpAudio.createOscillator();
      const gain = bpAudio.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, t0 + at);
      gain.gain.exponentialRampToValueAtTime(0.07, t0 + at + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, t0 + at + dur);
      osc.connect(gain).connect(bpAudio.destination);
      osc.start(t0 + at);
      osc.stop(t0 + at + dur + 0.03);
    }
  } catch {}
}

// ---------- Abrir y cerrar ----------

async function openBigPicture() {
  if (bp.open) return;
  bp.open = true;
  bp.section = "home";
  bp.detail = null;
  bp.menu = false;
  bp.pad = [...(navigator.getGamepads?.() || [])].some(Boolean);
  await loadHomeProfile();
  let el = $("#bigpic");
  if (!el) {
    el = document.createElement("div");
    el.id = "bigpic";
    document.body.appendChild(el);
  }
  el.hidden = false;
  document.body.classList.add("bp-on");
  invoke("set_fullscreen", { value: true }).catch(() => {});
  bpSound("menu");
  el.innerHTML = `
    <div class="bp-bg"><img id="bp-bg-img" alt=""></div>
    <header class="bp-top">
      <div class="bp-brand"><img src="logo.svg" alt=""><b>DÁRSENA</b></div>
      <div class="bp-top-right">
        <span id="bp-clock">${bpClock()}</span>
        ${state.user?.avatarUrl ? `<img class="bp-avatar" src="${esc(state.user.avatarUrl)}" alt="">` : `<span class="bp-avatar guest">${icon("user")}</span>`}
      </div>
    </header>
    <nav class="bp-side" id="bp-side"></nav>
    <main id="bp-main"></main>
    <footer class="bp-legend" id="bp-legend"></footer>`;
  bp.clock = setInterval(() => { const c = $("#bp-clock"); if (c) c.textContent = bpClock(); }, 10000);
  bpGo("home");
}

function closeBigPicture() {
  if (!bp.open) return;
  bp.open = false;
  clearInterval(bp.clock);
  clearInterval(bp.poll);
  $("#bigpic").hidden = true;
  document.body.classList.remove("bp-on");
  invoke("set_fullscreen", { value: false }).catch(() => {});
  render();
}

// ---------- Pantallas ----------

function bpGo(section) {
  bp.section = section;
  bp.detail = null;
  bp.menu = false;
  bp.r = 0;
  bp.c = 0;
  clearInterval(bp.poll);
  bpRender();
  if (section === "downloads") bp.poll = setInterval(() => { if (bp.open && bp.section === "downloads" && !bp.detail) bpRender(true); }, 1000);
}

function bpRender(keepFocus = false) {
  const main = $("#bp-main");
  if (!main) return;
  const builders = { home: bpHome, library: bpLibrary, explore: bpExplore, community: bpCommunity, downloads: bpDownloads, settings: bpSettings };
  const view = bp.detail ? bpDetail(bp.detail) : builders[bp.section]();
  bp.grid = view.rows;
  main.className = view.hero ? "has-hero" : "";
  main.innerHTML = view.html;
  bp.r = Math.min(bp.r, Math.max(0, bp.grid.length - 1));
  bp.c = Math.min(bp.c, Math.max(0, (bp.grid[bp.r]?.items.length ?? 1) - 1));
  bpRenderSide();
  bpRenderLegend();
  bpFocus(!keepFocus);
  if (view.after) view.after();
}

function bpRenderSide() {
  const side = $("#bp-side");
  side.classList.toggle("open", bp.menu);
  side.innerHTML = [...BP_SECTIONS.map(([id, label, ico], i) =>
    `<div class="bp-side-item ${bp.section === id ? "current" : ""} ${bp.menu && bp.menuIdx === i ? "focus" : ""}">${icon(ico)}<span>${t(label)}</span></div>`),
    `<div class="bp-side-item exit ${bp.menu && bp.menuIdx === BP_SECTIONS.length ? "focus" : ""}">${icon("close")}<span>${t("Salir de Pantalla grande")}</span></div>`].join("");
}

// Pie con las teclas: las del teclado si no hay mando, o los botones del mando (pequeños) si lo hay.
function bpRenderLegend() {
  const kb = !bp.pad;
  const key = (k) => `<i class="bp-key ${kb ? "kb" : k.length > 1 ? "pad sq" : "pad"}">${k}</i>`;
  const item = ([k, l]) => `<span>${key(k)}${l}</span>`;
  const K = kb ? { ok: "Intro", back: "Esc", play: "Espacio", menu: "F1", prev: "Q", next: "E" } : { ok: "A", back: "B", play: "X", menu: "Y", prev: "LB", next: "RB" };
  const left = bp.menu ? [] : [[K.menu, t("MENÚ")]];
  const right = bp.menu ? [[K.ok, t("IR A")], [K.back, t("CERRAR")]]
    : bp.detail ? [[K.ok, t("ACEPTAR")], [K.back, t("ATRÁS")]]
    : [[K.play, t("JUGAR")], [K.ok, t("SELECCIONAR")], [K.back, t("ATRÁS")]];
  $("#bp-legend").innerHTML = `<div>${left.map(item).join("")}</div><div>${bp.menu ? "" : `<span>${key(K.prev)}${key(K.next)}${t("SECCIÓN")}</span>`}${right.map(item).join("")}</div>`;
}

// Tarjetas
function bpCard(gm, r, c, wide = false) {
  const inst = installedPortOf(gm);
  const p = bpPlayed(gm.id);
  const img = wide ? (heroSrc(gm.id) || coverSrc(gm.id)) : coverSrc(gm.id);
  const running = inst && state.running.has(inst.id);
  return `<div class="bp-card ${wide ? "wide" : ""} ${inst || !isOwned(gm.id) ? "" : "not-installed"}" data-r="${r}" data-c="${c}" style="${coverStyle(gm)}">
    ${img ? `<img src="${img}" alt="" loading="lazy">` : `<span class="bp-card-name">${esc(gm.title)}</span>`}
    ${wide && logoSrc(gm.id) ? `<img class="bp-card-logo" src="${logoSrc(gm.id)}" alt="" onerror="this.remove()">` : ""}
    <div class="bp-card-info"><b>${esc(gm.title)}</b>${p ? `<span>${fmtHours(p.playtimeSecs)}</span>` : ""}</div>
    ${running ? `<em class="bp-run">${t("EJECUTANDO")}</em>` : ""}
  </div>`;
}

function bpRowHtml(row, r) {
  return `<section class="bp-row ${row.center ? "chips-row" : ""}" data-row="${r}">${row.title ? `<h3>${esc(row.title)}</h3>` : ""}
    <div class="bp-strip ${row.tall ? "tall" : ""}">${row.items.map((it, c) => it.html ? it.html(r, c) : bpCard(it.gm, r, c, it.wide ?? row.wide)).join("")}</div></section>`;
}

const bpOpenGame = (gm) => { bp.saved = { r: bp.r, c: bp.c }; bp.detail = gm; bp.r = 0; bp.c = 0; bpRender(); };
const bpGameItems = (games) => games.map((gm) => ({ gm, run: () => bpOpenGame(gm) }));

function bpHeroHtml() {
  return `<div class="bp-hero"><div class="bp-hero-text"><div class="bp-hero-console" id="bp-h-meta"></div><h1 id="bp-h-title"></h1><p id="bp-h-syn"></p></div></div>`;
}

// Tarjeta de noticia: fecha arriba, imagen y título debajo (como las de Novedades de Steam).
function bpNewsItem(gm, when) {
  return {
    gm,
    run: () => bpOpenGame(gm),
    html: (r, c) => `<div class="bp-news" data-r="${r}" data-c="${c}"><span class="bp-news-when">${esc(when)}</span>
      <div class="bp-news-card">${bpArt(gm) ? `<img src="${bpArt(gm)}" alt="" loading="lazy">` : ""}<div class="bp-news-title">${esc(gm.title)}</div></div></div>`,
  };
}

// Pestañas de la parte baja del inicio: destacados, añadidos hace poco y más descargados.
function bpHomeTabs() {
  const all = state.gameList;
  const lists = {
    featured: featuredGames().map((gm) => bpNewsItem(gm, t("Destacado"))),
    recent: [...all].filter((gm) => gm.addedAt).sort((a, b) => b.addedAt.localeCompare(a.addedAt)).slice(0, 16)
      .map((gm) => bpNewsItem(gm, whenLabel(Date.parse(gm.addedAt)))),
    popular: [...all].filter((gm) => gm.downloads).sort((a, b) => b.downloads - a.downloads).slice(0, 16)
      .map((gm) => bpNewsItem(gm, `${(gm.downloads / 1000).toFixed(0)} K ${t("descargas")}`)),
  };
  const tabs = [["featured", "Destacados"], ["recent", "Novedades"], ["popular", "Más descargados"]];
  const chips = {
    center: true,
    items: tabs.map(([id, label], c) => ({
      html: (r, i) => `<div class="bp-chip ${bp.homeTab === id ? "on" : ""}" data-r="${r}" data-c="${i}">${t(label)}</div>`,
      run: () => { bp.homeTab = id; bp.c = c; bpRender(true); },
    })),
  };
  return [chips, { items: lists[bp.homeTab] }];
}

// Inicio: lo último que jugaste (el primero, grande) y, debajo, pestañas con juegos del catálogo
function bpHome() {
  const games = libraryGames();
  const played = (gm) => bpPlayed(gm.id)?.lastPlayed || 0;
  const recent = games.filter((gm) => played(gm)).sort((a, b) => played(b) - played(a)).slice(0, 12);
  const installed = games.filter((gm) => installedPortOf(gm) && !recent.includes(gm));
  const top = recent.length ? bpGameItems(recent).map((it, i) => ({ ...it, wide: i === 0 })) : bpGameItems(installed);
  const rows = [];
  if (top.length) rows.push({ title: recent.length ? t("Jugados recientemente") : t("Instalados"), tall: true, items: top });
  rows.push(...bpHomeTabs());
  const [chips, news] = rows.slice(-2);
  const lead = rows.length > 2 ? rows[0] : null;
  const html = `<div class="bp-rows bp-home">${lead ? bpRowHtml(lead, 0) : ""}${bpRowHtml(chips, lead ? 1 : 0)}${bpRowHtml(news, lead ? 2 : 1)}</div>`;
  return { rows, html };
}

// Biblioteca: filtros y cuadrícula
function bpLibrary() {
  const filters = [["all", "Todos"], ["installed", "Instalados"], ["fav", "Favoritos"]];
  let games = libraryGames();
  if (bp.filter === "installed") games = games.filter((gm) => installedPortOf(gm));
  if (bp.filter === "fav") games = games.filter((gm) => isFavorite(gm.id));
  const chips = {
    items: filters.map(([id, label]) => ({
      html: (r, c) => `<div class="bp-chip ${bp.filter === id ? "on" : ""}" data-r="${r}" data-c="${c}">${t(label)}</div>`,
      run: () => { bp.filter = id; bp.r = 0; bp.c = filters.findIndex((f) => f[0] === id); bpRender(true); },
    })),
  };
  const rows = [chips];
  const cols = bpCols();
  for (let i = 0; i < games.length; i += cols) rows.push({ items: bpGameItems(games.slice(i, i + cols)) });
  const body = games.length ? `<div class="bp-rows">${rows.slice(1).map((row, i) => bpRowHtml(row, i + 1)).join("")}</div>`
    : `<p class="bp-note">${t("No hay juegos en este filtro.")}</p>`;
  return { rows, html: `<h2 class="bp-title">${t("Biblioteca")} <span>${games.length}</span></h2><div class="bp-chips">${chips.items.map((it, c) => it.html(0, c)).join("")}</div>${body}` };
}

// Explorar: filas del catálogo
function bpExplore() {
  const all = state.gameList;
  const byDl = [...all].filter((gm) => gm.downloads).sort((a, b) => b.downloads - a.downloads).slice(0, 20);
  const recent = [...all].filter((gm) => gm.addedAt).sort((a, b) => b.addedAt.localeCompare(a.addedAt)).slice(0, 20);
  const rows = [
    { title: t("Destacados"), tall: true, items: bpGameItems(featuredGames()).map((it, i) => ({ ...it, wide: i === 0 })) },
    { title: t("Más descargados"), items: bpGameItems(byDl) },
    { title: t("Añadidos recientemente"), items: bpGameItems(recent) },
  ];
  for (const c of ["PlayStation 2", "Nintendo 64", "GameCube", "Xbox 360", "PSP", "Super Nintendo"]) {
    const list = all.filter((gm) => consolesOf(gm).includes(c)).sort((a, b) => (b.downloads || 0) - (a.downloads || 0)).slice(0, 20);
    if (list.length >= 6) rows.push({ title: consoleName(c), items: bpGameItems(list) });
  }
  return { rows, html: `<div class="bp-rows">${rows.map(bpRowHtml).join("")}</div>` };
}

// Comunidad: mods y reseñas (se rellena cuando llegan los datos)
function bpCommunity() {
  const view = { rows: [], html: `<h2 class="bp-title">${t("Comunidad")}</h2><div class="bp-rows" id="bp-com"><p class="bp-note">${t("Cargando…")}</p></div>` };
  view.after = async () => {
    const [mods, reviews] = await Promise.all([loadMods(), invoke("recent_reviews").catch(() => [])]);
    if (!bp.open || bp.section !== "community" || bp.detail) return;
    const rows = [];
    const modItems = visibleMods(mods).filter((m) => state.games.get(m.game)).slice(0, 12).map((m) => ({
      html: (r, c) => `<div class="bp-note-card" data-r="${r}" data-c="${c}"><b>${esc(m.name)}</b><span>${esc(state.games.get(m.game).title)} · ${t("por {author}", { author: esc(m.author) })}</span><em>${m.likes || 0} ${t("Me gusta")}</em></div>`,
      run: () => bpOpenGame(state.games.get(m.game)),
    }));
    if (modItems.length) rows.push({ title: t("Mods populares"), items: modItems });
    const revItems = reviews.filter((r) => state.games.get(r.game)).slice(0, 12).map((r) => ({
      html: (ri, c) => `<div class="bp-note-card" data-r="${ri}" data-c="${c}"><b>${esc(state.games.get(r.game).title)}</b><span class="stars">${"★".repeat(r.rating)}${"☆".repeat(5 - r.rating)}</span><span>${esc((r.text || "").slice(0, 110))}</span><em>${esc(r.user)}</em></div>`,
      run: () => bpOpenGame(state.games.get(r.game)),
    }));
    if (revItems.length) rows.push({ title: t("Reseñas recientes"), items: revItems });
    bp.grid = rows;
    $("#bp-com").innerHTML = rows.length ? rows.map(bpRowHtml).join("") : `<p class="bp-note">${t("Aún no hay actividad en la comunidad.")}</p>`;
    bp.r = 0;
    bp.c = 0;
    bpFocus(false);
  };
  return view;
}

// Descargas: una fila por descarga
function bpDownloads() {
  const list = state.downloads;
  const rows = list.map((d) => ({
    items: [{
      html: (r, c) => {
        const gm = gameOfTask(d);
        const pct = d.total ? (d.downloaded / d.total) * 100 : 0;
        return `<div class="bp-dl ${d.status}" data-r="${r}" data-c="${c}">
          <div class="bp-dl-art">${gm && coverSrc(gm.id) ? `<img src="${coverSrc(gm.id)}" alt="">` : ""}</div>
          <div class="bp-dl-body"><b>${esc(taskTitle(d.title))}</b>
            <div class="bar ${barClass(d)}"><div style="width:${d.status === "completed" ? 100 : pct}%"></div></div>
            <span>${esc(dlInfo(d))}</span></div>
          <div class="bp-dl-act">${d.status === "completed" ? t("Jugar") : d.status === "paused" ? t("Reanudar") : d.status === "downloading" ? t("Pausar") : ""}</div>
        </div>`;
      },
      run: async () => {
        if (d.status === "completed" && !d.key.startsWith("data:")) await playWithCloud(d.key);
        else if (d.status === "paused") await call("resume_download", { id: d.key });
        else if (d.status === "downloading") await call("pause_download", { id: d.key });
      },
    }],
  }));
  const body = list.length ? rows.map((row, r) => bpRowHtml(row, r)).join("") : `<p class="bp-note">${t("No hay descargas.")}</p>`;
  return { rows, html: `<h2 class="bp-title">${t("Descargas")}</h2><div class="bp-rows bp-list">${body}</div>` };
}

// Ajustes: unos pocos interruptores
function bpSettings() {
  const s = state.settings;
  const toggle = (label, get, set) => ({
    items: [{
      html: (r, c) => `<div class="bp-setting" data-r="${r}" data-c="${c}"><span>${t(label)}</span><i class="bp-switch ${get() ? "on" : ""}"></i></div>`,
      run: async () => { await set(); bpRender(true); },
    }],
  });
  const rows = [
    toggle("Actualizar los ports al abrir Dársena", () => s.autoUpdatePorts !== false, async () => {
      s.autoUpdatePorts = s.autoUpdatePorts === false;
      await call("set_auto_update_ports", { value: s.autoUpdatePorts });
    }),
    toggle("Partidas en la nube", () => s.cloudSaves !== false, async () => {
      s.cloudSaves = s.cloudSaves === false;
      await call("set_cloud_saves", { value: s.cloudSaves });
    }),
    toggle("Sonidos de Pantalla grande", bpSoundOn, async () => {
      try { localStorage.setItem("darsena.bpSound", bpSoundOn() ? "0" : "1"); } catch {}
    }),
    toggle("Seguir en la bandeja al cerrar la ventana", () => s.closeToTray !== false, async () => {
      s.closeToTray = s.closeToTray === false;
      await call("set_close_to_tray", { value: s.closeToTray });
    }),
    { items: [{ html: (r, c) => `<div class="bp-setting" data-r="${r}" data-c="${c}"><span>${t("Salir de Pantalla grande")}</span></div>`, run: () => closeBigPicture() }] },
  ];
  return { rows, html: `<h2 class="bp-title">${t("Ajustes")}</h2><div class="bp-rows bp-list">${rows.map((row, r) => bpRowHtml(row, r)).join("")}</div>` };
}

function bpEmpty(title, text) {
  return { rows: [], html: `<div class="bp-empty"><img src="logo.svg" alt=""><h2>${title}</h2><p>${text}</p></div>` };
}

// Ficha de un juego
function bpDetail(gm) {
  const inst = installedPortOf(gm);
  const port = inst ? state.byId.get(inst.id) : sortedPorts(gm).find((p) => !isExternal(p)) || sortedPorts(gm)[0];
  const running = inst && state.running.has(inst.id);
  const p = bpPlayed(gm.id);
  const syn = synopsisOf(gm.id);
  const logo = logoSrc(gm.id);
  const buttons = [];
  if (inst) {
    buttons.push([running ? t("EJECUTANDO") : t("JUGAR"), async () => { if (!running) { await playWithCloud(inst.id); bpRender(true); } }, true]);
    buttons.push([isFavorite(gm.id) ? t("Quitar de favoritos") : t("Añadir a favoritos"), async () => { await toggleFavorite(gm.id); bpRender(true); }]);
  } else if (isOwned(gm.id)) {
    buttons.push([t("INSTALAR"), () => bpInstall(gm, port), true]);
  } else {
    buttons.push([t("AÑADIR A LA BIBLIOTECA"), async () => {
      await call("add_to_library", { game: gm.id });
      [state.library, state.owned] = await Promise.all([invoke("get_library"), invoke("get_owned")]);
      bpRender(true);
    }, true]);
  }
  buttons.push([t("Volver"), () => bpBack()]);
  const row = {
    items: buttons.map(([label, run, primary], c) => ({
      run,
      html: (r) => `<button class="bp-btn ${primary ? "primary" : ""}" data-r="${r}" data-c="${c}">${esc(label)}</button>`,
    })),
  };
  return {
    rows: [row],
    html: `<div class="bp-detail">
      <div class="bp-detail-body">
        ${logo ? `<img class="bp-logo" src="${logo}" alt="">` : `<h1>${esc(gm.title)}</h1>`}
        <div class="bp-meta">${esc(consolesText(gm))}${port ? ` · ${esc(port.port)} ${t("por {author}", { author: esc(port.author) })}` : ""}</div>
        <div class="bp-actions">${row.items.map((it) => it.html(0)).join("")}</div>
        <div class="bp-stats">
          <div><span>${t("TIEMPO DE JUEGO")}</span><b>${p ? fmtHours(p.playtimeSecs) : "—"}</b></div>
          <div><span>${t("ÚLTIMA SESIÓN")}</span><b>${p?.lastPlayed ? fmtIsoDate(p.lastPlayed * 1000) : t("Nunca")}</b></div>
          <div><span>${t("Estado")}</span><b>${inst ? t("Instalado") : isOwned(gm.id) ? t("En tu biblioteca") : t("No está en tu biblioteca")}</b></div>
        </div>
        ${syn ? `<p class="bp-syn">${esc(syn.text.length > 420 ? syn.text.slice(0, 417) + "…" : syn.text)}</p>` : ""}
      </div>
    </div>`,
  };
}

async function bpInstall(gm, port) {
  if (!port || isExternal(port)) { toast(t("Este port se instala desde fuentes: hazlo desde la vista de escritorio"), "error"); return; }
  try {
    await call("install_game", { id: port.id, source: null, installDir: state.settings.installDir, data: null });
    toast(t("Instalando…"), "ok");
    state.downloads = await invoke("list_downloads");
    bpGo("downloads");
  } catch {}
}

function bpBack() {
  if (bp.detail) {
    const s = bp.saved;
    bp.detail = null;
    bp.r = s?.r ?? 0;
    bp.c = s?.c ?? 0;
    bpRender();
  } else {
    // Atrás desde una pantalla principal no sale del modo: abre el menú (como Esc en Steam). Desde ahí se sale.
    bp.menu = true;
    bp.menuIdx = Math.max(0, BP_SECTIONS.findIndex((s) => s[0] === bp.section));
    bpRenderSide();
    bpRenderLegend();
  }
}

// ---------- Foco ----------

// Marca el elemento enfocado, lo trae a la vista y actualiza la cabecera y el fondo.
function bpFocus(smooth = true) {
  const main = $("#bp-main");
  if (!main) return;
  main.querySelectorAll(".focus").forEach((x) => x.classList.remove("focus"));
  const el = main.querySelector(`[data-r="${bp.r}"][data-c="${bp.c}"]`);
  const item = bp.grid[bp.r]?.items[bp.c];
  const gm = item?.gm || bp.detail;
  const bg = $("#bp-bg-img");
  const art = gm ? bpArt(gm) : "";
  if (bg && bg.dataset.src !== art) {
    bg.dataset.src = art;
    bg.style.opacity = 0;
    if (art) { bg.src = art; bg.onload = () => { bg.style.opacity = 1; }; }
  }
  if (!el) return;
  el.classList.add("focus");
  const behavior = smooth ? "smooth" : "auto";
  const strip = el.closest(".bp-strip");
  if (strip) strip.scrollTo({ left: Math.max(0, el.offsetLeft - strip.offsetLeft - 48), behavior });
  const rows = el.closest(".bp-rows");
  const section = el.closest(".bp-row");
  if (rows && section) rows.scrollTo({ top: Math.max(0, section.offsetTop - rows.offsetTop - 28), behavior });
  if (item?.gm && $("#bp-h-title")) {
    const syn = synopsisOf(item.gm.id);
    const inst = installedPortOf(item.gm);
    $("#bp-h-meta").textContent = `${consolesText(item.gm)}${inst ? ` · ${t("Instalado")}` : ""}`;
    $("#bp-h-title").textContent = item.gm.title;
    $("#bp-h-syn").textContent = syn ? (syn.text.length > 200 ? syn.text.slice(0, 197) + "…" : syn.text) : "";
  }
}

function bpInput(action) {
  if (!bp.open) return;
  if (action === "menu") {
    bpSound("menu");
    bp.menu = !bp.menu;
    bp.menuIdx = Math.max(0, BP_SECTIONS.findIndex((s) => s[0] === bp.section));
    bpRenderSide();
    bpRenderLegend();
    return;
  }
  if (action === "prev" || action === "next") {
    const i = BP_SECTIONS.findIndex((s) => s[0] === bp.section);
    bpSound("section");
    bpGo(BP_SECTIONS[(i + (action === "next" ? 1 : BP_SECTIONS.length - 1)) % BP_SECTIONS.length][0]);
    return;
  }
  if (bp.menu) {
    const last = BP_SECTIONS.length;
    const before = bp.menuIdx;
    if (action === "up") bp.menuIdx = Math.max(0, bp.menuIdx - 1);
    else if (action === "down") bp.menuIdx = Math.min(last, bp.menuIdx + 1);
    else if (action === "back") { bpSound("back"); bp.menu = false; }
    else if (action === "ok") { bpSound("select"); if (bp.menuIdx === last) closeBigPicture(); else bpGo(BP_SECTIONS[bp.menuIdx][0]); return; }
    if (bp.menuIdx !== before) bpSound("move");
    bpRenderSide();
    bpRenderLegend();
    return;
  }
  if (action === "back") { bpSound(bp.detail ? "back" : "menu"); bpBack(); return; }
  const row = bp.grid[bp.r];
  if (!row) return;
  if (action === "play") {
    const gm = row.items[bp.c]?.gm || bp.detail;
    const inst = gm && installedPortOf(gm);
    if (inst && !state.running.has(inst.id)) { bpSound("select"); playWithCloud(inst.id).then(() => bpRender(true)); } else bpSound("error");
    return;
  }
  if (action === "ok") { bpSound("select"); row.items[bp.c]?.run?.(); return; }
  const [r0, c0] = [bp.r, bp.c];
  if (action === "left") bp.c = Math.max(0, bp.c - 1);
  else if (action === "right") bp.c = Math.min(row.items.length - 1, bp.c + 1);
  else if (action === "up" || action === "down") {
    const r = Math.min(bp.grid.length - 1, Math.max(0, bp.r + (action === "down" ? 1 : -1)));
    if (r === bp.r) return;
    bp.r = r;
    bp.c = Math.min(bp.c, bp.grid[r].items.length - 1);
  }
  if (bp.r !== r0 || bp.c !== c0) bpSound("move");
  bpFocus();
}

// ---------- Entradas ----------

// Teclado (también sirve para probarlo sin mando).
document.addEventListener("keydown", (e) => {
  if (!bp.open) return;
  const map = {
    ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down", Enter: "ok", " ": "play",
    Escape: "back", Backspace: "back", F1: "menu", Tab: "menu", q: "prev", Q: "prev", e: "next", E: "next",
  };
  if (map[e.key]) {
    e.preventDefault();
    e.stopPropagation();
    if (bp.pad) { bp.pad = false; bpRenderLegend(); }
    bpInput(map[e.key]);
  }
}, true);

// Mando: se lee el estado cada fotograma (la API Gamepad no da eventos de botones). Repetición al mantener pulsado.
const pad = { prev: {}, held: {}, raf: 0 };
function padLoop() {
  pad.raf = 0;
  const gp = [...(navigator.getGamepads?.() || [])].find(Boolean);
  if (!gp) return;
  const b = (i) => !!gp.buttons[i]?.pressed;
  const ax = gp.axes || [];
  const now = performance.now();
  const on = {
    ok: b(0), back: b(1), play: b(2), menu: b(3), prev: b(4), next: b(5),
    up: b(12) || ax[1] < -0.6, down: b(13) || ax[1] > 0.6, left: b(14) || ax[0] < -0.6, right: b(15) || ax[0] > 0.6,
    toggle: b(16) || (b(8) && b(9)),
  };
  for (const [k, down] of Object.entries(on)) {
    if (down && !pad.prev[k]) {
      if (!bp.pad) { bp.pad = true; if (bp.open) bpRenderLegend(); }
      pad.held[k] = now + 380;
      if (k === "toggle") (bp.open ? closeBigPicture : openBigPicture)();
      else bpInput(k);
    } else if (down && ["up", "down", "left", "right"].includes(k) && now >= pad.held[k]) {
      pad.held[k] = now + 110;
      bpInput(k);
    }
    pad.prev[k] = down;
  }
  pad.raf = requestAnimationFrame(padLoop);
}
window.addEventListener("gamepadconnected", () => {
  if (!pad.raf) pad.raf = requestAnimationFrame(padLoop);
  bp.pad = true;
  if (bp.open) bpRenderLegend();
  if (!bp.open) toast(t("Mando conectado: pulsa Select + Start para abrir Pantalla grande"), "ok");
});
if ([...(navigator.getGamepads?.() || [])].some(Boolean)) pad.raf = requestAnimationFrame(padLoop);

// El estado de los juegos (EJECUTANDO) y la biblioteca también se ven en Pantalla grande.
listen("game-state", () => { if (bp.open) bpRender(true); });
listen("library-changed", () => { if (bp.open && !bp.detail && bp.section !== "downloads") setTimeout(() => bp.open && bpRender(true), 400); });
