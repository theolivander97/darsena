// Requisitos del sistema de cada juego y «¿Funciona en tu PC?» (compara tu hardware con ellos).
// Se carga después de app.js y comparte sus variables y funciones (todo es global, como en app.js).

// ---------- Requisitos del sistema ----------

const REQ_FIELDS = [["os", "Sistema operativo"], ["cpu", "Procesador"], ["ram", "Memoria"], ["gpu", "Gráficos"], ["storage", "Almacenamiento"], ["other", "Notas"]];

// Estimación por consola cuando nadie ha publicado requisitos: componentes concretos (como en Steam), de menos a más.
const REQ_TIERS = [
  { // 2D y 8/16 bits
    consoles: ["NES", "Super Nintendo", "Game Boy", "Game Boy Color", "Game Boy Advance", "Virtual Boy", "Sega", "Arcade", "PC", "Otros"],
    minimum: { cpu: "Intel Core 2 Duo E6600 / AMD Athlon 64 X2", ram: "2 GB", gpu: "Intel HD Graphics 4000 (OpenGL 3.3)", storage: "500 MB" },
    recommended: { cpu: "Intel Core i3-4130 / AMD FX-4300", ram: "4 GB", gpu: "Intel UHD Graphics 620" },
  },
  { // 3D de quinta generación y portátiles
    consoles: ["Nintendo 64", "PlayStation", "Nintendo DS"],
    minimum: { cpu: "Intel Core i3-2100 / AMD FX-4100", ram: "4 GB", gpu: "Intel HD Graphics 4000 / NVIDIA GeForce GT 630", storage: "1 GB" },
    recommended: { cpu: "Intel Core i5-4460 / AMD Ryzen 3 1200", ram: "8 GB", gpu: "NVIDIA GeForce GTX 750 Ti / AMD Radeon R7 260X" },
  },
  { // Sexta generación
    consoles: ["GameCube", "Wii", "PlayStation 2", "PSP", "Xbox", "Nintendo 3DS"],
    minimum: { cpu: "Intel Core i5-4460 / AMD Ryzen 3 1200", ram: "8 GB", gpu: "NVIDIA GeForce GTX 750 Ti / Intel UHD Graphics 620", storage: "5 GB" },
    recommended: { cpu: "Intel Core i5-8400 / AMD Ryzen 5 2600", ram: "8 GB", gpu: "NVIDIA GeForce GTX 1050 Ti / AMD Radeon RX 570" },
  },
  { // Séptima generación en adelante
    consoles: ["Xbox 360", "PlayStation 3", "Wii U", "Switch", "PlayStation 4"],
    minimum: { cpu: "Intel Core i3-8100 / AMD Ryzen 3 2200G", ram: "8 GB", gpu: "NVIDIA GeForce GT 1030 / AMD Radeon RX 550", storage: "20 GB" },
    recommended: { cpu: "Intel Core i5-10400 / AMD Ryzen 5 3600", ram: "16 GB", gpu: "NVIDIA GeForce GTX 1660 / AMD Radeon RX 5600 XT" },
  },
];
const tierOf = (gm) => {
  const consoles = consolesOf(gm);
  let best = 0;
  REQ_TIERS.forEach((tier, i) => { if (consoles.some((c) => tier.consoles.includes(c))) best = Math.max(best, i); });
  return REQ_TIERS[best];
};

