<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { marked } from "marked";
  import { fade } from "svelte/transition";
  import { onMount } from "svelte";
  import { Store } from "@tauri-apps/plugin-store";
  import { sendNotification, isPermissionGranted, requestPermission } from "@tauri-apps/plugin-notification";

  const NOTIFY_ICON = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAAIGNIUk0AAHomAACAhAAA+gAAAIDoAAB1MAAA6mAAADqYAAAXcJy6UTwAAAAGYktHRAD/AP8A/6C9p5MAAAAHdElNRQfqBxETGS8PP/fGAAAAJXRFWHRkYXRlOmNyZWF0ZQAyMDI2LTA3LTE3VDE5OjIwOjE1KzAwOjAwkmj5aQAAACV0RVh0ZGF0ZTptb2RpZnkAMjAyNi0wNy0xN1QxOToyMDoxNSswMDowMOM1QdUAAAAodEVYdGRhdGU6dGltZXN0YW1wADIwMjYtMDctMTdUMTk6MjU6NDcrMDA6MDCNdrQDAAAD9klEQVR42u2b3Y7bRBTHf8czTprSbBGVEEViS5FAiNL2gtdoJXgGJKqKwgUvwEVfABWplKqIS64RPEBV8QTbOy5a7W6l3UKlfiTdfKxnhotxQpq1nbQknqzxT4qsxBn7nP/MnOOZ5MjnXzwFUMCnwJdAG/gMePDTzeNUgdTHd4BfgQFwE/gF6OvU+a+Bb4HjwHb6WdWIgBPAOnAeeB+4qvE9P3IewIa2dIm49NgCvgF2NX7YT471aCRCOnSqgk19G9EErmj8nN+e+MIOcHJCjCpgU592Jt4LEGt8wFNTX/w+PVZlOow69qv0OO5YDTzIaHASHzWrxg5+tI/Rk6kunfPjGFAxxjFg0ueqzPFXphYgtAGhqQUIbUBoagFCGxCaWoDQBoSmFiC0AaHRZd/QWnAOvxidxIEIRCV3SakCiMB7pxXtNfl3b2Z8EjrPHJtbxgtURQGiCC5eaHLurMbag+c27iZcv7GHMRUVYOSoCCiVfa50e8q/5Wqx0gI0lNCK5b9f6DAK4IAjsRdgmTFxZQUAUAJ2yRlh4UGwKJdrLYgUt9VaAJcGSiGxjlj5ETF+hlhVAaz1ef7ihWamCCJwaj3/V7dT64rLl1o4ByaBYReUhsZrfiT89vuAe/fN3Nki63lrqQI4B+014dxZXdjTebTbwsdnvEnDHvSeO5SCY68LTuD2neFLjYCjseAc9JP8RoudAn70Ym12np9F94lj4w9L3PDT6INPIhxgDOxuWtpDxUdvyUxxjYWHXcOHb8bsG9jYGWByNvpLfxAqIm4Kb58WdAPihnB0Teh1HMk+JPswEMuj55ZohgDWwTBxPO1ZkhlxY6UEaLbg3TMvTnCJYNh3tI4JA+142DGoOWKACPz59z5AYRpdKQGyUErodR069k6NXvMwT7hY7HNAGnYX+UyvYxj2/eiQeb16mesv8mKSLmk37iaFabDdzu7CTufgctgm0P0LjjyGZx33StmlNAGiCDa3DNdv7GXfTAuXL7XGqW6azS3DDz/2SCbSloqEN1oRj/YMxix+xbjwGOAcBet5VxiRnYMkcS+0t8bx2FmSZNGWelZ6LQB+yg+S5S0IVl6AZVMLENqA0ATbFs/aFLUB/phTqgDW+iXt7TvD3G3xskUoVQDn4N498//9YQTCbH0X2hPagNDUAoQ2IDS1AKENCE0tQGgDQlMLENqA0NQChDYgNDqjMmy6uqoqZFbDaXxt0MjhrOqqKpBbDafx5aQn8Cv03OqqQ05uNZzG19KuTzU4UF1VEaar4bY1vpD4PL6cdKTWgeqqw0xONZwBbkX4Kurv8CMBZv+pogoY4Gfgmgb6wFVgF7gCxKGtWzLbwC3gGvDkH8ZcQcnSdwBLAAAAAElFTkSuQmCC";

  let store: Store;
  let storeReady = $state(false);
  let darkMode = $state(false);

  const DEFAULT_MODEL = "baidu/ernie-5.5-flash-preview:free";

  function applyTheme(dark: boolean) {
    document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
  }

  onMount(async () => {
    store = await Store.load("settings.json");
    const savedKey = await store.get<string>("apiKey");
    if (savedKey) apiKey = savedKey;

    const savedDark = await store.get<boolean>("darkMode");
    if (savedDark !== null && savedDark !== undefined) {
      darkMode = savedDark;
    } else {
      darkMode = window.matchMedia("(prefers-color-scheme: dark)").matches;
    }
    applyTheme(darkMode);

    const savedAutoClear = await store.get<boolean>("autoClearEnabled");
    if (savedAutoClear !== null && savedAutoClear !== undefined) {
      autoClearEnabled = savedAutoClear;
    }
    const savedRetention = await store.get<number>("retentionDays");
    if (savedRetention !== null && savedRetention !== undefined) {
      retentionDays = savedRetention;
    }
    const savedNotify = await store.get<boolean>("notifyEnabled");
    if (savedNotify !== null && savedNotify !== undefined) {
      notifyEnabled = savedNotify;
    }

    // Реестр моделей OpenRouter: список добавленных по ID, выбор активной.
    const savedRegistry = await store.get<ModelEntry[]>("modelRegistry");
    if (Array.isArray(savedRegistry) && savedRegistry.length > 0) {
      modelRegistry = savedRegistry
        .filter((m) => m && typeof m.id === "string" && m.id.trim() !== "")
        .map((m) => ({ id: m.id.trim(), name: m.name?.trim() || m.id.trim() }));
    } else {
      modelRegistry = [{ id: DEFAULT_MODEL, name: "ERNIE 5.5 Flash (free)" }];
    }
    const savedModel = await store.get<string>("model");
    if (savedModel && modelRegistry.some((m) => m.id === savedModel)) {
      selectedModel = savedModel;
    } else if (!modelRegistry.some((m) => m.id === selectedModel)) {
      selectedModel = modelRegistry[0].id;
    }

    await loadTables();

    storeReady = true;
  });

  window.addEventListener("beforeprint", () => {
    document.body.style.background = "#fff";
    document.body.style.color = "#000";
  });
  window.addEventListener("afterprint", () => {
    document.body.style.background = "";
    document.body.style.color = "";
  });

  $effect(() => {
    applyTheme(darkMode);
  });

  async function closeSettings() {
    showSettings = false;
    if (storeReady) {
      store.set("apiKey", apiKey);
      store.set("darkMode", darkMode);
      store.set("autoClearEnabled", autoClearEnabled);
      store.set("retentionDays", retentionDays);
      store.set("notifyEnabled", notifyEnabled);
      store.set("model", selectedModel);
      store.set("modelRegistry", modelRegistry);
      await store.save();
      if (autoClearEnabled) {
        invoke("clear_old_history", { retentionDays }).catch(() => {});
      }
    }
  }

  // ── Реестр моделей OpenRouter ──
  function persistModels() {
    if (!storeReady || !store) return;
    store.set("modelRegistry", modelRegistry);
    store.set("model", selectedModel);
    store.save().catch(() => {});
  }

  function selectModel(id: string) {
    if (selectedModel === id) return;
    selectedModel = id;
    persistModels();
  }

  function addModel() {
    const id = newModelId.trim();
    if (!id) return;
    const existing = modelRegistry.find((m) => m.id === id);
    if (existing) {
      selectedModel = id;
    } else {
      modelRegistry = [...modelRegistry, { id, name: id }];
      selectedModel = id;
    }
    newModelId = "";
    persistModels();
  }

  function removeModel(id: string) {
    modelRegistry = modelRegistry.filter((m) => m.id !== id);
    if (selectedModel === id) {
      selectedModel = modelRegistry[0]?.id ?? DEFAULT_MODEL;
    }
    persistModels();
  }

  let diagnoses = $state("");
  let apiKey = $state("");
  let loading = $state(false);
  let error = $state("");
  let showSettings = $state(false);
  let showHistory = $state(false);

  let autoClearEnabled = $state(false);
  let retentionDays = $state(30);
  let notifyEnabled = $state(false);

  interface ModelEntry {
    id: string;
    name: string;
  }

  let modelRegistry = $state<ModelEntry[]>([]);
  let newModelId = $state("");
  let selectedModel = $state(DEFAULT_MODEL);

  interface HistoryItem {
    id: number;
    diagnoses: string;
    scales: string;
    created_at: string;
    preview: string;
  }

  let historyEntries = $state<HistoryItem[]>([]);
  let selectedHistory = $state<{ id: number; raw: string } | null>(null);

  let applicableScales = $state<string[]>([]);
  let rawMarkdown = $state("");
  let visibleScales = $state<string[]>([]);

  const scales = [
    "ВАШ", "Мак-Гилла", "Рэнкин", "Ашфорта", "индекс Бартела",
    "по Вейссу", "мобильности Ривермид", "индекс Хоузера",
    "тест шестиминутной ходьбы", "тест Френча", "DN4",
    "шкала оценки двигательных функций",
  ];

  // Данные из CSV-таблиц (Процедуры.csv / ЛФК.csv) для сносок
  interface ProcInfo {
    equipment: string;
    parameters: string;
  }

  let proceduresMap = $state<Record<string, ProcInfo>>({});
  let lfkMap = $state<Record<string, string>>({});

  let renderedHtml = $derived(rawMarkdown ? marked(rawMarkdown) : "");

  function toggleVisibleScale(s: string) {
    if (visibleScales.includes(s)) {
      visibleScales = visibleScales.filter((x) => x !== s);
    } else {
      visibleScales = [...visibleScales, s];
    }
  }

  // --- Парсинг CSV (Процедуры.csv / ЛФК.csv) ---
  function parseCsvLine(line: string): string[] {
    const out: string[] = [];
    let cur = "";
    let inQuotes = false;
    for (let i = 0; i < line.length; i++) {
      const ch = line[i];
      if (ch === String.fromCharCode(34)) {
        if (inQuotes && line[i + 1] === String.fromCharCode(34)) {
          cur += String.fromCharCode(34);
          i++;
        } else {
          inQuotes = !inQuotes;
        }
      } else if (ch === ";" && !inQuotes) {
        out.push(cur);
        cur = "";
      } else {
        cur += ch;
      }
    }
    out.push(cur);
    return out.map((s) => s.trim());
  }

  function normName(s: string): string {
    return s
      .toLowerCase()
      .replace(/[^a-zа-я0-9]+/gi, " ")
      .replace(/\s+/g, " ")
      .trim();
  }

  async function loadTables() {
    try {
      const procCsv = (await invoke("get_procedures_table")) as string;
      const pm: Record<string, ProcInfo> = {};
      for (const raw of procCsv.split(/\r?\n/)) {
        if (!raw.trim()) continue;
        const cols = parseCsvLine(raw);
        if (cols.length < 4) continue;
        const id = cols[0];
        const name = cols[1] || "";
        const equipment = cols[2] || "";
        const parameters = cols[3] || "";
        if (!name || id.indexOf("Идентификатор") === 0) continue;
        pm[normName(name)] = { equipment, parameters };
      }
      proceduresMap = pm;
    } catch (e) {
      console.error("Не удалось загрузить Процедуры.csv", e);
    }

    try {
      const lfkCsv = (await invoke("get_lfk_table")) as string;
      const lm: Record<string, string> = {};
      for (const raw of lfkCsv.split(/\r?\n/)) {
        if (!raw.trim()) continue;
        const cols = parseCsvLine(raw);
        if (cols.length < 4) continue;
        const name = cols[1] || "";
        const equipment = cols[3] || "";
        if (!name || cols[0] === "№") continue;
        lm[normName(name)] = equipment;
      }
      lfkMap = lm;
    } catch (e) {
      console.error("Не удалось загрузить ЛФК.csv", e);
    }
  }

  function findProcedure(text: string): ProcInfo | null {
    const n = normName(text);
    if (!n) return null;
    if (proceduresMap[n]) return proceduresMap[n];
    for (const key of Object.keys(proceduresMap)) {
      if (key.indexOf(n) >= 0 || n.indexOf(key) >= 0) return proceduresMap[key];
    }
    return null;
  }

  function findLfk(text: string): string | null {
    const n = normName(text);
    if (!n) return null;
    if (lfkMap[n]) return lfkMap[n];
    for (const key of Object.keys(lfkMap)) {
      if (key.indexOf(n) >= 0 || n.indexOf(key) >= 0) return lfkMap[key];
    }
    return null;
  }

  // Оборудование и параметры для каждой процедуры из ячейки (Физио)
  function collectProcDetails(text: string): string[] {
    const parts = text.split(/[,;]/).map((s) => s.trim()).filter(Boolean);
    const res: string[] = [];
    for (const p of parts) {
      const info = findProcedure(p);
      if (!info) continue;
      const bits: string[] = [];
      if (info.equipment && info.equipment !== "-") bits.push("Оборудование: " + info.equipment);
      if (info.parameters && info.parameters !== "-") bits.push("Параметры: " + info.parameters);
      if (bits.length) res.push(p + " — " + bits.join(" | "));
    }
    return res;
  }

  // Оборудование для каждой процедуры из ячейки (ЛФК)
  function collectLfkDetails(text: string): string[] {
    const parts = text.split(/[,;]/).map((s) => s.trim()).filter(Boolean);
    const res: string[] = [];
    for (const p of parts) {
      const eq = findLfk(p);
      if (eq && eq !== "-") res.push(p + " — Оборудование: " + eq);
    }
    return res;
  }

  function parseResult(response: string) {
    const sepIdx = response.lastIndexOf("&&&");
    const structured = sepIdx !== -1
      ? response.substring(sepIdx + 3).trim()
      : response;

    const delimiter = "<tables_start>";
    const idx = structured.indexOf(delimiter);
    if (idx !== -1) {
      const scalesPart = structured.substring(0, idx).trim();
      rawMarkdown = structured.substring(idx + delimiter.length).trim();
      applicableScales = scalesPart.split(",").map((s) => s.trim()).filter(Boolean);
      visibleScales = [...applicableScales];
    } else {
      rawMarkdown = structured;
      applicableScales = [];
      visibleScales = [];
    }
  }

  async function send() {
    if (!diagnoses.trim()) return;
    loading = true;
    error = "";
    rawMarkdown = "";
    applicableScales = [];
    visibleScales = [];
    try {
      const result = await invoke("query_openrouter", {
        diagnoses: diagnoses.trim(),
        scales,
        apiKey: apiKey.trim(),
        model: selectedModel,
      });
      parseResult(result as string);
      if (notifyEnabled) {
        let perm = await isPermissionGranted();
        if (!perm) {
          perm = (await requestPermission()) === "granted";
        }
        if (perm) {
          sendNotification({ title: "MedGrapher", body: "Результаты готовы", icon: NOTIFY_ICON });
        }
      }
      const sep = (result as string).lastIndexOf("&&&");
      const structured = sep !== -1
        ? (result as string).substring(sep + 3).trim()
        : result;
      invoke("save_history", {
        diagnoses: diagnoses.trim(),
        scales,
        response: structured,
      }).catch(() => {});
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      send();
    }
  }

  function printPdf() {
    const now = new Date();
    const ts = now.toISOString().replace("T", " ").substring(0, 19).replace(/:/g, "-");
    const origTitle = document.title;
    document.title = `Вывод МКФ — ${ts}`;
    window.print();
    setTimeout(() => { document.title = origTitle; }, 100);
  }

  async function loadHistory() {
    showHistory = true;
    selectedHistory = null;
    try {
      historyEntries = await invoke("get_history");
    } catch {
      historyEntries = [];
    }
  }

  async function openHistoryEntry(id: number) {
    try {
      const raw = await invoke("get_history_by_id", { id }) as string;
      selectedHistory = { id, raw };
    } catch {
      selectedHistory = null;
    }
  }

  function closeHistory() {
    showHistory = false;
    selectedHistory = null;
  }

  // ── Справочник (процедуры / противопоказания / МКФ / протокол) ──
  interface RefProcedure {
    name: string;
    apparatus: string;
    parameters: string;
    time: string;
    course: string;
  }
  interface RefContra {
    procedure: string;
    absolute: string;
    relative: string;
  }
  interface RefProcIcf {
    procedure: string;
    icfCodes: string;
    indications: string;
    rationale: string;
    effect: string;
  }
  interface RefDocument {
    id: number;
    title: string;
    source: string;
    content: string;
  }
  interface ReferenceData {
    procedures: RefProcedure[];
    contraindications: RefContra[];
    procIcf: RefProcIcf[];
    documents: RefDocument[];
  }

  let showReference = $state(false);
  let reference = $state<ReferenceData | null>(null);
  let refLoading = $state(false);
  let refError = $state("");
  let refTab = $state<"procedures" | "contra" | "icf" | "protocol">("procedures");
  let refSearch = $state("");

  async function openReference() {
    showReference = true;
    if (reference || refLoading) return;
    refLoading = true;
    refError = "";
    try {
      reference = (await invoke("get_reference_data")) as ReferenceData;
    } catch (e) {
      refError = String(e);
    } finally {
      refLoading = false;
    }
  }

  function closeReference() {
    showReference = false;
  }

  let refProcedures = $derived.by(() => {
    if (!reference) return [];
    const q = refSearch.trim().toLowerCase();
    if (!q) return reference.procedures;
    return reference.procedures.filter((p) =>
      (p.name + " " + p.apparatus + " " + p.parameters).toLowerCase().includes(q),
    );
  });
  let refContra = $derived.by(() => {
    if (!reference) return [];
    const q = refSearch.trim().toLowerCase();
    if (!q) return reference.contraindications;
    return reference.contraindications.filter((c) =>
      (c.procedure + " " + c.absolute + " " + c.relative).toLowerCase().includes(q),
    );
  });
  let refIcf = $derived.by(() => {
    if (!reference) return [];
    const q = refSearch.trim().toLowerCase();
    if (!q) return reference.procIcf;
    return reference.procIcf.filter((p) =>
      (p.procedure + " " + p.icfCodes + " " + p.indications).toLowerCase().includes(q),
    );
  });

  let resultContainer: HTMLElement;

  function applyColumnVisibility() {
    const el = resultContainer;
    if (!el || applicableScales.length === 0) return;

    const tables = el.querySelectorAll("table");
    for (const table of tables) {
      const headerRow = table.querySelector("tr");
      if (!headerRow) continue;
      const headers = headerRow.querySelectorAll("th, td");

      for (const scale of applicableScales) {
        let colIdx = -1;
        for (let i = 0; i < headers.length; i++) {
          if ((headers[i].textContent?.trim() || "") === scale) {
            colIdx = i;
            break;
          }
        }
        if (colIdx === -1) continue;

        const show = visibleScales.includes(scale);
        for (const row of table.querySelectorAll("tr")) {
          const cell = row.children[colIdx] as HTMLElement | undefined;
          if (cell) cell.style.display = show ? "" : "none";
        }
      }
    }
  }

  function setupContraTooltips() {
    const el = resultContainer;
    if (!el) return;

    const tables = el.querySelectorAll("table");
    if (tables.length === 0) return;

    const table = tables[0];
    const rows = table.querySelectorAll("tr");
    if (rows.length === 0) return;

    const headerCells = rows[0].querySelectorAll("th, td");
    let physioIdx = -1;
    let contraPhysioIdx = -1;
    let lfcIdx = -1;
    let contraLfcIdx = -1;

    for (let i = 0; i < headerCells.length; i++) {
      const t = (headerCells[i].textContent || "").trim().toLowerCase();
      const hasPhysio = t.includes("физио");
      const hasLfk = t.includes("лфк");
      const hasContra = t.includes("против");
      if (hasPhysio && !hasContra) physioIdx = i;
      if (hasPhysio && hasContra) contraPhysioIdx = i;
      if (hasLfk && !hasContra) lfcIdx = i;
      if (hasLfk && hasContra) contraLfcIdx = i;
    }

    if (physioIdx === -1 && lfcIdx === -1) return;

    if (contraPhysioIdx >= 0) {
      (rows[0].children[contraPhysioIdx] as HTMLElement).style.display = "none";
    }
    if (contraLfcIdx >= 0) {
      (rows[0].children[contraLfcIdx] as HTMLElement).style.display = "none";
    }

    for (const row of rows) {
      if (contraPhysioIdx >= 0) {
        const c = row.children[contraPhysioIdx] as HTMLElement | undefined;
        if (c) c.style.display = "none";
      }
      if (contraLfcIdx >= 0) {
        const c = row.children[contraLfcIdx] as HTMLElement | undefined;
        if (c) c.style.display = "none";
      }
    }

    for (let r = 1; r < rows.length; r++) {
      const cells = rows[r].children;

      if (physioIdx >= 0) {
        const cell = cells[physioIdx] as HTMLElement | undefined;
        if (cell) {
          const cellText = (cell.textContent || "").trim();
          const parts: string[] = [];

          // Оборудование и параметры из Процедуры.csv
          const details = collectProcDetails(cellText);
          for (const d of details) parts.push(d);
          if (details.length === 0) {
            for (const part of cellText.split(/[,;]/).map((s) => s.trim()).filter(Boolean)) {
              const info = findProcedure(part);
              if (info && info.equipment && info.equipment !== "-") {
                parts.push(part + " — Оборудование: " + info.equipment);
              }
            }
          }

          // Противопоказания
          if (contraPhysioIdx >= 0) {
            const contra = cells[contraPhysioIdx] as HTMLElement | undefined;
            const t = contra ? (contra.textContent || "").trim() : "";
            if (t) parts.push("Противопоказания: " + t);
          }

          if (parts.length) {
            cell.setAttribute("data-contra", parts.join("\n"));
            cell.classList.add("with-contra");
          }
        }
      }

      if (lfcIdx >= 0) {
        const cell = cells[lfcIdx] as HTMLElement | undefined;
        if (cell) {
          const cellText = (cell.textContent || "").trim();
          const parts: string[] = [];

          // Оборудование из ЛФК.csv
          const details = collectLfkDetails(cellText);
          for (const d of details) parts.push(d);

          // Противопоказания
          if (contraLfcIdx >= 0) {
            const contra = cells[contraLfcIdx] as HTMLElement | undefined;
            const t = contra ? (contra.textContent || "").trim() : "";
            if (t) parts.push("Противопоказания: " + t);
          }

          if (parts.length) {
            cell.setAttribute("data-contra", parts.join("\n"));
            cell.classList.add("with-contra");
          }
        }
      }
    }
  }

  $effect(() => {
    renderedHtml;
    visibleScales;
    if (resultContainer) {
      setTimeout(() => {
        applyColumnVisibility();
        setupContraTooltips();
      }, 0);
    }
  });
