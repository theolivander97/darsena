// Taller (mods que sube la comunidad) y pestaña Comunidad.
// Se carga después de app.js y comparte sus variables y funciones (todo es global, como en app.js).

// ---------- Taller (mods de la comunidad) ----------

// Mods publicados (mods.json). Los ocultos por denuncias no se muestran; los +18 solo si se activan en Ajustes.
async function loadMods(force = false) {
  if (!force && state.mods && Date.now() - state.modsAt < 120000) return state.mods;
  state.mods = await invoke("list_mods", { force }).catch(() => state.mods || []);
  state.modsAt = Date.now();
  return state.mods;
}
const showNsfw = () => !!state.settings?.showNsfw;
// Orden: los normales por «me gusta»; los +18 siempre al final (y solo si están activados).
const visibleMods = (list) => list.filter((m) => !m.hidden && (!m.nsfw || showNsfw()))
  .sort((a, b) => (a.nsfw - b.nsfw) || (b.likes - a.likes) || b.createdAt.localeCompare(a.createdAt));

// Ports instalados de un juego donde puede ir el mod.
const modTargets = (m) => state.library.filter((g) => gameOfPort(g.id) === m.game && (!m.port || m.port === g.id));
const modInstalledIn = (m) => state.library.find((g) => g.mods?.some((x) => x.id === m.id));

function modCardHtml(m, withGame = false) {
  const gm = state.games.get(m.game);
  const inst = modInstalledIn(m);
  const targets = modTargets(m);
  const busy = state.modBusy?.has(m.id);
  const action = busy ? `<button class="btn" disabled>${t("Instalando…")}</button>`
    : inst ? `<button class="btn ghost" data-mod-uninstall="${esc(m.id)}" data-mod-port="${esc(inst.id)}">${icon("trash")}${t("Quitar")}</button>`
    : targets.length ? `<button class="btn primary" data-mod-install="${esc(m.id)}" data-mod-port="${esc(targets[0].id)}">${icon("downloads")}${t("Instalar")}</button>`
    : `<button class="btn" disabled title="${t("Instala antes el juego")}">${t("Instala antes el juego")}</button>`;
  const forPort = m.port ? state.byId.get(m.port)?.port : null;
  const desc = (m.description || "").split("\n").slice(0, 4).join("\n");
  return `<div class="mod-card ${m.nsfw ? "is-nsfw" : ""}">
    ${withGame && gm ? `<div class="mod-cover" data-open="${esc(gm.id)}" title="${esc(gm.title)}">${coverImg(gm, "mod-cover-img") || `<span>${esc(gm.title)}</span>`}</div>` : ""}
    <div class="mod-main">
      <div class="mod-name">${esc(m.name)} ${m.nsfw ? `<span class="tag nsfw">+18</span>` : ""}</div>
      <div class="muted small">${withGame && gm ? `${esc(gm.title)} · ` : ""}${t("por {author}", { author: esc(m.author) })}${forPort ? ` · ${t("para {port}", { port: esc(forPort) })}` : ""}${inst ? ` · <b class="ok-text">${t("Instalado")}</b>` : ""}</div>
      ${desc ? `<div class="mod-desc">${mdToHtml(desc)}</div>` : ""}
    </div>
    <div class="mod-actions">
      ${action}
      <button class="btn ghost small" data-mod-like="${esc(m.node)}" title="${t("Me gusta")}">${t("Me gusta")} · ${m.likes || 0}</button>
      <button class="btn ghost small" data-url="https://github.com/${esc(m.repo)}" title="${t("Ver en GitHub")}">${icon("github")}</button>
      <button class="btn ghost small" data-mod-report="${esc(m.node)}" title="${t("Denunciar")}">${t("Denunciar")}</button>
    </div>
  </div>`;
}