// Los del autor (darsena.json) ganan a los del catálogo. Sin ninguno, la estimación por consola.
function renderRequirements(gm, authorResults) {
  const box = $("#req-box");
  if (!box) return;
  const pick = (v) => (typeof v === "string" ? v : v ? v[LANG()] || v.es || v.en : "");
  const fromAuthor = authorResults.find(([, a]) => a?.requirements);
  const fromCatalog = sortedPorts(gm).find((p) => p.requirements);
  const [port, reqs, source] = fromAuthor ? [fromAuthor[0], fromAuthor[1].requirements, t("publicados por su autor")]
    : fromCatalog ? [fromCatalog, fromCatalog.requirements, t("del catálogo de Dársena")] : [sortedPorts(gm)[0], null, null];
  const general = !reqs;
  const os = { os: (port?.platforms || []).includes("Windows") || !port?.platforms?.length ? t("Windows 10 u 11 (64 bits)") : (port.platforms || []).join(", ") };
  const tier = tierOf(gm);
  const minimum = reqs?.minimum ? { ...os, ...reqs.minimum } : { ...os, ...tier.minimum };
  const recommended = reqs ? reqs.recommended : { ...os, ...tier.recommended };
  const col = (title, set) => `<div class="sysreq-col"><h4>${title}</h4><dl>${REQ_FIELDS.filter(([k]) => set[k])
    .map(([k, label]) => `<dt>${t(label)}</dt><dd>${esc(pick(set[k]))}</dd>`).join("")}</dl></div>`;
  box.innerHTML = `<h2 class="section-title">${t("Requisitos del sistema")}</h2>
    <div id="pc-check"></div>
    <div class="sysreq">${col(t("MÍNIMOS"), minimum)}${recommended ? col(t("RECOMENDADOS"), recommended) : ""}</div>
    <div class="muted small">${general
      ? t("Estimación de Dársena según la consola original: el autor del port no ha publicado requisitos. Además del PC, necesitarás tus datos del juego{what}.", { what: port?.needs?.length ? ` (${esc(needsText(port))})` : "" })
      : `${gm.ports.length > 1 ? `${esc(port.port)} · ` : ""}${source}`}</div>
    <p class="req-note">${t("Los requisitos son aproximados: cumplirlos no garantiza que el juego vaya siempre bien, y con un PC algo menos potente puede que funcione igual.")}</p>`;
  renderPcCheck(gm, minimum, recommended);
}

// ---------- ¿Funciona en tu PC? ----------

// Potencia aproximada de un procesador o una gráfica a partir de su nombre (escala propia: más es mejor; null = no se sabe).
// Sirve para comparar con los requisitos, que también son nombres. Es una estimación: por eso se avisa en la ficha.
function cpuScore(name) {
  const n = (name || "").toLowerCase().replace(/\((tm|r)\)/g, "");
  let m = n.match(/core\s*(?:tm\s*)?(?:ultra\s*)?i?([3579])[\s-]+(\d{4,5})/) || n.match(/core\s*ultra\s*([3579])\s+(\d{3})/);
  if (m) {
    const tier = Number(m[1]);
    const num = m[2];
    const gen = num.length === 3 ? 14 : num.length === 5 ? Number(num.slice(0, 2)) : Number(num[0]);
    return tier * (1 + 0.15 * (Math.min(gen, 15) - 2));
  }
  m = n.match(/ryzen\s*(?:ai\s*)?([3579])\s+(?:pro\s+)?(\d)(\d{3})/);
  if (m) {
    const intelGen = { 1: 5, 2: 6, 3: 9, 4: 10, 5: 11, 7: 13, 8: 13, 9: 15 }[Number(m[2])] ?? 9;
    return Number(m[1]) * 1.15 * (1 + 0.15 * (intelGen - 2));
  }
  if (/core\s*(?:tm)?\s*2|athlon|pentium|celeron|phenom/.test(n)) return 2;
  if (/\bfx[\s-]*\d/.test(n)) return 3;
  if (/xeon|threadripper|epyc/.test(n)) return 12;
  if (/apple|snapdragon/.test(n)) return 10;
  return null;
}

