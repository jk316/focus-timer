const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const elTime = $("time"), elTask = $("task"), elToday = $("today");
const btnStart = $("btn-start"), btnPause = $("btn-pause"), btnResume = $("btn-resume"),
      btnFinish = $("btn-finish"), btnReset = $("btn-reset"),
      btnClose = $("btn-close"), btnStats = $("btn-stats"), btnMin = $("btn-min");
const modal = $("task-modal"), taskInput = $("task-input"), durationSelect = $("duration-select"),
      btnConfirm = $("btn-confirm"), btnCancel = $("btn-cancel");
const statsPanel = $("stats-panel"), statsTodayTotal = $("stats-today-total"),
      statsDateSelect = $("stats-date-select"), statsSessions = $("stats-sessions"),
      btnStatsClose = $("btn-stats-close"), autostartToggle = $("autostart-toggle");
const resizeGrip = $("resize-grip");

let phase = "idle";

function fmt(secs) {
  secs = Math.max(0, Math.round(secs));
  const m = Math.floor(secs / 60), s = secs % 60;
  return String(m).padStart(2, "0") + ":" + String(s).padStart(2, "0");
}
function fmtMinutes(secs) {
  return Math.round(secs / 60) + " 分钟";
}
function truncate(s) {
  const t = String(s || "").trim();
  const max = 14;
  return t.length > max ? t.slice(0, max) + "…" : t;
}
function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

function render(state) {
  phase = state.phase;
  elTime.textContent = fmt(state.remainingSecs);
  elToday.textContent = "今日 " + fmtMinutes(state.todayTotalSecs);
  const active = phase === "running" || phase === "paused";
  elTask.textContent = active && state.task ? truncate(state.task) : "尚未开始专注";

  btnStart.classList.toggle("hidden", phase !== "idle");
  btnPause.classList.toggle("hidden", phase !== "running");
  btnResume.classList.toggle("hidden", phase !== "paused");
  btnFinish.classList.toggle("hidden", !(phase === "running" || phase === "paused"));
  btnReset.classList.toggle("hidden", phase === "idle");

  document.body.classList.toggle("running", phase === "running");
  document.body.classList.toggle("paused", phase === "paused");
}

async function refresh() {
  try { render(await invoke("get_state")); } catch (e) { console.error(e); }
}

listen("timer-tick", (event) => render(event.payload));

// —— 开始 / 任务弹窗 ——
btnStart.addEventListener("click", () => {
  taskInput.value = "";
  taskInput.classList.remove("error");
  modal.classList.remove("hidden");
  taskInput.focus();
});
btnConfirm.addEventListener("click", async () => {
  const task = taskInput.value.trim();
  if (!task) { taskInput.classList.add("error"); return; }
  const durationSecs = parseInt(durationSelect.value, 10);
  try {
    render(await invoke("start_timer", { task, durationSecs }));
    modal.classList.add("hidden");
  } catch (e) { alert(String(e)); }
});
btnCancel.addEventListener("click", () => modal.classList.add("hidden"));
taskInput.addEventListener("input", () => taskInput.classList.remove("error"));
taskInput.addEventListener("keydown", (e) => { if (e.key === "Enter") btnConfirm.click(); });

// —— 控制 ——
btnPause.addEventListener("click", async () => { try { render(await invoke("pause_timer")); } catch (e) { alert(String(e)); } });
btnResume.addEventListener("click", async () => { try { render(await invoke("resume_timer")); } catch (e) { alert(String(e)); } });
btnFinish.addEventListener("click", async () => { try { render(await invoke("finish_timer")); } catch (e) { alert(String(e)); } });
btnReset.addEventListener("click", async () => render(await invoke("reset_timer")));
btnMin.addEventListener("click", () => invoke("minimize_to_tray"));
btnClose.addEventListener("click", () => invoke("quit_app"));

// —— 统计 ——
btnStats.addEventListener("click", openStats);
btnStatsClose.addEventListener("click", () => statsPanel.classList.add("hidden"));
statsDateSelect.addEventListener("change", () => loadStats(statsDateSelect.value));

async function openStats() {
  statsPanel.classList.remove("hidden");
  await loadStats(null);
  try {
    const s = await invoke("get_settings");
    autostartToggle.checked = !!s.autostart;
  } catch (e) { console.error(e); }
}
autostartToggle.addEventListener("change", async () => {
  try { await invoke("set_autostart", { enabled: autostartToggle.checked }); }
  catch (e) { console.error(e); }
});
async function loadStats(date) {
  try {
    const s = await invoke("get_stats", { date });
    statsTodayTotal.textContent = fmtMinutes(s.todayTotalSecs);
    statsDateSelect.innerHTML = "";
    const dates = s.dates.length ? s.dates : [s.todayDate];
    for (const d of dates) {
      const opt = document.createElement("option");
      opt.value = d; opt.textContent = d;
      opt.selected = d === s.selectedDate;
      statsDateSelect.appendChild(opt);
    }
    statsSessions.innerHTML = "";
    if (!s.sessions.length) {
      statsSessions.innerHTML = '<li class="empty">当天暂无专注记录</li>';
    } else {
      for (const sess of s.sessions) {
        const li = document.createElement("li");
        const t = (sess.startAt || "").slice(11, 16) + "–" + (sess.endAt || "").slice(11, 16);
        li.innerHTML =
          '<span class="s-task">' + escapeHtml(sess.task) + "</span>" +
          '<span class="s-dur">' + fmtMinutes(sess.durationSecs) + "</span>" +
          '<span class="s-time">' + t + "</span>";
        statsSessions.appendChild(li);
      }
    }
  } catch (e) { console.error(e); }
}

// —— 缩放抓手（OS 原生 resize，顺滑不卡）——
resizeGrip.addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  e.preventDefault();
  invoke("start_resize_dragging", { direction: "SouthEast" });
});

// —— 拖动窗口（OS 原生拖动；整个窗口都可拖，按钮/输入框/下拉框除外）——
$("app").addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  if (e.target.closest("button, input, select, a, label, #resize-grip, .overlay")) return;
  invoke("start_drag");
});

refresh();
