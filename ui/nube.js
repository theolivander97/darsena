// Partidas guardadas en la nube (Gist privado de GitHub).
// Se carga después de app.js y comparte sus variables y funciones (todo es global, como en app.js).

// ---------- Partidas en la nube ----------

const cloudOn = () => !!state.user && state.settings?.cloudSaves !== false;

// Antes de jugar: si la nube tiene partidas más nuevas, se traen; si han cambiado las dos, se pregunta.
async function playWithCloud(id) {
  if (cloudOn()) {
    const st = await invoke("cloud_status", { id }).catch(() => null);
    if (st?.action === "download") {
      toast(t("Trayendo tus partidas de la nube…"), "ok");
      await call("cloud_restore", { id }).catch(() => {});
    } else if (st?.action === "conflict") {
      const choice = await askCloudConflict(st);
      if (choice === null) return;
      if (choice === "cloud") await call("cloud_restore", { id }).catch(() => {});
      else await call("cloud_upload", { id }).catch(() => {});
    }
  }
  await call("launch_game", { id });
}

async function uploadSavesAfterPlaying(id) {
  if (!cloudOn()) return;
  const st = await invoke("cloud_status", { id }).catch(() => null);
  if (st?.action !== "upload") return;
  try {
    await invoke("cloud_upload", { id });
    toast(t("Partidas subidas a la nube"), "ok");
  } catch {}
  if (state.view === "library") renderCloudBox();
}

function askCloudConflict(st) {
  return new Promise((resolve) => {
    $("#mod-dialog")?.remove();
    const box = document.createElement("div");
    box.id = "mod-dialog";
    box.className = "news-modal";
    const when = (s) => (s ? fmtDate(s) : "—");
    box.innerHTML = `<div class="news-dialog small-dialog">
        <div class="news-head"><h2>${t("¿Qué partidas usamos?")}</h2>
          <div class="muted small">${t("Las partidas de este PC y las de la nube han cambiado por separado. La que no elijas se guarda como copia en la carpeta .darsena-copias del juego.")}</div></div>
        <div class="news-body cloud-choice">
          <button class="btn" data-cc="local"><b>${t("Las de este PC")}</b><span class="muted small">${t("Modificadas: {d}", { d: when(st.localModified) })}</span></button>
          <button class="btn" data-cc="cloud"><b>${t("Las de la nube")}</b><span class="muted small">${t("Subidas: {d}", { d: when(st.remoteUpdated) })}${st.remoteMachine ? ` · ${esc(st.remoteMachine)}` : ""}</span></button>
        </div>
      </div>
      <div class="news-side"><button class="news-round" data-dlg-close title="${t("Cerrar")}">${icon("close")}</button></div>`;
    document.body.appendChild(box);
    box.addEventListener("click", (e) => {
      const c = e.target.closest("[data-cc]");
      if (c) { box.remove(); resolve(c.dataset.cc); }
      else if (e.target === box || e.target.closest("[data-dlg-close]")) { box.remove(); resolve(null); }
    });
  });
}

// Recuadro de la ficha de la biblioteca: estado y botones.
async function renderCloudBox() {
  const box = $("#cloud-box");
  if (!box) return;
  const id = box.dataset.cloud;
  if (!state.user) { box.innerHTML = ""; return; }
  const st = await invoke("cloud_status", { id }).catch(() => null);
  if (!st || $("#cloud-box")?.dataset.cloud !== id) return;
  const text = {
    none: st.dirs.length ? t("Aún no hay partidas") : t("No se encontraron carpetas de partidas de este juego"),
    synced: t("Sincronizadas · {d}", { d: fmtDate(st.lastSync) }),
    upload: t("Hay partidas nuevas en este PC sin subir"),
    download: t("Hay partidas más nuevas en la nube ({m})", { m: esc(st.remoteMachine || "?") }),
    conflict: t("Las partidas de este PC y las de la nube son distintas"),
  }[st.action];
  $("#cloud-box").innerHTML = `<div class="cloud-box ${st.action}">
    <div class="cloud-txt"><b>${t("Partidas en la nube")}</b><div class="muted small">${text}</div></div>
    ${st.dirs.length ? `<button class="btn ghost small" data-cloud-up="${esc(id)}">${t("Subir")}</button>` : ""}
    ${st.remoteUpdated ? `<button class="btn ghost small" data-cloud-down="${esc(id)}">${t("Traer de la nube")}</button>` : ""}
  </div>`;
}

document.addEventListener("click", async (e) => {
  const up = e.target.closest("[data-cloud-up]");
  const down = e.target.closest("[data-cloud-down]");
  if (!up && !down) return;
  const b = up || down;
  b.disabled = true;
  try {
    if (up) { await call("cloud_upload", { id: up.dataset.cloudUp }); toast(t("Partidas subidas a la nube"), "ok"); }
    else { await call("cloud_restore", { id: down.dataset.cloudDown }); toast(t("Partidas traídas de la nube (la copia anterior está en .darsena-copias)"), "ok"); }
  } catch {}
  renderCloudBox();
});

$("#cloud-saves").addEventListener("change", async (e) => {
  try {
    await call("set_cloud_saves", { value: e.target.checked });
    state.settings.cloudSaves = e.target.checked;
  } catch { e.target.checked = !e.target.checked; }
});