function gpuScore(name) {
  const n = (name || "").toLowerCase().replace(/\((tm|r)\)/g, "");
  let m = n.match(/(gtx|rtx|gt)\s*(\d{3,4})\s*(ti|super)?/);
  if (m) {
    const num = m[2];
    const gen = num.length === 4 ? Number(num.slice(0, 2)) : Number(num[0]);
    const cls = Number(num.slice(-2));
    const base = { 6: 1, 7: 2.5, 9: 4.5, 10: 6, 16: 8, 20: 11, 30: 13, 40: 15, 50: 17 }[gen] ?? 5;
    return Math.max(0.5, base + (cls - 50) / 10 * 2 + (m[3] ? 0.7 : 0));
  }
  m = n.match(/rx\s*(\d{3,4})\s*(xt)?/);
  if (m) {
    const num = m[1];
    if (num.length === 4) {
      const base = { 5: 5, 6: 6, 7: 7.5, 9: 10 }[Number(num[0])] ?? 6;
      return base + (Number(num[1]) - 5) * 2.5 + (m[2] ? 1 : 0);
    }
    return { 4: 3, 5: 3, 6: 4, 7: 6, 8: 7 }[Number(num[1])] ?? 4;
  }
  if (/r[579]\s*\d{3}/.test(n)) return 2.5;
  if (/arc\s*a(\d{3})/.test(n)) return Number(n.match(/arc\s*a(\d{3})/)[1]) >= 700 ? 12 : 6;
  if (/arc\s*b\d{3}/.test(n)) return 14;
  if (/iris\s*xe|arc\s*graphics/.test(n)) return 3.5;
  if (/uhd.*7[0-9]{2}/.test(n)) return 2.5;
  if (/uhd|hd\s*(graphics\s*)?[56]\d{2}/.test(n)) return 2;
  if (/hd\s*(graphics\s*)?4\d{3}|hd graphics$/.test(n)) return 1;
  if (/radeon.*(vega|graphics)|780m|680m|880m/.test(n)) return 3.5;
  if (/quadro|tesla/.test(n)) return 8;
  return null;
}

// Requisito escrito ("Intel Core i5-4460 / AMD Ryzen 3 1200") -> la menor de sus alternativas.
const reqScore = (text, fn) => {
  const vals = String(text || "").split(/\/|\bo\b|\bor\b/).map(fn).filter((v) => v != null);
  return vals.length ? Math.min(...vals) : null;
};
const gbOf = (text) => { const m = String(text || "").replace(",", ".").match(/(\d+(?:\.\d+)?)\s*(gb|mb)/i); return m ? Number(m[1]) / (m[2].toLowerCase() === "mb" ? 1024 : 1) : null; };

async function loadHardware() {
  if (!state.hardware) state.hardware = invoke("get_hardware").catch(() => null);
  return state.hardware;
}

// Compara el PC con los requisitos mostrados en la ficha: "rec" (cumple los recomendados), "min", "below" o null (no se sabe).
function checkPc(hw, minimum, recommended) {
  const pick = (v) => (typeof v === "string" ? v : v ? v[LANG()] || v.es || v.en : "");
  const myCpu = cpuScore(hw.cpu);
  const myGpu = Math.max(...hw.gpus.map(gpuScore).filter((v) => v != null), -1);
  const rows = [];
  const level = (mine, min, rec) => (mine == null || mine < 0 || min == null ? null : mine >= (rec ?? Infinity) ? "rec" : mine >= min * 0.9 ? "min" : "below");
  rows.push({ label: "Procesador", mine: hw.cpu || "?", status: level(myCpu, reqScore(pick(minimum.cpu), cpuScore), reqScore(pick(recommended?.cpu), cpuScore)) });
  rows.push({ label: "Memoria", mine: `${Math.round(hw.ramGb)} GB`, status: level(Math.round(hw.ramGb), gbOf(pick(minimum.ram)), gbOf(pick(recommended?.ram))) });
  rows.push({ label: "Gráficos", mine: hw.gpus[0] ? [...hw.gpus].sort((a, b) => (gpuScore(b) ?? 0) - (gpuScore(a) ?? 0))[0] : "?", status: level(myGpu, reqScore(pick(minimum.gpu), gpuScore), reqScore(pick(recommended?.gpu), gpuScore)) });
  // Lo que hace fallar un port aunque el PC sea potente.
  const all = [minimum, recommended].filter(Boolean).flatMap((s) => Object.values(s).map(pick)).join(" ");
  const blockers = [];
  if (/avx2/i.test(all) && !hw.avx2) blockers.push(t("Tu procesador no tiene AVX2, y este port lo necesita."));
  const vk = all.match(/vulkan\s*(\d\.\d)/i);
  if (vk && (!hw.vulkan || Number(hw.vulkan) < Number(vk[1]))) blockers.push(t("Este port necesita Vulkan {need} y tu gráfica tiene {have}.", { need: vk[1], have: hw.vulkan || t("ninguno") }));
  const known = rows.map((r) => r.status).filter(Boolean);
  const overall = blockers.length || known.includes("below") ? "below" : !known.length ? null : known.every((s) => s === "rec") ? "rec" : "min";
  return { rows, blockers, overall };
}

