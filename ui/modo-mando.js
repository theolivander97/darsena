// Modo mando: vista a pantalla completa que se maneja con mando o teclado.
// Se carga después de app.js y comparte sus variables y funciones (todo es global, como en app.js).

// ---------- Modo mando (pantalla completa, estilo Big Picture) ----------
// Se maneja con mando (cruceta o stick, Ⓐ, Ⓑ) o con el teclado (flechas, Intro, Esc).
// Se abre desde Biblioteca → Modo mando o pulsando a la vez Ⓢelect y Ⓢtart (o el botón central) en el mando.

const bpPlayed = (id) => playedOf(id);
const bp = { open: false, screen: "home", rows: [], r: 0, c: 0, game: null, btn: 0 };

function bpRows() {
  const games = libraryGames();
  const played = bpPlayed;
  const recent = games.filter((gm) => played(gm.id)?.lastPlayed).sort((a, b) => played(b.id).lastPlayed - played(a.id).lastPlayed).slice(0, 12);
  const installed = games.filter((gm) => installedPortOf(gm));
  const rest = games.filter((gm) => !installedPortOf(gm));
  return [
    { title: t("Seguir jugando"), games: recent },
    { title: t("Instalados"), games: installed },
    { title: t("Resto de tu biblioteca"), games: rest },
  ].filter((r) => r.games.length);
}

async function openBigPicture() {
  if (bp.open) return;
  bp.open = true;
  bp.screen = "home";
  bp.r = 0;
  bp.c = 0;
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
  bpRender();
  bpClock = setInterval(() => { const c = $("#bp-clock"); if (c) c.textContent = new Date().toLocaleTimeString(LOCALE(), { hour: "2-digit", minute: "2-digit" }); }, 10000);
}
let bpClock = null;

function closeBigPicture() {
  if (!bp.open) return;
  bp.open = false;
  $("#bigpic").hidden = true;
  document.body.classList.remove("bp-on");
  clearInterval(bpClock);
  invoke("set_fullscreen", { value: false }).catch(() => {});
  render();
}

function bpCard(gm, focused) {
  const cover = coverSrc(gm.id);
  const inst = installedPortOf(gm);
  return `<div class="bp-card ${focused ? "focus" : ""} ${inst ? "" : "dim"}" style="${coverStyle(gm)}">
    ${cover ? `<img src="${cover}" alt="">` : `<span>${esc(gm.title)}</span>`}
    ${state.running.has(inst?.id) ? `<em class="bp-run">${t("EJECUTANDO")}</em>` : ""}
  </div>`;
}

function bpRender() {
  const el = $("#bigpic");
  if (!el) return;
  const head = `<div class="bp-head"><img src="logo.svg" alt=""><b>DÁRSENA</b><span class="bp-mode">${t("MODO MANDO")}</span>
    <span class="bp-clock" id="bp-clock">${new Date().toLocaleTimeString(LOCALE(), { hour: "2-digit", minute: "2-digit" })}</span></div>`;
  const legend = (items) => `<div class="bp-legend">${items.map(([k, l]) => `<span><i class="bp-key">${k}</i>${l}</span>`).join("")}</div>`;
  if (bp.screen === "home") {
    bp.rows = bpRows();
    if (!bp.rows.length) {
      el.innerHTML = `${head}<div class="bp-empty"><h2>${t("Tu biblioteca está vacía")}</h2><p>${t("Añade juegos desde Explorar y aparecerán aquí.")}</p></div>${legend([["B", t("Salir del modo mando")]])}`;
      return;
    }
    bp.r = Math.min(bp.r, bp.rows.length - 1);
    bp.c = Math.min(bp.c, bp.rows[bp.r].games.length - 1);
    const focusGm = bp.rows[bp.r].games[bp.c];
    const hero = heroSrc(focusGm.id) || coverSrc(focusGm.id);
    el.innerHTML = `${hero ? `<img class="bp-bg" src="${hero}" alt="">` : ""}${head}
      <div class="bp-focus-title"><h1>${esc(focusGm.title)}</h1><div>${esc(consolesText(focusGm))}${bpPlayed(focusGm.id) ? ` · ${fmtHours(bpPlayed(focusGm.id).playtimeSecs)}` : ""}</div></div>
      <div class="bp-rows">${bp.rows.map((row, ri) => `<section class="bp-row ${ri === bp.r ? "active" : ""}"><h3>${esc(row.title)}</h3>
        <div class="bp-strip" data-row="${ri}">${row.games.map((gm, ci) => bpCard(gm, ri === bp.r && ci === bp.c)).join("")}</div></section>`).join("")}</div>
      ${legend([["A", t("Abrir")], ["B", t("Salir del modo mando")]])}`;
    el.querySelector(".bp-card.focus")?.scrollIntoView({ block: "nearest", inline: "center", behavior: "smooth" });
    el.querySelector(".bp-row.active")?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  } else {
    const gm = bp.game;
    const inst = installedPortOf(gm);
    const running = inst && state.running.has(inst.id);
    const p = bpPlayed(gm.id);
    bp.buttons = inst
      ? [[running ? t("EJECUTANDO") : t("JUGAR"), () => !running && playWithCloud(inst.id).then(() => bpRender())], [t("Volver"), () => bpBack()]]
      : [[t("Instalar en el escritorio"), () => { state.libGame = gm.id; closeBigPicture(); show("library"); }], [t("Volver"), () => bpBack()]];
    bp.btn = Math.min(bp.btn, bp.buttons.length - 1);
    const hero = heroSrc(gm.id) || coverSrc(gm.id);
    const logo = logoSrc(gm.id);
    el.innerHTML = `${hero ? `<img class="bp-bg sharp" src="${hero}" alt="">` : ""}${head}
      <div class="bp-game">
        ${logo ? `<img class="bp-logo" src="${logo}" alt="">` : `<h1>${esc(gm.title)}</h1>`}
        <div class="bp-meta">${esc(consolesText(gm))}${p ? ` · ${t("TIEMPO DE JUEGO")} ${fmtHours(p.playtimeSecs)}` : ""}${inst ? "" : ` · ${t("No instalado")}`}</div>
        <div class="bp-buttons">${bp.buttons.map(([l], i) => `<button class="bp-btn ${i === bp.btn ? "focus" : ""} ${i === 0 ? "primary" : ""}">${esc(l)}</button>`).join("")}</div>
      </div>
      ${legend([["A", t("Aceptar")], ["B", t("Atrás")]])}`;
  }
}
function bpBack() {
  if (bp.screen === "game") { bp.screen = "home"; bpRender(); }
  else closeBigPicture();
}