</script>

<svelte:head>
  <title>MedGrapher</title>
  <style media="print">
    header, aside, .pdf-btn, .history-panel, .reference-panel, .settings-modal, .settings-overlay { display:none !important; }
    .shell { padding:0 !important; max-width:100% !important; }
    .layout { display:block !important; }
    main { overflow:visible !important; }
    body { background:#fff !important; color:#000 !important; }
    table { min-width:0 !important; font-size:0.72rem; }
    th { background:#d4d4d8 !important; color:#000 !important; border-color:#666 !important; }
    td { border-color:#999 !important; color:#000 !important; }
    .with-contra { text-decoration:none !important; }
    .with-contra::after { display:none !important; }
  </style>
</svelte:head>

<div class="shell">
  <header>
    <h1>MedGrapher</h1>
    <span class="subtitle">МКФ · Физио · ЛФК через OpenRouter</span>
    <div class="header-actions">
      <button class="gear-btn" onclick={openReference} title="Справочник">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/></svg>
      </button>
      <button class="gear-btn" onclick={loadHistory} title="История запросов">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
      </button>
      <button class="gear-btn" onclick={() => showSettings = !showSettings} title="Настройки">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/></svg>
      </button>
    </div>
  </header>

  {#if showSettings}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="settings-overlay" role="presentation" onclick={closeSettings}></div>
    <div class="settings-modal" transition:fade={{ duration: 150 }}>
      <div class="settings-header">
        <span>⚙️ Настройки</span>
        <button class="gear-btn" onclick={closeSettings} title="Закрыть">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <div class="setting-row">
        <label for="theme-toggle">🌙 Тёмная тема</label>
        <button
          class="toggle"
          class:active={darkMode}
          onclick={() => darkMode = !darkMode}
          role="switch"
          aria-checked={darkMode}
          aria-label="Переключить тёмную тему"
          id="theme-toggle"
        >
          <span class="toggle-knob"></span>
        </button>
      </div>

      <div class="setting-row">
        <label for="auto-clear-toggle">🗑️ Автоочистка старых записей</label>
        <button
          class="toggle"
          class:active={autoClearEnabled}
          onclick={() => autoClearEnabled = !autoClearEnabled}
          role="switch"
          aria-checked={autoClearEnabled}
          aria-label="Автоочистка истории"
          id="auto-clear-toggle"
        >
          <span class="toggle-knob"></span>
        </button>
      </div>

      {#if autoClearEnabled}
        <div class="slider-row">
          <label for="retention-slider">
            Хранить записи: <strong>{retentionDays} дн.</strong>
          </label>
          <input
            id="retention-slider"
            type="range"
            min="1"
            max="1460"
            bind:value={retentionDays}
          />
          <div class="slider-labels">
            <span>1 день</span>
            <span>4 года</span>
          </div>
        </div>
      {:else}
        <div class="warning-text">
          Осторожно! Долгая неочистка базы данных ведет к бесконечному накоплению памяти.
        </div>
      {/if}

      <div class="setting-row">
        <label for="notify-toggle">🔔 Присылать уведомления при получении результатов</label>
        <button
          class="toggle"
          class:active={notifyEnabled}
          onclick={() => notifyEnabled = !notifyEnabled}
          role="switch"
          aria-checked={notifyEnabled}
          aria-label="Уведомления о результатах"
          id="notify-toggle"
        >
          <span class="toggle-knob"></span>
        </button>
      </div>

      <label for="model-select">🤖 Модель (OpenRouter)</label>
      {#if modelRegistry.length > 0}
        <select id="model-select" class="model-select" bind:value={selectedModel} onchange={persistModels}>
          {#each modelRegistry as m (m.id)}
            <option value={m.id}>{m.name}</option>
          {/each}
        </select>
      {:else}
        <div class="registry-empty">Список моделей пуст — добавьте модель по ID ниже.</div>
      {/if}

      <div class="add-model-row">
        <input
          id="new-model"
          type="text"
          bind:value={newModelId}
          placeholder="ID модели, например anthropic/claude-3.5-sonnet"
          onkeydown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addModel();
            }
          }}
        />
        <button class="add-btn" onclick={addModel} disabled={!newModelId.trim()}>+ Добавить</button>
      </div>

      {#if modelRegistry.length > 0}
        <ul class="model-list">
          {#each modelRegistry as m (m.id)}
            <li class:active={m.id === selectedModel}>
              <button class="model-pick" onclick={() => selectModel(m.id)} title={m.id}>
                <span class="model-name">{m.name}</span>
                <span class="model-id">{m.id}</span>
              </button>
              <button class="model-del" onclick={() => removeModel(m.id)} title="Удалить модель">✕</button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="registry-hint">
        Скопируйте ID модели со страницы <code>openrouter.ai/models</code> и добавьте в реестр.
        Можно добавить несколько и переключаться между ними.
      </div>

      <label for="k">API-ключ OpenRouter</label>
      <input
        id="k"
        type="password"
        bind:value={apiKey}
        placeholder="sk-or-v1-..."
      />
      <div class="instructions">
        <strong>Как получить ключ:</strong>
        <ol>
          <li>Открой <code>openrouter.ai/keys</code> и войди в аккаунт</li>
          <li>Нажми <strong>Create Key</strong> и скопируй ключ (начинается с <code>sk-or-</code>)</li>
          <li>Вставь его в поле выше</li>
        </ol>
      </div>
    </div>
  {/if}

  {#if showHistory}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="settings-overlay" role="presentation" onclick={closeHistory}></div>
    <div class="history-panel" transition:fade={{ duration: 150 }}>
      <div class="settings-header">
        <span>📋 История запросов</span>
        <button class="gear-btn" onclick={closeHistory} title="Закрыть">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      {#if selectedHistory}
        <button class="back-btn" onclick={() => selectedHistory = null}>
          ← Назад к списку
        </button>
        <div class="history-full">
          {@html marked(selectedHistory.raw)}
        </div>
      {:else}
        <div class="history-list">
          {#if historyEntries.length === 0}
            <div class="history-empty">История пуста</div>
          {:else}
            {#each historyEntries as entry}
              <button class="history-item" onclick={() => openHistoryEntry(entry.id)}>
                <div class="history-diag">{entry.diagnoses}</div>
                <div class="history-date">{entry.created_at}</div>
              </button>
            {/each}
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if showReference}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="settings-overlay" role="presentation" onclick={closeReference}></div>
    <div class="history-panel reference-panel" transition:fade={{ duration: 150 }}>
      <div class="settings-header">
        <span>📚 Справочник</span>
        <button class="gear-btn" onclick={closeReference} title="Закрыть">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>

      <div class="ref-tabs">
        <button class:active={refTab === "procedures"} onclick={() => { refTab = "procedures"; refSearch = ""; }}>Процедуры</button>
        <button class:active={refTab === "contra"} onclick={() => { refTab = "contra"; refSearch = ""; }}>Противопоказания</button>
        <button class:active={refTab === "icf"} onclick={() => { refTab = "icf"; refSearch = ""; }}>Процедуры ↔ МКФ</button>
        <button class:active={refTab === "protocol"} onclick={() => { refTab = "protocol"; refSearch = ""; }}>Протокол</button>
      </div>

      {#if refLoading}
        <div class="loading">Загрузка справочника…</div>
      {:else if refError}
        <div class="error">{refError}</div>
      {:else if reference}
        {#if refTab !== "protocol"}
          <input
            class="ref-search"
            type="text"
            bind:value={refSearch}
            placeholder="Поиск по названию…"
          />
        {/if}
        <div class="ref-body">
          {#if refTab === "procedures"}
            <table class="ref-table">
              <thead><tr><th>Процедура</th><th>Аппарат</th><th>Параметры</th><th>Время</th><th>Курс</th></tr></thead>
              <tbody>
                {#each refProcedures as p (p.name)}
                  <tr>
                    <td class="ref-name">{p.name}</td>
                    <td>{p.apparatus || "—"}</td>
                    <td>{p.parameters || "—"}</td>
                    <td>{p.time || "—"}</td>
                    <td>{p.course || "—"}</td>
                  </tr>
                {/each}
                {#if refProcedures.length === 0}<tr><td colspan="5" class="ref-empty">Ничего не найдено</td></tr>{/if}
              </tbody>
            </table>
          {:else if refTab === "contra"}
            <table class="ref-table">
              <thead><tr><th>Процедура</th><th>Абсолютные противопоказания</th><th>Относительные противопоказания</th></tr></thead>
              <tbody>
                {#each refContra as c (c.procedure)}
                  <tr>
                    <td class="ref-name">{c.procedure}</td>
                    <td>{c.absolute || "—"}</td>
                    <td>{c.relative || "—"}</td>
                  </tr>
                {/each}
                {#if refContra.length === 0}<tr><td colspan="3" class="ref-empty">Ничего не найдено</td></tr>{/if}
              </tbody>
            </table>
          {:else if refTab === "icf"}
            <table class="ref-table">
              <thead><tr><th>Процедура</th><th>Коды МКФ</th><th>Показания</th><th>Ожидаемый эффект</th></tr></thead>
              <tbody>
                {#each refIcf as p (p.procedure)}
                  <tr>
                    <td class="ref-name">{p.procedure}</td>
                    <td class="ref-icf">{p.icfCodes || "—"}</td>
                    <td>{p.indications || "—"}</td>
                    <td>{p.effect || "—"}</td>
                  </tr>
                {/each}
                {#if refIcf.length === 0}<tr><td colspan="4" class="ref-empty">Ничего не найдено</td></tr>{/if}
              </tbody>
            </table>
          {:else}
            <div class="ref-protocol">
              {#each reference.documents as doc (doc.id)}
                <h3>{doc.title}</h3>
                <div class="ref-source">Источник: {doc.source}</div>
                <pre>{doc.content}</pre>
              {/each}
              {#if reference.documents.length === 0}
                <div class="ref-empty">Документы не найдены</div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  <div class="layout">
    <aside>
      <div class="card input-card">
        <label for="d">
          <span>🩺</span> Клинические диагнозы
        </label>
        <textarea
          id="d"
          bind:value={diagnoses}
          onkeydown={handleKeydown}
          rows="5"
          placeholder="коксоартроз, инфаркт головного мозга, дорсопатия, ..."
        ></textarea>

        <div class="row">
          <button onclick={send} disabled={loading || !diagnoses.trim()}>
            {#if loading}
              <span class="spinner"></span>
              Отправка...
            {:else}
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
              Сгенерировать
            {/if}
          </button>
          <span class="hint">Ctrl+Enter</span>

          {#if renderedHtml}
            <button class="pdf-btn" onclick={printPdf} title="Сохранить как PDF">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/><polyline points="10 9 9 9 8 9"/></svg>
              PDF
            </button>
          {/if}
        </div>
      </div>

      {#if applicableScales.length > 0}
        <div class="card filters-card" transition:fade={{ duration: 250 }}>
          <span class="filters-label">Применимые шкалы</span>
          <div class="chips">
            {#each applicableScales as s}
              <label class="chip" class:active={visibleScales.includes(s)}>
                <input type="checkbox" checked={visibleScales.includes(s)} onchange={() => toggleVisibleScale(s)} />
                <span>{s}</span>
              </label>
            {/each}
          </div>
        </div>
      {/if}

      {#if error}
        <div class="error" transition:fade={{ duration: 250 }}>{error}</div>
      {/if}
    </aside>

    <main bind:this={resultContainer}>
      {#if loading}
        <div class="loading">
          <div class="dots"><span></span><span></span><span></span></div>
          Модель обрабатывает запрос…
        </div>
      {:else if renderedHtml}
        <div class="tables">
          {@html renderedHtml}
        </div>
      {:else}
        <div class="empty">
          <p>Результаты появятся здесь</p>
        </div>
      {/if}
    </main>
  </div>
</div>

<style>
  :global(:root) {
    --bg: #f0f4f8;
    --surface: #ffffff;
    --text: #0f172a;
    --text2: #64748b;
    --border: #cbd5e1;
    --border-strong: #94a3b8;
    --accent: #6366f1;
    --accent-hover: #4f46e5;
    --glow: rgba(99,102,241,0.25);
    --radius: 10px;
    --shadow: 0 1px 3px rgba(0,0,0,.1);
  }

  :global([data-theme="dark"]) {
    --bg: #0b1120;
    --surface: #141d2b;
    --text: #e2e8f0;
    --text2: #8899aa;
    --border: #334155;
    --border-strong: #4b5b6f;
  }

  :global(*) { box-sizing:border-box; }
  :global(body) {
    margin:0;
    font-family: -apple-system,BlinkMacSystemFont,'Segoe UI',system-ui,sans-serif;
    background:var(--bg);
    color:var(--text);
    line-height:1.5;
  }

  .shell {
    max-width:1360px;
    margin:0 auto;
    padding:20px 24px;
    display:flex;
    flex-direction:column;
    gap:18px;
    height:100vh;
  }

  header {
    display:flex;
    align-items:baseline;
    gap:12px;
    flex-shrink:0;
  }

  h1 {
    margin:0;
    font-size:1.35rem;
    font-weight:700;
    letter-spacing:-.02em;
    background:linear-gradient(135deg,#6366f1,#a78bfa);
    -webkit-background-clip:text;
    background-clip:text;
    -webkit-text-fill-color:transparent;
  }

  .subtitle {
    font-size:0.82rem;
    color:var(--text2);
  }

  .layout {
    display:grid;
    grid-template-columns:360px 1fr;
    gap:18px;
    flex:1;
    min-height:0;
  }

  aside {
    display:flex;
    flex-direction:column;
    gap:12px;
    overflow-y:auto;
  }

  .card {
    background:var(--surface);
    border:1.5px solid var(--border);
    border-radius:var(--radius);
    box-shadow:var(--shadow);
  }

  .input-card {
    padding:16px;
    display:flex;
    flex-direction:column;
    gap:12px;
  }

  .input-card label {
    display:flex;
    align-items:center;
    gap:6px;
    font-weight:600;
    font-size:0.88rem;
  }

  textarea {
    font-family:inherit;
    font-size:0.92rem;
    padding:10px 12px;
    border:2px solid var(--border-strong);
    border-radius:8px;
    background:var(--surface);
    color:var(--text);
    outline:none;
    width:100%;
    resize:vertical;
    min-height:80px;
    transition:border-color .2s,box-shadow .2s;
    line-height:1.5;
  }

  textarea:focus {
    border-color:var(--accent);
    box-shadow:0 0 0 3px var(--glow);
  }

  textarea::placeholder {
    color:var(--text2);
    opacity:.6;
  }

  .row {
    display:flex;
    align-items:center;
    gap:10px;
  }

  button {
    font-family:inherit;
    font-size:0.9rem;
    padding:10px 20px;
    border:none;
    border-radius:8px;
    background:linear-gradient(135deg,#6366f1,#8b5cf6);
    color:#fff;
    font-weight:600;
    cursor:pointer;
    display:inline-flex;
    align-items:center;
    gap:7px;
    transition:transform .12s,box-shadow .2s;
    box-shadow:0 3px 10px var(--glow);
  }

  button:hover:not(:disabled) {
    transform:translateY(-1px);
    box-shadow:0 5px 16px var(--glow);
  }

  button:disabled {
    opacity:.45;
    cursor:not-allowed;
    box-shadow:none;
  }

  .spinner {
    width:14px;
    height:14px;
    border:2px solid rgba(255,255,255,.3);
    border-top-color:#fff;
    border-radius:50%;
    animation:spin .6s linear infinite;
  }

  @keyframes spin { to { transform:rotate(360deg); } }

  .hint {
    font-size:0.78rem;
    color:var(--text2);
  }

  .filters-card {
    padding:12px 14px;
  }

  .filters-label {
    display:block;
    font-weight:600;
    font-size:0.82rem;
    color:var(--text2);
    margin-bottom:8px;
  }

  .chips {
    display:flex;
    flex-wrap:wrap;
    gap:6px;
  }

  .chip {
    display:inline-flex;
    align-items:center;
    gap:4px;
    padding:3px 10px;
    border-radius:20px;
    font-size:0.8rem;
    cursor:pointer;
    border:1.5px solid var(--border-strong);
    background:var(--bg);
    user-select:none;
    transition:border-color .15s,background .15s;
  }

  .chip.active {
    border-color:var(--accent);
    background:color-mix(in srgb,var(--accent) 10%,transparent);
  }

  .chip input { display:none; }

  .error {
    background:color-mix(in srgb,#ef4444 8%,transparent);
    border:1px solid color-mix(in srgb,#ef4444 30%,transparent);
    border-radius:8px;
    padding:10px 14px;
    font-size:0.84rem;
    color:#b91c1c;
  }

  :global([data-theme="dark"]) .error { color:#fca5a5; }

  .header-actions {
    margin-left:auto;
    display:flex;
    align-items:center;
    gap:4px;
  }

  .gear-btn {
    background:none;
    border:none;
    color:var(--text2);
    cursor:pointer;
    padding:4px;
    border-radius:6px;
    display:flex;
    transition:color .15s,background .15s;
  }
  .gear-btn:hover { color:var(--text); background:color-mix(in srgb,var(--text2) 10%,transparent); }

  .settings-overlay {
    position:fixed; inset:0; z-index:9;
    background:rgba(0,0,0,.3);
  }
  .settings-modal {
    position:fixed; top:50%; left:50%; translate:-50% -50%; z-index:10;
    background:var(--surface); border:1.5px solid var(--border);
    border-radius:12px; padding:20px; min-width:400px; max-width:90vw;
    box-shadow:0 8px 32px rgba(0,0,0,.25);
    display:flex; flex-direction:column; gap:14px;
  }
  .settings-header {
    display:flex; align-items:center; gap:8px;
    font-weight:600; font-size:0.95rem;
  }
  .settings-header .gear-btn { margin-left:auto; }

  .settings-modal label {
    font-weight:600; font-size:0.84rem;
    display:flex; align-items:center; gap:6px;
  }
  .settings-modal input {
    font-family:inherit; font-size:0.85rem;
    padding:9px 12px;
    border:2px solid var(--border-strong);
    border-radius:8px; background:var(--surface); color:var(--text);
    outline:none; width:100%;
    transition:border-color .2s,box-shadow .2s;
  }
  .settings-modal input:focus,
  .settings-modal select:focus {
    border-color:var(--accent);
    box-shadow:0 0 0 3px var(--glow);
  }
  .settings-modal select {
    font-family:inherit; font-size:0.85rem;
    padding:9px 12px;
    border:2px solid var(--border-strong);
    border-radius:8px; background:var(--surface); color:var(--text);
    outline:none; width:100%;
    transition:border-color .2s,box-shadow .2s;
    cursor:pointer;
  }
  /* Нативный попап списка рендерится WebView2 с системным белым фоном, а текст
     наследует цвет темы — задаём option'ам явные цвета для обеих тем */
  .settings-modal select option {
    background:#ffffff;
    color:#0f172a;
  }
  :global([data-theme="dark"]) .settings-modal select option {
    background:#141d2b;
    color:#e2e8f0;
  }

  .add-model-row {
    display:flex;
    gap:8px;
  }
  .add-model-row input {
    flex:1;
    min-width:0;
  }
  .add-btn {
    flex-shrink:0;
    padding:9px 14px;
    font-size:0.82rem;
    white-space:nowrap;
  }
  .model-list {
    list-style:none;
    margin:0;
    padding:0;
    display:flex;
    flex-direction:column;
    gap:6px;
    max-height:220px;
    overflow-y:auto;
  }
  .model-list li {
    display:flex;
    align-items:stretch;
    gap:6px;
    border:1.5px solid var(--border);
    border-radius:8px;
    background:var(--bg);
    overflow:hidden;
    transition:border-color .15s;
  }
  .model-list li.active {
    border-color:var(--accent);
    background:color-mix(in srgb,var(--accent) 10%,transparent);
  }
  .model-pick {
    flex:1;
    min-width:0;
    display:flex;
    flex-direction:column;
    align-items:flex-start;
    gap:2px;
    padding:8px 10px;
    background:transparent;
    border:none;
    box-shadow:none;
    color:var(--text);
    text-align:left;
    font-weight:500;
    cursor:pointer;
  }
  .model-pick:hover { transform:none; box-shadow:none; }
  .model-name {
    font-size:0.84rem;
    font-weight:600;
    max-width:100%;
    overflow:hidden;
    text-overflow:ellipsis;
    white-space:nowrap;
  }
  .model-id {
    font-size:0.72rem;
    color:var(--text2);
    font-family:ui-monospace,SFMono-Regular,Menlo,monospace;
    max-width:100%;
    overflow:hidden;
    text-overflow:ellipsis;
    white-space:nowrap;
  }
  .model-del {
    flex-shrink:0;
    width:34px;
    padding:0;
    background:transparent;
    border:none;
    border-left:1.5px solid var(--border);
    color:var(--text2);
    font-size:0.9rem;
    box-shadow:none;
    cursor:pointer;
  }
  .model-del:hover:not(:disabled) {
    color:#ef4444;
    background:color-mix(in srgb,#ef4444 10%,transparent);
    transform:none;
    box-shadow:none;
  }
  .registry-empty {
    font-size:0.8rem;
    color:var(--text2);
    background:var(--bg);
    padding:10px 12px;
    border-radius:8px;
    border:1.5px dashed var(--border-strong);
  }
  .registry-hint {
    font-size:0.74rem;
    line-height:1.5;
    color:var(--text2);
  }
  .registry-hint code {
    background:color-mix(in srgb,var(--text2) 12%,transparent);
    padding:1px 5px;
    border-radius:4px;
    font-size:0.72rem;
  }

  .instructions {
    font-size:0.8rem; line-height:1.6;
    color:var(--text2); background:var(--bg);
    padding:12px 14px; border-radius:8px;
  }
  .instructions ol { margin:6px 0 0; padding-left:20px; }
  .instructions li { margin-bottom:4px; }
  .instructions code {
    background:color-mix(in srgb,var(--text2) 12%,transparent);
    padding:1px 5px; border-radius:4px;
    font-size:0.78rem;
  }

  aside { scrollbar-width:thin; }

  .setting-row {
    display:flex;
    align-items:center;
    justify-content:space-between;
    gap:12px;
  }
  .setting-row label {
    margin:0;
  }

  .toggle {
    position:relative;
    width:44px;
    height:24px;
    border-radius:12px;
    border:2px solid var(--border-strong);
    background:var(--border);
    cursor:pointer;
    padding:0;
    transition:background .2s,border-color .2s;
    flex-shrink:0;
  }
  .toggle.active {
    background:var(--accent);
    border-color:var(--accent);
  }
  .toggle-knob {
    position:absolute;
    top:2px;
    left:2px;
    width:16px;
    height:16px;
    border-radius:50%;
    background:#fff;
    transition:transform .2s;
    box-shadow:0 1px 3px rgba(0,0,0,.2);
  }
  .toggle.active .toggle-knob {
    transform:translateX(20px);
  }

  .slider-row {
    display:flex;
    flex-direction:column;
    gap:6px;
  }
  .slider-row label {
    font-size:0.82rem;
    margin:0;
  }
  .slider-row input[type="range"] {
    width:100%;
    accent-color:var(--accent);
  }
  .slider-labels {
    display:flex;
    justify-content:space-between;
    font-size:0.72rem;
    color:var(--text2);
  }

  .warning-text {
    font-size:0.78rem;
    color:#b91c1c;
    background:color-mix(in srgb,#ef4444 8%,transparent);
    border:1px solid color-mix(in srgb,#ef4444 25%,transparent);
    border-radius:6px;
    padding:8px 12px;
    line-height:1.5;
  }
  :global([data-theme="dark"]) .warning-text { color:#fca5a5; }

  .history-panel {
    position:fixed; top:50%; left:50%; translate:-50% -50%; z-index:10;
    background:var(--surface); border:1.5px solid var(--border);
    border-radius:12px; padding:20px; width:520px; max-width:92vw; max-height:80vh;
    box-shadow:0 8px 32px rgba(0,0,0,.25);
    display:flex; flex-direction:column; gap:12px;
  }
  .back-btn {
    background:none;
    border:none;
    color:var(--accent);
    font-size:0.84rem;
    cursor:pointer;
    padding:0;
    text-align:left;
    font-weight:600;
    box-shadow:none;
    align-self:flex-start;
  }
  .back-btn:hover { text-decoration:underline; }

  .history-list {
    overflow-y:auto;
    display:flex;
    flex-direction:column;
    gap:6px;
    max-height:60vh;
  }
  .history-empty {
    text-align:center;
    color:var(--text2);
    padding:24px;
    font-size:0.9rem;
  }
  .history-item {
    display:flex;
    flex-direction:column;
    gap:3px;
    background:var(--bg);
    border:1.5px solid var(--border);
    border-radius:8px;
    padding:10px 14px;
    cursor:pointer;
    text-align:left;
    box-shadow:none;
    color:var(--text);
    font-weight:400;
    transition:border-color .15s;
    min-width:0;
    overflow:hidden;
  }
  .history-item:hover {
    border-color:var(--accent);
    transform:none;
    box-shadow:0 0 0 2px var(--glow);
  }
  .history-date {
    font-size:0.76rem;
    color:var(--accent);
    font-weight:600;
  }
  .history-diag {
    font-size:0.83rem;
    font-weight:600;
    line-height:1.3;
    overflow:hidden;
    text-overflow:ellipsis;
    white-space:nowrap;
  }

  .history-full {
    overflow-y:auto;
    overflow-x:auto;
    max-height:60vh;
    font-size:0.85rem;
    line-height:1.5;
    word-break:break-word;
  }
  .history-full :global(table) {
    font-size:0.74rem;
    border-collapse:collapse;
    width:100%;
    table-layout:fixed;
  }
  .history-full :global(th),
  .history-full :global(td) {
    border:1.5px solid var(--border-strong);
    padding:4px 7px;
    text-align:left;
    vertical-align:top;
    word-break:break-word;
    overflow-wrap:break-word;
  }
  .history-full :global(th) {
    background:linear-gradient(135deg,#6366f1,#8b5cf6);
    color:#fff;
    font-weight:600;
  }

  .reference-panel {
    width: 900px;
    max-width: 94vw;
    max-height: 86vh;
  }
  .ref-tabs {
    display:flex;
    gap:6px;
    flex-wrap:wrap;
  }
  .ref-tabs button {
    padding:7px 12px;
    font-size:0.8rem;
    border-radius:8px;
    border:1.5px solid var(--border-strong);
    background:var(--bg);
    color:var(--text);
    box-shadow:none;
    font-weight:600;
  }
  .ref-tabs button:hover { transform:none; box-shadow:none; }
  .ref-tabs button.active {
    border-color:var(--accent);
    background:color-mix(in srgb,var(--accent) 12%,transparent);
    color:var(--accent);
  }
  .ref-search {
    font-family:inherit;
    font-size:0.85rem;
    padding:8px 12px;
    border:2px solid var(--border-strong);
    border-radius:8px;
    background:var(--surface);
    color:var(--text);
    outline:none;
    width:100%;
  }
  .ref-search:focus { border-color:var(--accent); box-shadow:0 0 0 3px var(--glow); }
  .ref-body {
    overflow-y:auto;
    overflow-x:auto;
    max-height:58vh;
    flex:1;
    min-height:0;
  }
  .ref-table {
    border-collapse:collapse;
    width:100%;
    font-size:0.76rem;
    line-height:1.45;
  }
  .ref-table th,
  .ref-table td {
    border:1.5px solid var(--border-strong);
    padding:5px 8px;
    text-align:left;
    vertical-align:top;
    word-break:break-word;
    overflow-wrap:break-word;
  }
  .ref-table th {
    background:linear-gradient(135deg,#6366f1,#8b5cf6);
    color:#fff;
    font-weight:600;
    position:sticky;
    top:0;
  }
  .ref-table tr:nth-child(even) td { background:color-mix(in srgb,var(--bg) 50%,transparent); }
  .ref-name { font-weight:600; white-space:nowrap; }
  .ref-icf {
    font-family:ui-monospace,SFMono-Regular,Menlo,monospace;
    font-size:0.72rem;
    color:var(--accent);
  }
  .ref-empty {
    text-align:center;
    color:var(--text2);
    padding:18px;
  }
  .ref-protocol {
    font-size:0.82rem;
    line-height:1.55;
  }
  .ref-protocol h3 { margin:6px 0 2px; }
  .ref-source { color:var(--text2); font-size:0.74rem; margin-bottom:8px; }
  .ref-protocol pre {
    white-space:pre-wrap;
    word-break:break-word;
    background:var(--bg);
    border:1px solid var(--border);
    border-radius:8px;
    padding:12px 14px;
    font-size:0.78rem;
    margin:0 0 18px;
  }

  main {
    overflow-y:auto;
    min-height:0;
    padding-right:4px;
  }

  .loading {
    display:flex;
    align-items:center;
    gap:12px;
    padding:20px;
    color:var(--text2);
    font-size:0.9rem;
  }

  .dots {
    display:flex;
    gap:4px;
  }

  .dots span {
    width:7px;height:7px;
    border-radius:50%;
    background:var(--accent);
    animation:db 1.2s ease-in-out infinite;
  }

  .dots span:nth-child(2) { animation-delay:.15s; }
  .dots span:nth-child(3) { animation-delay:.3s; }

  @keyframes db {
    0%,80%,100% { transform:scale(.6); opacity:.4; }
    40% { transform:scale(1); opacity:1; }
  }

  .empty {
    display:flex;
    align-items:center;
    justify-content:center;
    height:100%;
    color:var(--text2);
    font-size:0.9rem;
    opacity:.5;
  }

  .tables {
    line-height:1.65;
    font-size:0.92rem;
  }

  .tables :global(table) {
    border-collapse:collapse;
    width:100%;
    margin:14px 0;
    font-size:0.81rem;
    min-width:600px;
    border-radius:8px;
    overflow:hidden;
  }

  .tables :global(th),
  .tables :global(td) {
    border:1.5px solid var(--border-strong);
    padding:7px 10px;
    text-align:left;
    vertical-align:top;
    word-break:break-word;
  }

  .tables :global(.with-contra) {
    cursor:help;
    text-decoration:underline;
    text-decoration-style:dotted;
    text-decoration-color:var(--accent);
    text-underline-offset:3px;
    position:relative;
  }

  .tables :global(.with-contra:hover::after) {
    content:attr(data-contra);
    position:absolute;
    bottom:100%;
    left:0;
    margin-bottom:4px;
    background:var(--surface);
    border:1.5px solid var(--accent);
    border-radius:6px;
    padding:8px 12px;
    font-size:0.75rem;
    line-height:1.5;
    color:var(--text);
    white-space:pre-line;
    min-width:220px;
    max-width:420px;
    z-index:100;
    box-shadow:0 4px 16px rgba(0,0,0,.2);
    pointer-events:none;
    text-align:left;
    text-transform:none;
    letter-spacing:normal;
    font-weight:400;
  }

  :global([data-theme="dark"] .with-contra:hover::after) {
    box-shadow:0 4px 16px rgba(0,0,0,.5);
  }

  .tables :global(th) {
    background:linear-gradient(135deg,#6366f1,#8b5cf6);
    color:#fff;
    font-weight:600;
    white-space:nowrap;
    font-size:0.77rem;
    text-transform:uppercase;
    letter-spacing:.04em;
  }

  .tables :global(tr:nth-child(even)) {
    background:color-mix(in srgb,var(--bg) 50%,transparent);
  }

  .tables :global(tr:hover td) {
    background:color-mix(in srgb,var(--accent) 8%,transparent);
  }

  .tables :global(h1),
  .tables :global(h2),
  .tables :global(h3) {
    margin:20px 0 8px;
    color:var(--text);
  }
  .tables :global(h1) { font-size:1.3rem; }
  .tables :global(h2) { font-size:1.1rem; }
  .tables :global(h3) { font-size:1rem; }
  .tables :global(p) { margin:6px 0; }
  .tables :global(code) {
    background:var(--bg);
    padding:1px 5px;
    border-radius:4px;
    font-size:0.85rem;
    border:1px solid var(--border);
  }
  .tables :global(pre) {
    background:var(--bg);
    padding:12px 16px;
    border-radius:8px;
    overflow-x:auto;
    border:1px solid var(--border);
  }

  .pdf-btn {
    font-family:inherit;
    font-size:0.78rem;
    padding:4px 10px;
    border:1.5px solid var(--accent);
    border-radius:6px;
    background:transparent;
    color:var(--accent);
    font-weight:600;
    cursor:pointer;
    display:inline-flex;
    align-items:center;
    gap:4px;
    box-shadow:none;
    transition:background .15s;
    margin-left:auto;
  }
  .pdf-btn:hover {
    background:color-mix(in srgb,var(--accent) 12%,transparent);
  }
</style>