const PC_VERDICT = {
  rec: "Tu PC cumple los requisitos recomendados",
  min: "Tu PC cumple los requisitos mínimos",
  below: "Tu PC está por debajo de los requisitos",
};

async function renderPcCheck(gm, minimum, recommended) {
  const hw = await loadHardware();
  const box = $("#pc-check");
  if (!box || !hw || state.detailGame !== gm.id) return;
  const c = checkPc(hw, minimum, recommended);
  const compat = await invoke("get_compat", { game: gm.id }).catch(() => null);
  if (state.detailGame !== gm.id || !$("#pc-check")) return;
  const verdict = c.overall ? PC_VERDICT[c.overall] : "No sabemos comparar tu PC con estos requisitos";
  // Informes de PCs parecidos al tuyo (misma potencia de gráfica, más o menos).
  const myGpu = Math.max(...hw.gpus.map(gpuScore).filter((x) => x != null), -1);
  const similar = (compat?.reports || []).filter((r) => { const g = gpuScore(r.gpu); return g != null && myGpu > 0 && Math.abs(g - myGpu) <= 2; });
  const mine = state.myCompat?.[gm.id];
  $("#pc-check").innerHTML = `<div class="pc-check ${c.overall || "unknown"}">
    <div class="pc-head"><div><b>${t("¿Funciona en tu PC?")}</b><div>${t(verdict)}</div></div></div>
    <dl class="pc-rows">${c.rows.map((r) => `<dt><i class="dot ${r.status || "unknown"}"></i>${t(r.label)}</dt><dd>${esc(r.mine)}</dd>`).join("")}</dl>
    ${c.blockers.map((b) => `<div class="pc-blocker">${esc(b)}</div>`).join("")}
    ${compat ? `<div class="pc-community">${t("La comunidad: {g} bien · {o} con problemas · {b} mal", { g: compat.good, o: compat.ok, b: compat.bad })}${similar.length
      ? ` · ${t("en PCs como el tuyo: {g} de {n} bien", { g: similar.filter((r) => r.result === "good").length, n: similar.length })}` : ""}</div>` : ""}
    <div class="pc-ask"><span class="muted small">${mine ? t("Gracias por contar qué tal te funcionó. Puedes cambiarlo:") : t("¿Lo has jugado? Cuenta qué tal te funcionó:")}</span>
      ${[["good", "Bien"], ["ok", "Con problemas"], ["bad", "Mal"]].map(([k, l]) => `<button class="btn ghost small ${mine === k ? "on" : ""}" data-compat="${k}" data-compat-game="${esc(gm.id)}">${t(l)}</button>`).join("")}</div>
  </div>`;
}

document.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-compat]");
  if (!b) return;
  if (!state.user) { toast(t("Inicia sesión con GitHub para contar qué tal te funcionó"), "error"); return; }
  const gameId = b.dataset.compatGame;
  const gm = state.games.get(gameId);
  const port = (installedPortOf(gm) && state.byId.get(installedPortOf(gm).id)) || sortedPorts(gm)[0];
  b.disabled = true;
  try {
    await call("report_compat", { game: gameId, result: b.dataset.compat, port: port?.port || "" });
    (state.myCompat ||= {})[gameId] = b.dataset.compat;
    try { localStorage.setItem("darsena.myCompat", JSON.stringify(state.myCompat)); } catch {}
    toast(t("¡Gracias! Tu informe ayuda a otros a saber si les funcionará."), "ok");
    renderDetail();
  } catch { b.disabled = false; }
});
try { state.myCompat = JSON.parse(localStorage.getItem("darsena.myCompat") || "{}"); } catch { state.myCompat = {}; }