function bpInput(action) {
  if (!bp.open) return;
  if (bp.screen === "home") {
    if (!bp.rows.length) { if (action === "back") closeBigPicture(); return; }
    if (action === "left") bp.c = Math.max(0, bp.c - 1);
    else if (action === "right") bp.c = Math.min(bp.rows[bp.r].games.length - 1, bp.c + 1);
    else if (action === "up") bp.r = Math.max(0, bp.r - 1);
    else if (action === "down") bp.r = Math.min(bp.rows.length - 1, bp.r + 1);
    else if (action === "ok") { bp.game = bp.rows[bp.r].games[bp.c]; bp.screen = "game"; bp.btn = 0; }
    else if (action === "back") { closeBigPicture(); return; }
  } else {
    if (action === "left" || action === "up") bp.btn = Math.max(0, bp.btn - 1);
    else if (action === "right" || action === "down") bp.btn = Math.min(bp.buttons.length - 1, bp.btn + 1);
    else if (action === "ok") { bp.buttons[bp.btn][1](); return; }
    else if (action === "back") { bpBack(); return; }
  }
  bpRender();
}

// Teclado (también sirve para probarlo sin mando).
document.addEventListener("keydown", (e) => {
  if (!bp.open) return;
  const map = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down", Enter: "ok", " ": "ok", Escape: "back", Backspace: "back" };
  if (map[e.key]) { e.preventDefault(); e.stopPropagation(); bpInput(map[e.key]); }
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
  const state_ = {
    ok: b(0), back: b(1),
    up: b(12) || ax[1] < -0.6, down: b(13) || ax[1] > 0.6, left: b(14) || ax[0] < -0.6, right: b(15) || ax[0] > 0.6,
    toggle: b(16) || (b(8) && b(9)),
  };
  for (const [k, on] of Object.entries(state_)) {
    if (on && !pad.prev[k]) {
      pad.held[k] = now + 380;
      if (k === "toggle") (bp.open ? closeBigPicture : openBigPicture)();
      else bpInput(k);
    } else if (on && ["up", "down", "left", "right"].includes(k) && now >= pad.held[k]) {
      pad.held[k] = now + 110;
      bpInput(k);
    }
    pad.prev[k] = on;
  }
  pad.raf = requestAnimationFrame(padLoop);
}
window.addEventListener("gamepadconnected", () => {
  if (!pad.raf) pad.raf = requestAnimationFrame(padLoop);
  if (!bp.open) toast(t("Mando conectado: pulsa Select + Start para el modo mando"), "ok");
});
if ([...(navigator.getGamepads?.() || [])].some(Boolean)) pad.raf = requestAnimationFrame(padLoop);

// El estado de los juegos (EJECUTANDO) también se ve en el modo mando.
listen("game-state", () => { if (bp.open) bpRender(); });