// Sección «Taller» de la ficha de un juego (en Explorar y en la biblioteca).
async function renderWorkshopFor(gameId) {
  if (!document.querySelector(`.workshop-box[data-game="${CSS.escape(gameId)}"]`)) return;
  const all = (await loadMods()).filter((m) => m.game === gameId);
  const list = visibleMods(all);
  const hiddenNsfw = all.filter((m) => !m.hidden && m.nsfw && !showNsfw()).length;
  const html = `<h2 class="section-title">${t("Taller")} <span class="muted small">· ${t("mods de la comunidad")}</span>
      <button class="btn ghost small ws-upload" data-mod-upload="${esc(gameId)}">${icon("plus")}${t("Subir un mod")}</button></h2>
    ${list.length ? `<div class="mod-list">${list.map((m) => modCardHtml(m)).join("")}</div>`
      : `<p class="muted">${t("Aún no hay mods para este juego. ¿Has hecho uno? Súbelo al Taller.")}</p>`}
    ${hiddenNsfw ? `<p class="muted small">${t("{n} mods +18 ocultos.", { n: hiddenNsfw })} <a class="link" data-goto-settings-nsfw>${t("Mostrarlos")}</a></p>` : ""}`;
  for (const b of document.querySelectorAll(`.workshop-box[data-game="${CSS.escape(gameId)}"]`)) b.innerHTML = html;
}

function repaintWorkshop() {
  for (const b of document.querySelectorAll(".workshop-box[data-game]")) renderWorkshopFor(b.dataset.game);
  if (state.view === "community") renderCommunity();
}

document.addEventListener("click", async (e) => {
  const ins = e.target.closest("[data-mod-install]");
  const un = e.target.closest("[data-mod-uninstall]");
  const like = e.target.closest("[data-mod-like]");
  const rep = e.target.closest("[data-mod-report]");
  const up = e.target.closest("[data-mod-upload]");
  if (e.target.closest("[data-goto-settings-nsfw]")) { openSettings("general"); requestAnimationFrame(() => $("#show-nsfw")?.scrollIntoView({ block: "center" })); return; }
  if (up) { openModUpload(up.dataset.modUpload || null); return; }
  if (ins) {
    const id = ins.dataset.modInstall;
    (state.modBusy ||= new Set()).add(id);
    repaintWorkshop();
    try {
      await call("install_mod", { portId: ins.dataset.modPort, modId: id });
      state.library = await invoke("get_library");
      toast(t("Mod instalado"), "ok");
    } catch {}
    state.modBusy.delete(id);
    repaintWorkshop();
    return;
  }
  if (un) {
    try {
      await call("uninstall_mod", { portId: un.dataset.modPort, modId: un.dataset.modUninstall });
      state.library = await invoke("get_library");
      toast(t("Mod quitado"), "ok");
    } catch {}
    repaintWorkshop();
    return;
  }
  if (like) {
    if (!state.user) { toast(t("Inicia sesión con GitHub para dar «me gusta»"), "error"); return; }
    like.disabled = true;
    try {
      await call("like_mod", { node: like.dataset.modLike });
      const m = state.mods?.find((x) => x.node === like.dataset.modLike);
      if (m && !m.likedHere) { m.likes = (m.likes || 0) + 1; m.likedHere = true; }
      repaintWorkshop();
    } catch { like.disabled = false; }
    return;
  }
  if (rep) {
    if (!state.user) { toast(t("Inicia sesión con GitHub para denunciar un mod"), "error"); return; }
    openModReport(rep.dataset.modReport);
  }
});

function openModReport(node) {
  $("#mod-dialog")?.remove();
  const box = document.createElement("div");
  box.id = "mod-dialog";
  box.className = "news-modal";
  box.innerHTML = `<div class="news-dialog small-dialog">
      <div class="news-head"><h2>${t("Denunciar mod")}</h2>
        <div class="muted small">${t("Con varias denuncias, el mod se oculta hasta que se revise.")}</div></div>
      <div class="news-body ed-form">
        <label>${t("Motivo")}<select id="rep-reason">
          <option value="nsfw">${t("Contenido +18 sin marcar")}</option>
          <option value="spam">${t("Spam o engañoso")}</option>
          <option value="malware">${t("Malware o archivos peligrosos")}</option>
          <option value="other">${t("Otro motivo")}</option>
        </select></label>
        <label>${t("Detalles (opcional)")}<textarea id="rep-text" class="short"></textarea></label>
      </div>
      <div class="news-foot"><button class="btn primary" id="rep-send">${t("Enviar denuncia")}</button></div>
    </div>
    <div class="news-side"><button class="news-round" data-dlg-close title="${t("Cerrar")}">${icon("close")}</button></div>`;
  document.body.appendChild(box);
  box.addEventListener("click", async (e) => {
    if (e.target === box || e.target.closest("[data-dlg-close]")) { box.remove(); return; }
    const send = e.target.closest("#rep-send");
    if (!send) return;
    send.disabled = true;
    try {
      await call("report_mod", { node, reason: $("#rep-reason").value, text: $("#rep-text").value });
      toast(t("Denuncia enviada. Gracias."), "ok");
      box.remove();
    } catch { send.disabled = false; }
  });
}

// Subir un mod: repositorio con releases, juego (y port opcional), descripción, carpeta y +18.
function openModUpload(gameId) {
  if (!state.user) { toast(t("Inicia sesión con GitHub para subir un mod"), "error"); return; }
  $("#mod-dialog")?.remove();
  const box = document.createElement("div");
  box.id = "mod-dialog";
  box.className = "news-modal";
  const games = [...state.games.values()].sort((a, b) => a.title.localeCompare(b.title, "es"));
  box.innerHTML = `<div class="news-dialog editor">
      <div class="news-head"><span class="news-kind">${t("TALLER")}</span><h2>${t("Subir un mod")}</h2>
        <div class="muted small">${t("Tu mod debe estar en un repositorio de GitHub del que seas autor, con una release que incluya un .zip. Dársena lo extrae en la carpeta del port.")}</div></div>
      <div class="news-body ed-form">
        <label>${t("Repositorio (usuario/repositorio)")}<input id="mod-repo" type="text" list="mod-repos" placeholder="usuario/mi-mod"><datalist id="mod-repos"></datalist></label>
        <div class="ed-grid">
          <label>${t("Juego")}<select id="mod-game">${games.map((g) => `<option value="${esc(g.id)}" ${g.id === gameId ? "selected" : ""}>${esc(g.title)}</option>`).join("")}</select></label>
          <label>${t("Port")}<select id="mod-port"></select></label>
        </div>
        <label>${t("Nombre del mod")}<input id="mod-name" type="text" maxlength="80"></label>
        <label>${t("Descripción (admite Markdown sencillo)")}<textarea id="mod-desc"></textarea></label>
        <label>${t("Carpeta dentro del port donde se extrae (vacío = la carpeta del port)")}<input id="mod-dir" type="text" placeholder="mods"></label>
        <label class="check"><input id="mod-nsfw" type="checkbox"> ${t("Contenido +18 (desnudos, sexo o violencia extrema)")}</label>
        <p class="muted small">${t("Los mods +18 sin marcar se ocultan al recibir denuncias.")}</p>
      </div>
      <div class="news-foot"><button class="btn primary" id="mod-send">${icon("plus")}${t("Subir al Taller")}</button></div>
    </div>
    <div class="news-side"><button class="news-round" data-dlg-close title="${t("Cerrar")}">${icon("close")}</button></div>`;
  document.body.appendChild(box);
  const fillPorts = () => {
    const gm = state.games.get($("#mod-game").value);
    $("#mod-port").innerHTML = `<option value="">${t("Cualquier port del juego")}</option>` +
      (gm?.ports || []).map((p) => `<option value="${esc(p.id)}">${esc(p.port)} · ${esc(p.author)}</option>`).join("");
  };
  fillPorts();
  $("#mod-game").addEventListener("change", fillPorts);
  invoke("my_repos").then((list) => {
    $("#mod-repos").innerHTML = list.map((r) => `<option value="${esc(r.fullName)}">`).join("");
  }).catch(() => {});
  box.addEventListener("click", async (e) => {
    if (e.target === box || e.target.closest("[data-dlg-close]")) { box.remove(); return; }
    const send = e.target.closest("#mod-send");
    if (!send) return;
    send.disabled = true;
    try {
      const meta = {
        repo: $("#mod-repo").value, game: $("#mod-game").value, port: $("#mod-port").value || null,
        name: $("#mod-name").value, description: $("#mod-desc").value, installDir: $("#mod-dir").value, nsfw: $("#mod-nsfw").checked,
      };
      await call("submit_mod", { meta });
      box.remove();
      toast(t("¡Mod subido! Aparecerá en el Taller en unos minutos."), "ok");
      setTimeout(() => loadMods(true).then(repaintWorkshop), 90000);
    } catch { send.disabled = false; }
  });
}

$("#show-nsfw").addEventListener("change", async (e) => {
  try {
    await call("set_show_nsfw", { value: e.target.checked });
    state.settings.showNsfw = e.target.checked;
    repaintWorkshop();
  } catch { e.target.checked = !e.target.checked; }
});

// ---------- Comunidad ----------

async function renderCommunity() {
  const box = $("#community");
  if (!box.innerHTML) box.innerHTML = `<div class="muted" style="padding:24px">${t("Cargando…")}</div>`;
  const [mods, reviews] = await Promise.all([loadMods(), state.recentReviews || invoke("recent_reviews").catch(() => [])]);
  state.recentReviews = reviews;
  if (state.view !== "community") return;
  const vis = visibleMods(mods).filter((m) => state.games.get(m.game));
  const popular = [...vis].sort((a, b) => (a.nsfw - b.nsfw) || (b.likes - a.likes)).slice(0, 8);
  const recent = [...vis].sort((a, b) => (a.nsfw - b.nsfw) || b.createdAt.localeCompare(a.createdAt)).slice(0, 8);
  const stars = (n) => "★".repeat(n) + "☆".repeat(5 - n);
  const revs = reviews.filter((r) => state.games.get(r.game));
  box.innerHTML = `<div class="community">
    <div class="com-head">
      <div><h1>${t("Comunidad")}</h1><p class="muted">${t("Mods del Taller y reseñas de quienes juegan con Dársena.")}</p></div>
      <button class="btn primary" data-mod-upload="">${icon("plus")}${t("Subir un mod")}</button>
    </div>
    <h2 class="section-title" id="com-mods">${t("Mods populares")}</h2>
    ${popular.length ? `<div class="mod-list">${popular.map((m) => modCardHtml(m, true)).join("")}</div>`
      : `<p class="muted">${t("El Taller acaba de abrir: aún no hay mods. ¡Sé el primero en subir uno!")}</p>`}
    ${vis.length > 8 ? `<h2 class="section-title">${t("Mods nuevos")}</h2><div class="mod-list">${recent.map((m) => modCardHtml(m, true)).join("")}</div>` : ""}
    <h2 class="section-title" id="com-reviews">${t("Reseñas recientes")}</h2>
    ${revs.length ? `<div class="com-reviews">${revs.map((r) => {
      const gm = state.games.get(r.game);
      return `<div class="com-review" data-open="${esc(gm.id)}">
        <div class="mod-cover">${coverImg(gm, "mod-cover-img") || ""}</div>
        <div><div class="com-r-head"><b>${esc(gm.title)}</b> <span class="stars">${stars(r.rating)}</span></div>
          <div class="muted small">${esc(r.user)} · ${esc(fmtIsoDate(Date.parse(r.date)))}</div>
          ${r.text ? `<p class="com-r-text">${esc(r.text.slice(0, 280))}${r.text.length > 280 ? "…" : ""}</p>` : ""}</div>
      </div>`;
    }).join("")}</div>` : `<p class="muted">${t("Aún no hay reseñas.")}</p>`}
  </div>`;
}
