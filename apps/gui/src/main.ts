import "./styles.css";
import { api } from "./api";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { t, lang, toggleLang, type MessageKey } from "./i18n";
import type {
  DpiColor,
  DpiStage,
  MacroDto,
  Page,
  SoftwareMacro,
  SoftwareMacroEvent,
  Profile,
  StateDto,
  StatusDto,
} from "./types";

const app = document.querySelector<HTMLDivElement>("#app")!;

let state: StateDto | null = null;
let status: StatusDto | null = null;
let page: Page = "dpi";
let busy = false;
let lastBackup: string | null = null;
let macros: MacroDto[] | null = null;
let macrosLoading = false;
let macrosFailed = false;
let macroEditIndex = 0;
let renamingMacro: number | null = null;
let renameDraft: string | null = null;
let selectedKeySlot = 0;
let ladderDrag: {
  stage: number;
  track: HTMLElement;
  startY: number;
  moved: boolean;
} | null = null;
let stageLongPress: {
  index: number;
  timer: number;
  startX: number;
  startY: number;
  armed: boolean;
} | null = null;
const STAGE_LONG_PRESS_MS = 650;
const DPI_VISIBLE_STAGES = 6;
// Короткая пауза склеивает быстрые серии изменений (колесо,
// перетаскивание) в одну запись.
const WRITE_COALESCE_MS = 120;
const DIAL_WHEEL_STEP = 50;
type DialKey = "polling" | "debounce" | "lod";
const DIAL_STEPS: Record<DialKey, number[]> = {
  polling: [125, 250, 500, 1000],
  debounce: Array.from({ length: 21 }, (_, index) => index),
  lod: [1, 2],
};
let pendingTimer: number | undefined;
let dialWheelAccum = 0;
// Отложенная запись: каждое изменение хранит свои значения (run) и умеет
// заново наложиться на профиль (apply), если тот пришёл из мыши.
type PendingTask = {
  apply: (p: Profile) => void;
  run: () => Promise<void>;
  meta?: object;
};
const pendingTasks = new Map<string, PendingTask>();
let inflightTasks: PendingTask[] = [];
let deviceProfile: Profile | null = null;
let structSeq = 0;
let pageEnter = true;
let newStageIndex: number | null = null;
let saveIndicatorTimer: number | undefined;
let deviceWrites: Promise<void> = Promise.resolve();
let timelineResize: {
  index: number;
  startX: number;
  startDelay: number;
} | null = null;
let timelineBlockDrag: {
  index: number;
  startX: number;
  startY: number;
  active: boolean;
  block: HTMLElement;
  dropIndex: number;
} | null = null;
let macroLongPress: {
  index: number;
  timer: number;
  startX: number;
  startY: number;
  armed: boolean;
  row: HTMLElement;
} | null = null;
const MACRO_EVENT_LIMIT = 70;
const MACRO_KEY_FLAG = 0x01;
const MACRO_DELAY_LIMIT = 1000;
const MACRO_RESIZE_MS_PER_PX = 1;
let recording = false;
let shredDrag: {
  startX: number;
  distance: number;
  progress: number;
  root: HTMLElement;
} | null = null;
let clearedMacro: { index: number; events: SoftwareMacroEvent[] } | null = null;
let recordCountdown = 0;
let recordTimer: number | undefined;
let lastRecordedAt = 0;
const recordedDown = new Set<number>();
let macroEvents: SoftwareMacro | null = null;
let macroEventsIndex = -1;
let macroEventsLoading = false;
let toastTimer: number | undefined;
let statusTimer: number | undefined;
const ACTION_ALIAS_KEY = "aj199.actionAliases";
let actionAliases: Record<string, string> = (() => {
  try {
    const parsed = JSON.parse(localStorage.getItem(ACTION_ALIAS_KEY) ?? "{}");
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
})();

const KNOWN_ACTIONS = [
  { action: "left", key: "buttons.left", code: 0x0101 },
  { action: "right", key: "buttons.right", code: 0x0201 },
  { action: "middle", key: "buttons.middle", code: 0x0401 },
  { action: "forward", key: "buttons.forward", code: 0x1001 },
  { action: "back", key: "buttons.back", code: 0x0801 },
  { action: "dpi-plus", key: "buttons.dpiUp", code: 0x0202 },
  { action: "dpi-minus", key: "buttons.dpiDown", code: 0x0302 },
  { action: "dpi-loop", key: "buttons.dpiLoop", code: 0x0102 },
  { action: "disable", key: "buttons.disable", code: 0x000000 },
] as const;

const MACRO_KEY_NAMES: Record<number, string> = (() => {
  const names: Record<number, string> = {
    0x08: "Backspace",
    0x09: "Tab",
    0x0d: "Enter",
    0x10: "Shift",
    0x11: "Ctrl",
    0x12: "Alt",
    0x13: "Pause",
    0x14: "Caps Lock",
    0x1b: "Esc",
    0x20: "Space",
    0x21: "Page Up",
    0x22: "Page Down",
    0x23: "End",
    0x24: "Home",
    0x25: "Left",
    0x26: "Up",
    0x27: "Right",
    0x28: "Down",
    0x2c: "Print Screen",
    0x2d: "Insert",
    0x2e: "Delete",
    0x5b: "Left Win",
    0x5c: "Right Win",
    0x5d: "Menu",
    0x6a: "Num *",
    0x6b: "Num +",
    0x6d: "Num -",
    0x6e: "Num .",
    0x6f: "Num /",
    0x90: "Num Lock",
    0x91: "Scroll Lock",
    0xa0: "Left Shift",
    0xa1: "Right Shift",
    0xa2: "Left Ctrl",
    0xa3: "Right Ctrl",
    0xa4: "Left Alt",
    0xa5: "Right Alt",
    0xba: ";",
    0xbb: "=",
    0xbc: ",",
    0xbd: "-",
    0xbe: ".",
    0xbf: "/",
    0xc0: "`",
    0xdb: "[",
    0xdc: "\\",
    0xdd: "]",
    0xde: "'",
    0xf0: "Media Play/Pause",
    0xf1: "Media Stop",
    0xf2: "Media Prev",
    0xf3: "Volume Up",
    0xf4: "Media Next",
    0xf5: "Volume Down",
    0xf6: "Mute",
  };
  for (let index = 0; index < 26; index += 1) {
    names[0x41 + index] = String.fromCharCode(0x41 + index);
  }
  for (let index = 0; index < 10; index += 1) {
    names[0x30 + index] = String(index);
    names[0x60 + index] = `Num ${index}`;
  }
  for (let index = 0; index < 12; index += 1) {
    names[0x70 + index] = `F${index + 1}`;
  }
  return names;
})();

const UNSUPPORTED_MACRO_KEYS = new Set([0xf5, 0xf6]);
const MACRO_KEYS = Object.entries(MACRO_KEY_NAMES)
  .map(([code, label]) => ({ code: Number(code), label }))
  .filter((item) => !UNSUPPORTED_MACRO_KEYS.has(item.code))
  .sort((left, right) => left.code - right.code);

function macroKeyLabel(code: number): string {
  const name = MACRO_KEY_NAMES[code];
  if (!name) return `Неизвестно (0x${hex(code, 4)})`;
  return UNSUPPORTED_MACRO_KEYS.has(code) ? `${name} (не поддержано)` : name;
}

function macroKeyOptions(current: number): string {
  const known = MACRO_KEYS.some((item) => item.code === current);
  const options = MACRO_KEYS.map(
    (item) =>
      `<option value="${item.code}" ${item.code === current ? "selected" : ""}>${esc(item.label)}</option>`,
  );
  if (!known) {
    options.unshift(
      `<option value="${current}" selected>${esc(macroKeyLabel(current))}</option>`,
    );
  }
  return options.join("");
}

// Основной LED (0x4C): [mode, A, B] парами с checksum. По записям OEM:
// steady — mode 1, A = яркость (уровень 1..10, 10 -> 0xFF), B = 3;
// breathing — mode 2, A = 0xFF, B = скорость 1..5; off — mode 0.
type LedEffect = "steady" | "breathing" | "off";
const LED_EFFECTS: { effect: LedEffect; mode: number; key: MessageKey }[] = [
  { effect: "steady", mode: 1, key: "led.steady" },
  { effect: "breathing", mode: 2, key: "led.breathing" },
  { effect: "off", mode: 0, key: "led.off" },
];
const LED_BRIGHTNESS_MAX = 10;
const LED_SPEED_MAX = 5;

function ledEffect(mode: number): LedEffect {
  return mode === 1 ? "steady" : mode === 2 ? "breathing" : "off";
}

function ledModeOf(effect: LedEffect): number {
  return LED_EFFECTS.find((item) => item.effect === effect)?.mode ?? 0;
}

function ledBrightnessLevel(byte: number): number {
  return Math.max(1, Math.min(LED_BRIGHTNESS_MAX, Math.round(byte / 25)));
}

function ledBrightnessByte(level: number): number {
  return level >= LED_BRIGHTNESS_MAX ? 0xff : level * 25;
}

function ledSpeedLevel(byte: number): number {
  return Math.max(1, Math.min(LED_SPEED_MAX, byte));
}

function ledBreathSeconds(level: number): number {
  return [5, 3.8, 2.8, 2, 1.4][level - 1] ?? 2.8;
}

// Код кнопки с макросом: младший байт 0x06, следующий — номер макроса.
function isMacroCode(code: number): boolean {
  return (code & 0xff) === 0x06;
}

function macroIdOf(code: number): number {
  return (code >> 8) & 0xff;
}

function isMacroOn(code: number, index: number): boolean {
  return isMacroCode(code) && macroIdOf(code) === index;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function esc(value: unknown): string {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function hex(value: number, width = 2): string {
  return value.toString(16).toUpperCase().padStart(width, "0");
}


function toHexColor(color: DpiColor): string {
  return `#${hex(color.r)}${hex(color.g)}${hex(color.b)}`;
}

function parseColor(value: string): [number, number, number] {
  const text = value.replace("#", "");
  if (text.length !== 6) return [255, 255, 255];
  return [
    parseInt(text.slice(0, 2), 16),
    parseInt(text.slice(2, 4), 16),
    parseInt(text.slice(4, 6), 16),
  ];
}

function rgbToHsv(r: number, g: number, b: number): {
  h: number;
  s: number;
  v: number;
} {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const delta = max - min;
  let h = 0;
  if (delta !== 0) {
    if (max === rn) h = ((gn - bn) / delta) % 6;
    else if (max === gn) h = (bn - rn) / delta + 2;
    else h = (rn - gn) / delta + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: max === 0 ? 0 : delta / max, v: max };
}

function hsvToHex(h: number, s: number, v: number): string {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let r = 0;
  let g = 0;
  let b = 0;
  if (h < 60) [r, g, b] = [c, x, 0];
  else if (h < 120) [r, g, b] = [x, c, 0];
  else if (h < 180) [r, g, b] = [0, c, x];
  else if (h < 240) [r, g, b] = [0, x, c];
  else if (h < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return `#${[r, g, b]
    .map((value) =>
      Math.round((value + m) * 255)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

function clampDpi(value: number): number {
  if (!Number.isFinite(value)) return 400;
  const clamped = Math.max(50, Math.min(26000, Math.round(value)));
  return Math.max(50, Math.round(clamped / 50) * 50);
}

const DPI_MIN = 50;
const DPI_MAX = 26000;
const DPI_LOG_MIN = Math.log(DPI_MIN);
const DPI_LOG_RANGE = Math.log(DPI_MAX) - DPI_LOG_MIN;

function dpiLadderPercent(value: number): number {
  const dpi = Math.max(DPI_MIN, Math.min(DPI_MAX, value));
  const ratio = (Math.log(dpi) - DPI_LOG_MIN) / DPI_LOG_RANGE;
  return Math.max(7, Math.min(100, ratio * 100));
}

function dpiFromLadderRatio(ratio: number): number {
  return clampDpi(Math.exp(DPI_LOG_MIN + clamp(ratio, 0, 1) * DPI_LOG_RANGE));
}

function setStageValue(stage: number, value: number): void {
  const content = document.querySelector<HTMLElement>("#content");
  const p = profile();
  if (!content || !p) return;
  const next = clampDpi(value);
  const item = p.dpi_stages[stage];
  if (!item) return;
  item.x = next;
  item.y = next;
  const ladderStage = content.querySelector<HTMLElement>(
    `[data-ladder-stage="${stage}"]`,
  );
  if (ladderStage) {
    const valueNode = ladderStage.querySelector<HTMLElement>(".ladder-value");
    const fill = ladderStage.querySelector<HTMLElement>(".ladder-fill");
    if (valueNode) valueNode.textContent = String(next);
    if (fill) fill.style.height = `${dpiLadderPercent(next)}%`;
  }
  if (stage === activeStageIndex(p)) {
    const hero = content.querySelector<HTMLElement>(".hero-value");
    if (hero) hero.textContent = `${next} × ${next}`;
  }
}

function writeEnabled(): boolean {
  return Boolean(state?.write_enabled);
}

function profile(): Profile | null {
  return state?.profile ?? null;
}

function activeStageIndex(p: Profile): number {
  return Math.min(Math.max(p.current_dpi_stage, 0), p.dpi_stages.length - 1);
}

function activeStage(p: Profile): DpiStage {
  return p.dpi_stages[activeStageIndex(p)] ?? p.dpi_stages[0];
}

function actionLabel(code: number): string {
  const known = KNOWN_ACTIONS.find((item) => item.code === code);
  if (known) return t(known.key);
  if (isMacroCode(code)) {
    const id = macroIdOf(code);
    const name = macros?.find((macro) => macro.index === id)?.name;
    return name ? t("buttons.macroNamed", { name }) : t("buttons.macroId", { id });
  }
  return actionAliases[code] ?? `${t("buttons.unknown")} (0x${hex(code, 6)})`;
}

function persistActionAliases(): void {
  localStorage.setItem(ACTION_ALIAS_KEY, JSON.stringify(actionAliases));
}

function actionOptions(code: number): string {
  const parts: string[] = [];
  let matched = false;
  for (const item of KNOWN_ACTIONS) {
    const selected = item.code === code;
    if (selected) matched = true;
    parts.push(
      `<option value="a:${item.action}" ${selected ? "selected" : ""}>${esc(t(item.key))}</option>`,
    );
  }
  const macroParts = (macros ?? [])
    .filter((macro) => macro.name)
    .map((macro) => {
      const macroCode = (MACRO_KEY_FLAG << 16) | (macro.index << 8) | 0x06;
      const selected = isMacroOn(code, macro.index);
      if (selected) matched = true;
      return `<option value="c:${selected ? code : macroCode}" ${selected ? "selected" : ""}>${esc(t("buttons.macroNamed", { name: macro.name ?? "" }))}</option>`;
    });
  if (!matched) {
    parts.push(
      `<option value="c:${code}" selected>${esc(actionLabel(code))}</option>`,
    );
  }
  if (macroParts.length) {
    parts.push(
      `<optgroup label="${esc(t("nav.macros"))}">${macroParts.join("")}</optgroup>`,
    );
  }
  return parts.join("");
}

function bump(element: Element | null | undefined): void {
  if (!element) return;
  element.classList.remove("bump");
  void (element as HTMLElement).offsetWidth;
  element.classList.add("bump");
}

function setState(next: StateDto): void {
  state = next;
  if (next.backup) lastBackup = next.backup;
  deviceProfile = next.profile ? structuredClone(next.profile) : null;
  reapplyPending();
  updateChrome();
}

function setBusy(value: boolean): void {
  busy = value;
  const overlay = document.querySelector<HTMLDivElement>("#busy");
  overlay?.classList.toggle("hidden", !value);
}

async function loadState(showError = true): Promise<void> {
  try {
    const next = await api.state();
    setState(next);
    if (next.connected) {
      status = {
        connected: true,
        error: null,
        connection: next.connection,
        pid: next.pid,
        firmware: next.firmware,
        battery: next.battery,
        charging: next.charging,
        write_enabled: next.write_enabled,
      };
    } else if (showError) {
      showToast(next.error ?? t("common.notConnected"), "error");
    }
  } catch (error) {
    if (showError) showToast(String(error), "error");
  }
  updateChrome();
  renderPage();
}

async function refreshStatus(): Promise<void> {
  if (busy) return;
  try {
    const next = await api.status();
    const wasOffline = !state?.connected;
    const wasWritable = writeEnabled();
    // Кабель вставили или вынули: профиль перечитывается уже через новое подключение.
    const changed =
      next.connected !== Boolean(state?.connected) ||
      (next.connected && next.pid !== state?.pid);
    status = next;
    if (changed && !wasOffline) {
      await loadState(false);
      return;
    }
    if (next.connected && state) {
      state.battery = next.battery;
      state.charging = next.charging;
      state.connection = next.connection;
      state.firmware = next.firmware;
      state.pid = next.pid;
      state.write_enabled = next.write_enabled;
    }
    if (next.connected && wasOffline) {
      await loadState(false);
      return;
    }
    updateChrome();
    if (next.write_enabled !== wasWritable && canRenderNow()) renderPage();
  } catch {
    /* keep last status */
  }
}

function confirmAction(message: string): Promise<boolean> {
  return new Promise((resolve) => {
    const overlay = document.querySelector<HTMLDivElement>("#confirm");
    const text = document.querySelector<HTMLParagraphElement>("#confirm-text");
    const ok = document.querySelector<HTMLButtonElement>("#confirm-ok");
    const cancel = document.querySelector<HTMLButtonElement>("#confirm-cancel");
    if (!overlay || !text || !ok || !cancel) {
      resolve(false);
      return;
    }
    text.textContent = message;
    overlay.classList.remove("hidden");
    const close = (value: boolean) => {
      overlay.classList.add("hidden");
      ok.onclick = null;
      cancel.onclick = null;
      overlay.onclick = null;
      resolve(value);
    };
    ok.onclick = () => close(true);
    cancel.onclick = () => close(false);
    overlay.onclick = (event) => {
      if (event.target === overlay) close(false);
    };
    ok.focus();
  });
}

function showToast(message: string, kind: "ok" | "error" = "ok"): void {
  const element = document.querySelector<HTMLDivElement>("#toast");
  if (!element) return;
  element.textContent = message;
  element.className = `toast ${kind}`;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    element.className = "toast hidden";
  }, 4200);
}

function showError(error: unknown): void {
  showToast(error instanceof Error ? error.message : String(error), "error");
}

// Версия из bcdDevice: цифры записаны двоично-десятично, 0x1332 -> 13.32.
function firmwareVersion(value: number): string {
  return `${(value >> 8).toString(16)}.${(value & 0xff).toString(16).padStart(2, "0")}`;
}

function batteryTone(level: number): string {
  return level <= 15 ? "low" : level <= 35 ? "mid" : "ok";
}

function batteryGlyph(level: number, charging: boolean): string {
  const value = clamp(level, 0, 100);
  const tone = batteryTone(value);
  return `<span class="battery ${tone} ${charging ? "charging" : ""}" title="${esc(t("status.battery"))}: ${value}%${charging ? " · " + esc(t("status.charging")) : ""}">
    <span class="battery-body"><span class="battery-fill" style="width:${value}%"></span>${charging ? `<svg class="battery-bolt" viewBox="0 0 10 14" aria-hidden="true"><path d="M6 0 0 8h4l-1 6 6-8H5l1-6Z"/></svg>` : ""}</span>
    <span class="battery-cap"></span>
    <span class="battery-text">${value}%</span>
  </span>`;
}

function connectionClass(value: string | undefined): string {
  if (value === "USB") return "usb";
  if (value === "2.4G") return "wireless";
  return "offline";
}

function navItem(target: Page, key: MessageKey, icon: string): string {
  return `<button class="nav-item ${page === target ? "active" : ""}" data-page="${target}">
    <span class="nav-icon">${icon}</span><span>${esc(t(key))}</span>
  </button>`;
}

function iconDpi(): string {
  return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 17h16M6 17V9m4 8V5m4 12v-6m4 6v-3" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
}

function iconButtons(): string {
  return `<svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="3" width="10" height="18" rx="5" fill="none" stroke="currentColor" stroke-width="1.8"/><path d="M12 3v7" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
}

function iconLight(): string {
  return `<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="5" fill="none" stroke="currentColor" stroke-width="1.8"/><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.4 1.4M17.6 17.6 19 19M19 5l-1.4 1.4M6.4 17.6 5 19" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>`;
}

function iconThemes(): string {
  return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a9 9 0 1 0 0 18c1.4 0 2-1 2-2 0-1.3-1-1.6-1-2.8 0-1 .8-1.7 1.8-1.7H17a4 4 0 0 0 4-4C21 6.4 17 3 12 3Z" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/><circle cx="7.5" cy="11" r="1.2" fill="currentColor"/><circle cx="10" cy="7" r="1.2" fill="currentColor"/><circle cx="15" cy="7.5" r="1.2" fill="currentColor"/></svg>`;
}

type ThemeId = "deep" | "sunset" | "graphite" | "phosphor" | "fireflies" | "aurora";
const THEMES: { id: ThemeId; key: MessageKey }[] = [
  { id: "deep", key: "theme.deep" },
  { id: "sunset", key: "theme.sunset" },
  { id: "graphite", key: "theme.graphite" },
  { id: "phosphor", key: "theme.phosphor" },
  { id: "fireflies", key: "theme.fireflies" },
  { id: "aurora", key: "theme.aurora" },
];
// Старое хранилище темы: читается только для переноса в settings.json.
const LEGACY_THEME_KEY = "aj199.theme";
let activeTheme: ThemeId = "deep";

function isThemeId(value: unknown): value is ThemeId {
  return THEMES.some((theme) => theme.id === value);
}

function currentTheme(): ThemeId {
  return activeTheme;
}

async function restoreTheme(): Promise<void> {
  const saved = await api.getTheme().catch(() => null);
  if (isThemeId(saved)) {
    applyTheme(saved);
    return;
  }
  let legacy: string | null = null;
  try {
    legacy = localStorage.getItem(LEGACY_THEME_KEY);
  } catch {
    // нет доступа к хранилищу — тема по умолчанию
  }
  const id = isThemeId(legacy) ? legacy : "deep";
  applyTheme(id);
  api.setTheme(id).catch(() => undefined);
}

// Детерминированный «случай» — фон темы одинаковый от запуска к запуску.
function seeded(seed: number): () => number {
  let value = seed;
  return () => {
    value = (value * 1103515245 + 12345) % 2147483648;
    return value / 2147483648;
  };
}

function themeFxMarkup(id: ThemeId): string {
  const rand = seeded(id.length * 7919 + 17);
  const range = (min: number, max: number) => (min + rand() * (max - min)).toFixed(2);
  if (id === "deep") {
    return Array.from(
      { length: 14 },
      () =>
        `<i class="bubble" style="--x:${range(2, 98)}%;--s:${range(4, 13)}px;--d:${range(16, 32)}s;--delay:-${range(0, 32)}s;--sway:${range(-40, 40)}px"></i>`,
    ).join("");
  }
  if (id === "sunset") {
    return `<div class="sun"></div><div class="horizon"><div class="grid"></div></div>`;
  }
  if (id === "graphite") {
    return `<div class="dotgrid"></div>`;
  }
  if (id === "phosphor") {
    return `<div class="crt-vignette"></div><div class="crt-roll"></div>`;
  }
  if (id === "fireflies") {
    const flies = Array.from(
      { length: 15 },
      () =>
        `<i class="firefly" style="--x:${range(4, 96)}%;--y:${range(10, 95)}%;--dx:${range(-70, 70)}px;--dy:${range(-55, 55)}px;--d:${range(14, 26)}s;--b:${range(3, 7)}s;--delay:-${range(0, 20)}s;--s:${range(2.5, 4.5)}px"></i>`,
    ).join("");
    return `<div class="moon"></div>${flies}`;
  }
  return `<div class="stars"></div><div class="stars far"></div><div class="ribbon r1"></div><div class="ribbon r2"></div><div class="ribbon r3"></div>`;
}

// Лаконичная иконка программы под каждую тему (64×64).
function themeIconSvg(id: ThemeId, uid: string): string {
  const g = `${id}-${uid}`;
  const mouse = (stroke: string, wheel: string, width = 3) =>
    `<path d="M32 13c-8.5 0-13 5.2-13 14.5v9C19 45.6 24.4 51 32 51s13-5.4 13-14.5v-9C45 18.2 40.5 13 32 13Z" fill="none" stroke="${stroke}" stroke-width="${width}"/><path d="M19.5 30.5h25M32 13.5v17" fill="none" stroke="${stroke}" stroke-width="${width * 0.6}" opacity=".55"/><rect x="29.6" y="18.5" width="4.8" height="8" rx="2.4" fill="${wheel}"/>`;
  const icons: Record<ThemeId, string> = {
    deep: `<defs><linearGradient id="b${g}" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#10324a"/><stop offset="1" stop-color="#0b1022"/></linearGradient><linearGradient id="w${g}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#6ee7ff"/><stop offset="1" stop-color="#8b5cf6"/></linearGradient></defs><rect width="64" height="64" rx="16" fill="url(#b${g})"/><path d="M0 46c10-5 20 5 32 0s22-5 32 0v18H0Z" fill="#6ee7ff" opacity=".12"/>${mouse("#cfefff", `url(#w${g})`)}<circle cx="49" cy="15" r="3" fill="none" stroke="#6ee7ff" stroke-width="1.6" opacity=".8"/><circle cx="54" cy="24" r="1.8" fill="none" stroke="#6ee7ff" stroke-width="1.3" opacity=".6"/>`,
    sunset: `<defs><linearGradient id="s${g}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffd27a"/><stop offset=".5" stop-color="#ff9a62"/><stop offset="1" stop-color="#ff4f9a"/></linearGradient></defs><rect width="64" height="64" rx="16" fill="#1c0b18"/><circle cx="32" cy="34" r="22" fill="url(#s${g})"/><path d="M8 38h48M8 44h48M8 50h48" stroke="#1c0b18" stroke-width="2.6"/>${mouse("#1c0b18", "#1c0b18", 3.4)}`,
    graphite: `<rect x="1.5" y="1.5" width="61" height="61" rx="6" fill="#141416" stroke="#3a3b40"/><path d="M6 14V6h8M50 6h8v8M58 50v8h-8M14 58H6v-8" fill="none" stroke="#d6d8dc" stroke-width="1.6"/>${mouse("#e8e9ec", "#e8e9ec", 2.2)}`,
    phosphor: `<rect width="64" height="64" rx="10" fill="#03110a"/><rect x="3" y="3" width="58" height="58" rx="8" fill="none" stroke="#39ff88" stroke-opacity=".35"/><path d="M15 22l12 10-12 10" fill="none" stroke="#39ff88" stroke-width="4.5" stroke-linecap="round" stroke-linejoin="round"/><rect x="31" y="40" width="18" height="4.5" rx="1.5" fill="#39ff88"/>`,
    fireflies: `<defs><radialGradient id="f${g}"><stop offset="0" stop-color="#fff2b0"/><stop offset=".4" stop-color="#ffd36b" stop-opacity=".9"/><stop offset="1" stop-color="#ffd36b" stop-opacity="0"/></radialGradient></defs><circle cx="32" cy="32" r="31" fill="#0e1d15"/><path d="M6 48c4-6 6-12 7-18 1 7 3 12 6 16 2-7 5-13 9-17-1 8 0 14 3 19 3-6 7-10 12-13-3 7-3 12-1 17 4-4 9-7 15-8-4 4-6 9-6 14H6Z" fill="#1d3a28"/>${mouse("#f1e9c6", "#ffd36b", 2.6)}<circle cx="50" cy="17" r="7" fill="url(#f${g})"/><circle cx="50" cy="17" r="2" fill="#fff6c8"/>`,
    aurora: `<defs><linearGradient id="a${g}" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#5ef2c2"/><stop offset="1" stop-color="#b47cff"/></linearGradient></defs><rect width="64" height="64" rx="16" fill="#060b18"/><path d="M4 22c10-10 18 4 28-4s18-10 28 0" fill="none" stroke="url(#a${g})" stroke-width="5" stroke-linecap="round" opacity=".55"/><path d="M4 28c10-8 18 4 28-3s18-7 28 1" fill="none" stroke="url(#a${g})" stroke-width="2" stroke-linecap="round" opacity=".4"/>${mouse("#e9f1ff", `url(#a${g})`, 2.6)}<circle cx="12" cy="44" r="1.2" fill="#fff"/><circle cx="53" cy="47" r="1" fill="#fff"/>`,
  };
  return `<svg viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">${icons[id]}</svg>`;
}

function titlebarInner(): string {
  const id = currentTheme();
  const title = id === "phosphor" ? "aj199@ajazz:~$" : "AJAZZ · AJ199";
  const glyphs = id === "phosphor" ? ["[_]", "[□]", "[x]"] : ["", "", ""];
  return `<div class="tb-brand" data-tauri-drag-region>
      <span class="tb-icon" data-tauri-drag-region>${themeIconSvg(id, "tb")}</span>
      <span class="tb-title" data-tauri-drag-region>${esc(title)}</span>
    </div>
    <div class="tb-fx" data-tauri-drag-region></div>
    <div class="tb-controls">
      <button class="tb-btn tb-min" data-win="min" aria-label="${esc(t("win.minimize"))}" title="${esc(t("win.minimize"))}"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2 6.5h8"/></svg><span class="tb-glyph">${glyphs[0]}</span></button>
      <button class="tb-btn tb-max" data-win="max" aria-label="${esc(t("win.maximize"))}" title="${esc(t("win.maximize"))}"><svg viewBox="0 0 12 12" aria-hidden="true"><rect x="2.2" y="2.2" width="7.6" height="7.6" rx="1.2"/></svg><span class="tb-glyph">${glyphs[1]}</span></button>
      <button class="tb-btn tb-close" data-win="close" aria-label="${esc(t("win.close"))}" title="${esc(t("win.close"))}"><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 2.5l7 7m0-7-7 7"/></svg><span class="tb-glyph">${glyphs[2]}</span></button>
    </div>`;
}

let iconTimer: number | undefined;
let batteryAlerts = true;
let testToastStep = 0;

// Картинка уведомления (728×360, рекомендованный размер hero-изображения Windows).
function themeHeroSvg(id: ThemeId, kind: "low" | "full"): string {
  const full = kind === "full";
  const dots = (items: [number, number, number][], fill: string) =>
    items.map(([x, y, r]) => `<circle cx="${x}" cy="${y}" r="${r}" fill="${fill}"/>`).join("");
  const scenes: Record<ThemeId, { defs: string; scene: string; line: string; fill: string; hollow: string }> = {
    deep: {
      defs: `<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#123a5a"/><stop offset="1" stop-color="#070b18"/></linearGradient><linearGradient id="f" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#6ee7ff"/><stop offset="1" stop-color="#8b5cf6"/></linearGradient>`,
      scene: `<rect width="728" height="360" fill="url(#bg)"/><path d="M0 70c120-40 240 40 364 0s244-40 364 0V0H0Z" fill="#6ee7ff" opacity=".07"/><path d="M0 300c120-30 240 30 364 0s244-30 364 0v60H0Z" fill="#6ee7ff" opacity=".08"/>${[[90, 250, 12], [150, 150, 7], [610, 90, 14], [660, 230, 8], [560, 280, 5], [190, 70, 5]].map(([x, y, r]) => `<circle cx="${x}" cy="${y}" r="${r}" fill="none" stroke="#9fefff" stroke-opacity=".55" stroke-width="2"/>`).join("")}`,
      line: "#d8f4ff",
      fill: "url(#f)",
      hollow: "#0a1a2c",
    },
    sunset: {
      defs: `<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1c0b24"/><stop offset=".6" stop-color="#4a1440"/><stop offset="1" stop-color="#1c0b18"/></linearGradient><linearGradient id="sun" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffd27a"/><stop offset=".5" stop-color="#ff9a62"/><stop offset="1" stop-color="#ff4f9a"/></linearGradient>`,
      scene: `<rect width="728" height="360" fill="url(#bg)"/><circle cx="364" cy="176" r="150" fill="url(#sun)"/>${[214, 240, 264, 286, 304].map((y, i) => `<rect x="0" y="${y}" width="728" height="${5 + i * 2}" fill="#3a1236"/>`).join("")}<rect y="300" width="728" height="60" fill="#1c0b18"/>${[-300, -180, -60, 60, 180, 300].map((x) => `<path d="M364 300 L${364 + x * 2.4} 360" stroke="#ff4f9a" stroke-opacity=".6" stroke-width="2"/>`).join("")}<path d="M0 300H728M0 318H728M0 342H728" stroke="#ff4f9a" stroke-opacity=".5" stroke-width="2"/>`,
      line: "#1c0712",
      fill: "url(#sun)",
      hollow: "#1c0712",
    },
    graphite: {
      defs: `<pattern id="d" width="24" height="24" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r="1.2" fill="#cdd0d5" fill-opacity=".28"/></pattern>`,
      scene: `<rect width="728" height="360" fill="#121214"/><rect width="728" height="360" fill="url(#d)"/><path d="M20 52V20h32M676 20h32v32M708 308v32h-32M52 340H20v-32" fill="none" stroke="#e8e9ec" stroke-width="2"/><path d="M254 262h220M254 256v12M474 256v12" stroke="#9b9da3" stroke-width="1.5"/>`,
      line: "#e8e9ec",
      fill: "#e8e9ec",
      hollow: "#121214",
    },
    phosphor: {
      defs: `<pattern id="s" width="4" height="4" patternUnits="userSpaceOnUse"><rect width="4" height="2" fill="#000" fill-opacity=".28"/></pattern>`,
      scene: `<rect width="728" height="360" fill="#03110a"/><text x="28" y="56" fill="#39ff88" font-family="Cascadia Mono, Consolas, monospace" font-size="24">$ battery --status</text><text x="28" y="334" fill="#39ff88" font-family="Cascadia Mono, Consolas, monospace" font-size="24">${full ? "[ ok ] charge complete" : "[warn] plug in cable"} _</text>`,
      line: "#39ff88",
      fill: "#39ff88",
      hollow: "#03110a",
    },
    fireflies: {
      defs: `<linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#0d2230"/><stop offset="1" stop-color="#08140f"/></linearGradient><radialGradient id="g"><stop offset="0" stop-color="#fff2b0"/><stop offset=".3" stop-color="#ffd36b" stop-opacity=".8"/><stop offset="1" stop-color="#ffd36b" stop-opacity="0"/></radialGradient><linearGradient id="f" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#ffd36b"/><stop offset="1" stop-color="#7fd88f"/></linearGradient>`,
      scene: `<rect width="728" height="360" fill="url(#bg)"/><circle cx="628" cy="76" r="40" fill="#f3e2a6" opacity=".85"/>${[[110, 110, 34], [190, 220, 22], [560, 190, 28], [640, 260, 20], [300, 70, 18]].map(([x, y, r]) => `<circle cx="${x}" cy="${y}" r="${r}" fill="url(#g)"/>`).join("")}<path d="M0 360V300c20-24 28-60 40-90 8 34 14 62 30 84 14-34 26-66 46-92-4 40 0 72 14 98 20-30 38-56 66-74-12 34-12 58-6 82 26-22 56-36 92-44-20 22-30 40-32 66 34-16 72-24 112-24-24 16-38 34-44 56 34-10 74-12 112-4-22 12-36 28-40 46 40-12 84-14 124-6-22 12-36 26-40 40 34-8 66-8 96 2v60Z" fill="#14301f"/>`,
      line: "#f1e9c6",
      fill: "url(#f)",
      hollow: "#0a1a14",
    },
    aurora: {
      defs: `<linearGradient id="r" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#5ef2c2"/><stop offset=".5" stop-color="#5ec8ff"/><stop offset="1" stop-color="#b47cff"/></linearGradient><filter id="b" x="-20%" y="-60%" width="140%" height="220%"><feGaussianBlur stdDeviation="18"/></filter>`,
      scene: `<rect width="728" height="360" fill="#050a18"/><path d="M-20 120c120-90 220 40 360-30s240-70 400 10" fill="none" stroke="url(#r)" stroke-width="56" filter="url(#b)" opacity=".75"/><path d="M-20 170c140-60 240 30 380-20s240-50 390 10" fill="none" stroke="url(#r)" stroke-width="26" filter="url(#b)" opacity=".5"/>${dots([[60, 300, 1.8], [150, 330, 1.4], [240, 290, 1.6], [520, 310, 1.8], [610, 280, 1.5], [690, 330, 1.4], [420, 335, 1.3]], "#fff")}`,
      line: "#e9f1ff",
      fill: "url(#r)",
      hollow: "#050a18",
    },
  };
  const art = scenes[id];
  const fillColor = full ? art.fill : id === "phosphor" ? "#ffb43c" : "#ff5d6c";
  const width = full ? 196 : 34;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="728" height="360" viewBox="0 0 728 360"><defs>${art.defs}</defs>${art.scene}
    <rect x="254" y="126" width="220" height="108" rx="${id === "graphite" || id === "phosphor" ? 6 : 22}" fill="${art.hollow}" fill-opacity=".85" stroke="${art.line}" stroke-width="8"/>
    <rect x="474" y="158" width="14" height="44" rx="4" fill="${art.line}"/>
    <rect x="266" y="138" width="${width}" height="84" rx="${id === "graphite" || id === "phosphor" ? 2 : 12}" fill="${fillColor}"/></svg>`;
}

function svgToCanvas(svg: string, width: number, height: number): Promise<HTMLCanvasElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      canvas.getContext("2d")?.drawImage(image, 0, 0, width, height);
      resolve(canvas);
    };
    image.onerror = () => reject(new Error("svg"));
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  });
}

async function svgToPng(svg: string, width: number, height: number): Promise<number[]> {
  const canvas = await svgToCanvas(svg, width, height);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("png");
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}

async function saveThemeArt(id: ThemeId): Promise<void> {
  const [logo, low, full] = await Promise.all([
    svgToPng(themeIconSvg(id, "art"), 128, 128),
    svgToPng(themeHeroSvg(id, "low"), 728, 360),
    svgToPng(themeHeroSvg(id, "full"), 728, 360),
  ]);
  await api.saveThemeArt(id, logo, low, full);
}

// Иконка окна, панели задач и трея: SVG темы -> пиксели RGBA -> Rust.
function pushThemeIcon(id: ThemeId): void {
  window.clearTimeout(iconTimer);
  iconTimer = window.setTimeout(async () => {
    const canvas = await svgToCanvas(themeIconSvg(id, "png"), 64, 64);
    const pixels = canvas.getContext("2d")?.getImageData(0, 0, 64, 64).data;
    if (pixels) api.setThemeIcon(Array.from(pixels), 64, 64).catch(() => undefined);
    saveThemeArt(id).catch(() => undefined);
  }, 120);
}

function refreshThemeChrome(id: ThemeId): void {
  const titlebar = document.getElementById("titlebar");
  if (titlebar) titlebar.innerHTML = titlebarInner();
  const brand = document.getElementById("brand-logo");
  if (brand) brand.innerHTML = themeIconSvg(id, "brand");
  pushThemeIcon(id);
}

function renderThemeFx(id: ThemeId): void {
  let layer = document.getElementById("theme-fx");
  if (!layer) {
    layer = document.createElement("div");
    layer.id = "theme-fx";
    layer.setAttribute("aria-hidden", "true");
    document.body.prepend(layer);
  }
  layer.className = `fx-${id}`;
  layer.innerHTML = themeFxMarkup(id);
}

function applyTheme(id: ThemeId): void {
  activeTheme = id;
  document.documentElement.dataset.theme = id;
  renderThemeFx(id);
  refreshThemeChrome(id);
}

function renderThemes(): string {
  const active = currentTheme();
  const cards = THEMES.map(
    (theme) => `<button class="theme-card ${theme.id === active ? "selected" : ""}" data-theme-pick="${theme.id}">
      <span class="theme-preview" data-theme="${theme.id}" aria-hidden="true">
        <span class="tp-side"><i></i><i></i><i></i></span>
        <span class="tp-main">
          <span class="tp-title"></span>
          <span class="tp-card"><span class="tp-bars"><i style="height:45%"></i><i style="height:70%"></i><i style="height:100%"></i><i style="height:60%"></i></span></span>
          <span class="tp-row"><i></i><i></i></span>
        </span>
        <span class="tp-fx"></span>
      </span>
      <span class="theme-meta">
        <span class="theme-name"><span class="theme-icon">${themeIconSvg(theme.id, "card")}</span><strong>${esc(t(theme.key))}</strong></span>
        <span class="theme-dots" data-theme="${theme.id}"><i></i><i></i><i></i></span>
      </span>
      <span class="theme-check" aria-hidden="true"><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7"/></svg></span>
    </button>`,
  ).join("");
  return `<div class="theme-grid">${cards}</div>`;
}

function iconMacros(): string {
  return `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M4 12h10M4 17h13" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/><circle cx="17" cy="12" r="2" fill="none" stroke="currentColor" stroke-width="1.8"/></svg>`;
}

function renderShell(): void {
  app.innerHTML = `
  <header class="titlebar" id="titlebar" data-tauri-drag-region>${titlebarInner()}</header>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <div class="brand-logo" id="brand-logo">${themeIconSvg(currentTheme(), "brand")}</div>
        <div class="brand-text"><strong>AJAZZ</strong><span>AJ199</span></div>
      </div>
      <nav class="nav">
        ${navItem("dpi", "nav.dpi", iconDpi())}
        ${navItem("buttons", "nav.buttons", iconButtons())}
        ${navItem("lighting", "nav.lighting", iconLight())}
        ${navItem("macros", "nav.macros", iconMacros())}
        ${navItem("themes", "nav.themes", iconThemes())}
      </nav>
      <div class="sidebar-footer">
        <button class="btn tiny ghost" id="lang-btn">${lang() === "ru" ? "EN" : "RU"}</button>
        <span class="version">v0.1.0-dev</span>
      </div>
    </aside>
    <main class="main">
      <header class="topbar">
        <div class="title-wrap">
          <h1 id="page-title"></h1>
          <p id="page-subtitle"></p>
        </div>
        <div class="top-actions">
          <span class="save-indicator" id="save-indicator" data-phase="idle" aria-live="polite"><span class="save-mark" aria-hidden="true"><svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7"/></svg></span><span data-save-text></span></span>
          <div class="menu-wrap" id="menu-wrap">
            <button class="btn ghost menu-btn" id="menu-btn" aria-label="${esc(t("common.menu"))}">⋮</button>
            <div class="menu-panel hidden" id="menu-panel">
              <button class="menu-item" id="test-write-btn">${esc(t("common.testWrite"))}</button>
              <button class="menu-item" id="device-info-btn">${esc(t("common.deviceInfo"))}</button>
              <div class="menu-sep"></div>
              <span class="menu-label">${esc(t("alerts.title"))}</span>
              <label class="write-switch" title="${esc(t("alerts.toggle"))}">
                <input id="alerts-toggle" type="checkbox" ${batteryAlerts ? "checked" : ""}/>
                <span class="switch-track"><span class="switch-thumb"></span></span>
                <span class="switch-label">${esc(t("alerts.toggle"))}</span>
              </label>
              <button class="menu-item" id="test-toast-btn">${esc(t("alerts.test"))}</button>
              <div class="menu-sep"></div>
              <span class="menu-label">${esc(t("common.profileActions"))}</span>
              <button class="menu-item" id="backup-btn">${esc(t("common.backup"))}</button>
              <button class="menu-item" id="export-btn">${esc(t("common.export"))}</button>
              <label class="menu-item import-label">${esc(t("common.import"))}<input id="import-file" type="file" accept=".json,application/json" hidden/></label>
              <button class="menu-item danger" id="reset-btn">${esc(t("common.reset"))}</button>
            </div>
          </div>
        </div>
      </header>
      <div id="banner" class="banner hidden"></div>
      <section id="content" class="content"></section>
      <footer class="statusbar" id="statusbar"></footer>
    </main>
  </div>
  <div id="toast" class="toast hidden"></div>
  <div id="busy" class="busy hidden"><span class="spinner"></span><span>${esc(t("common.loading"))}</span></div>
  <div id="confirm" class="confirm hidden">
    <div class="confirm-box">
      <span class="eyebrow">${esc(t("common.confirmTitle"))}</span>
      <p id="confirm-text"></p>
      <div class="confirm-actions">
        <button class="btn ghost" id="confirm-cancel">${esc(t("common.cancel"))}</button>
        <button class="btn primary" id="confirm-ok">${esc(t("common.confirm"))}</button>
      </div>
    </div>
  </div>
  <div id="info" class="confirm hidden">
    <div class="confirm-box info-box">
      <span class="eyebrow">${esc(t("common.deviceInfo"))}</span>
      <div id="info-content" class="info-grid"></div>
      <div class="confirm-actions">
        <button class="btn ghost" id="info-close">${esc(t("common.close"))}</button>
      </div>
    </div>
  </div>
  <div id="macro-picker" class="confirm hidden">
    <div class="confirm-box macro-picker-box">
      <span class="eyebrow">${esc(t("macros.title"))}</span>
      <div id="macro-picker-content" class="macro-picker-content"></div>
      <div class="confirm-actions">
        <label class="btn ghost import-label macro-picker-import">${esc(t("macros.importJmm"))}<input id="macro-picker-import" type="file" accept=".jmm" hidden /></label>
        <button class="btn ghost" id="macro-picker-export">${esc(t("macros.exportJmm"))}</button>
        <span class="confirm-actions-spacer"></span>
        <button class="btn ghost" id="macro-picker-refresh">${esc(t("common.refresh"))}</button>
        <button class="btn ghost" id="macro-picker-close">${esc(t("common.close"))}</button>
      </div>
    </div>
  </div>`;

  wireShell();
  updateChrome();
}

function wireShell(): void {
  document.querySelectorAll<HTMLButtonElement>(".nav-item").forEach((button) => {
    button.addEventListener("click", () => {
      const target = button.dataset.page as Page;
      if (target === page) return;
      page = target;
      pageEnter = true;
      renderShell();
      renderPage();
    });
  });

  document
    .querySelector<HTMLButtonElement>("#lang-btn")
    ?.addEventListener("click", () => {
      toggleLang();
      renderShell();
      renderPage();
    });

  const menuPanel = document.querySelector<HTMLDivElement>("#menu-panel");
  const toggleMenu = (force?: boolean) => {
    if (!menuPanel) return;
    const next = force ?? menuPanel.classList.contains("hidden");
    menuPanel.classList.toggle("hidden", !next);
  };
  document
    .querySelector<HTMLButtonElement>("#menu-btn")
    ?.addEventListener("click", (event) => {
      event.stopPropagation();
      toggleMenu();
    });

  const showDeviceInfo = () => {
    const info = document.querySelector<HTMLDivElement>("#info");
    const body = document.querySelector<HTMLDivElement>("#info-content");
    if (!info || !body) return;
    const connected = Boolean(state?.connected);
    const battery = state?.battery ?? status?.battery ?? 0;
    const charging = state?.charging ?? status?.charging ?? false;
    const link = state?.connection === "USB" ? t("status.linkCable") : t("status.linkDongle");
    const tiles: Array<[string, string]> = [
      [t("status.connection"), connected ? link : "—"],
      [t("status.firmware"), connected ? firmwareVersion(state?.firmware ?? 0) : "—"],
      [t("status.model"), connected ? "AJAZZ AJ199" : "—"],
      [t("status.cidmid"), connected ? `${state?.cid ?? 0} / ${state?.mid ?? 0}` : "—"],
    ];
    const level = clamp(battery, 0, 100);
    body.innerHTML = `
      <div class="info-battery ${batteryTone(level)}">
        <div class="info-battery-head"><span>${esc(t("status.battery"))}</span><strong>${connected ? `${level}%` : "—"}${connected && charging ? ` · ${esc(t("status.charging"))}` : ""}</strong></div>
        <div class="info-battery-track"><span style="width:${connected ? level : 0}%"></span></div>
      </div>
      <div class="info-tiles">${tiles
        .map(
          ([label, value]) =>
            `<div class="info-tile"><span>${esc(label)}</span><code>${esc(value)}</code></div>`,
        )
        .join("")}</div>
      <div class="info-row info-backup"><span>${esc(t("common.backup"))}</span><code title="${esc(lastBackup ?? "")}">${esc(lastBackup ?? "—")}</code></div>`;
    info.classList.remove("hidden");
  };
  document
    .querySelector<HTMLButtonElement>("#device-info-btn")
    ?.addEventListener("click", showDeviceInfo);
  document
    .querySelector<HTMLButtonElement>("#info-close")
    ?.addEventListener("click", () =>
      document.querySelector<HTMLDivElement>("#info")?.classList.add("hidden"),
    );
  document
    .querySelector<HTMLDivElement>("#info")
    ?.addEventListener("click", (event) => {
      if (event.target === event.currentTarget) {
        (event.currentTarget as HTMLDivElement).classList.add("hidden");
      }
    });

  const macroPicker = document.querySelector<HTMLDivElement>("#macro-picker");
  macroPicker?.addEventListener("click", (event) => {
    if (event.target === macroPicker) closeMacroPicker();
  });
  document
    .querySelector<HTMLButtonElement>("#macro-picker-close")
    ?.addEventListener("click", closeMacroPicker);
  document
    .querySelector<HTMLInputElement>("#macro-picker-import")
    ?.addEventListener("change", (event) => {
      const input = event.target as HTMLInputElement;
      const file = input.files?.[0];
      input.value = "";
      if (!file) return;
      closeMacroPicker();
      void importMacro(file);
    });
  document
    .querySelector<HTMLButtonElement>("#macro-picker-export")
    ?.addEventListener("click", () => {
      if (!macroEvents) {
        showToast(t("macros.loadEventsHint"), "error");
        return;
      }
      void exportMacro();
    });
  document
    .querySelector<HTMLButtonElement>("#macro-picker-refresh")
    ?.addEventListener("click", () => {
      macros = null;
      void loadMacros();
    });
  document
    .querySelector<HTMLDivElement>("#macro-picker-content")
    ?.addEventListener("click", (event) => {
      const target = event.target as HTMLElement;
      const action = target.closest<HTMLElement>("[data-action]")?.dataset
        .action;
      if (action === "create-macro") void createMacro();
      if (action === "confirm-delete-macro") {
        const index = Number(
          target.closest<HTMLElement>("[data-index]")?.dataset.index,
        );
        macroLongPress = null;
        if (Number.isFinite(index)) void deleteMacroAt(index);
      }
      if (action === "cancel-delete-macro") {
        document
          .querySelectorAll(".macro-pick-row.armed")
          .forEach((row) => row.classList.remove("armed"));
        macroLongPress = null;
      }
    });

  const pickerContent = document.querySelector<HTMLDivElement>(
    "#macro-picker-content",
  );
  pickerContent?.addEventListener("keydown", (event) => {
    const input = (event.target as Element).closest<HTMLInputElement>(
      "[data-macro-rename]",
    );
    if (!input) return;
    if (event.key === "Enter") {
      event.preventDefault();
      void finishMacroRename(input, true);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      void finishMacroRename(input, false);
    }
  });
  pickerContent?.addEventListener("input", (event) => {
    const input = (event.target as Element).closest<HTMLInputElement>(
      "[data-macro-rename]",
    );
    if (input) renameDraft = input.value;
  });
  pickerContent?.addEventListener("focusout", (event) => {
    const input = (event.target as Element).closest<HTMLInputElement>(
      "[data-macro-rename]",
    );
    if (!input) return;
    // Перерисовка списка пересоздаёт поле — это не уход фокуса.
    window.setTimeout(() => {
      if (!input.isConnected) return;
      if (document.activeElement === input) return;
      void finishMacroRename(input, true);
    }, 0);
  });
  pickerContent?.addEventListener("pointerdown", (event) => {
    const target = event.target as Element;
    if (target.closest("button, input")) return;
    const row = target.closest<HTMLElement>("[data-macro-pick]");
    if (!row) return;
    if (macroLongPress) {
      window.clearTimeout(macroLongPress.timer);
    }
    document
      .querySelectorAll(".macro-pick-row.armed")
      .forEach((node) => node.classList.remove("armed"));
    const index = Number(row.dataset.macroPick);
    const timer = window.setTimeout(() => {
      if (!writeEnabled()) {
        showToast(t("common.writeDisabled"), "error");
        macroLongPress = null;
        return;
      }
      if (!macroLongPress || macroLongPress.index !== index) return;
      macroLongPress.armed = true;
      row.classList.add("armed");
    }, 650);
    macroLongPress = {
      index,
      timer,
      startX: event.clientX,
      startY: event.clientY,
      armed: false,
      row,
    };
  });
  pickerContent?.addEventListener("pointermove", (event) => {
    if (!macroLongPress || macroLongPress.armed) return;
    if (
      Math.hypot(
        event.clientX - macroLongPress.startX,
        event.clientY - macroLongPress.startY,
      ) > 8
    ) {
      window.clearTimeout(macroLongPress.timer);
      macroLongPress = null;
    }
  });
  pickerContent?.addEventListener("pointerup", (event) => {
    if (!macroLongPress) return;
    const press = macroLongPress;
    window.clearTimeout(press.timer);
    if (!press.armed) {
      const row = (event.target as Element).closest<HTMLElement>(
        "[data-macro-pick]",
      );
      macroLongPress = null;
      if (row && Number(row.dataset.macroPick) === press.index) {
        selectMacro(press.index);
      }
    }
  });
  pickerContent?.addEventListener("pointercancel", () => {
    if (!macroLongPress) return;
    window.clearTimeout(macroLongPress.timer);
    macroLongPress.row.classList.remove("armed");
    macroLongPress = null;
  });

  document
    .querySelector<HTMLInputElement>("#alerts-toggle")
    ?.addEventListener("change", (event) => {
      batteryAlerts = (event.target as HTMLInputElement).checked;
      api.setBatteryAlerts(batteryAlerts).catch(showError);
    });
  document
    .querySelector<HTMLButtonElement>("#test-toast-btn")
    ?.addEventListener("click", () => {
      const kinds = ["low", "critical", "full"];
      const kind = kinds[testToastStep % kinds.length];
      testToastStep += 1;
      api.testBatteryToast(kind).catch(showError);
    });
  document
    .querySelector<HTMLButtonElement>("#test-write-btn")
    ?.addEventListener("click", async () => {
      if (!writeEnabled()) {
        showToast(t("common.writeDisabled"), "error");
        return;
      }
      setBusy(true);
      try {
        const next = await api.testWrite();
        setState(next);
        showToast(
          t("toast.testWrite", { path: next.backup ?? "backup" }),
        );
        toggleMenu(false);
      } catch (error) {
        showError(error);
      } finally {
        setBusy(false);
      }
    });

  document
    .querySelector<HTMLButtonElement>("#backup-btn")
    ?.addEventListener("click", async () => {
      setBusy(true);
      try {
        const path = await api.backup();
        lastBackup = path;
        showToast(t("toast.backup", { path }));
      } catch (error) {
        showError(error);
      } finally {
        setBusy(false);
        updateChrome();
      }
    });

  document
    .querySelector<HTMLButtonElement>("#export-btn")
    ?.addEventListener("click", async () => {
      try {
        const json = await api.exportProfile();
        const blob = new Blob([json], { type: "application/json" });
        const url = URL.createObjectURL(blob);
        const anchor = document.createElement("a");
        anchor.href = url;
        anchor.download = "aj199_profile.json";
        anchor.click();
        URL.revokeObjectURL(url);
        showToast(t("toast.exported"));
      } catch (error) {
        showError(error);
      }
    });

  document
    .querySelector<HTMLInputElement>("#import-file")
    ?.addEventListener("change", async (event) => {
      const input = event.target as HTMLInputElement;
      const file = input.files?.[0];
      input.value = "";
      if (!file) return;
      if (!writeEnabled()) {
        showToast(t("common.writeDisabled"), "error");
        return;
      }
      if (!(await confirmAction(t("common.importConfirm")))) return;
      setBusy(true);
      try {
        const text = await file.text();
        const next = await api.restoreProfile(text);
        setState(next);
        showToast(t("toast.imported"));
        renderPage();
      } catch (error) {
        showError(error);
      } finally {
        setBusy(false);
      }
    });

  document
    .querySelector<HTMLButtonElement>("#reset-btn")
    ?.addEventListener("click", async () => {
      if (!writeEnabled()) {
        showToast(t("common.writeDisabled"), "error");
        return;
      }
      if (!(await confirmAction(t("common.resetConfirm")))) return;
      setBusy(true);
      try {
        const next = await api.resetProfile();
        setState(next);
        showToast(t("toast.reset"));
        renderPage();
      } catch (error) {
        showError(error);
      } finally {
        setBusy(false);
      }
    });

  const content = document.querySelector<HTMLElement>("#content")!;
  content.addEventListener("click", (event) => {
    const node = event.target as HTMLElement;
    const zone = (event.target as Element).closest("[data-key-zone]");
    if (zone) {
      selectedKeySlot = Number(zone.getAttribute("data-key-zone"));
      renderPage();
      return;
    }
    const pill = node.closest<HTMLButtonElement>("[data-pill]");
    if (pill) {
      const on = pill.classList.toggle("on");
      pill.textContent = on ? t("common.on") : t("common.off");
      queueParams();
      return;
    }
    const modeCard = node.closest<HTMLButtonElement>("[data-mode-card]");
    if (modeCard) {
      content
        .querySelectorAll<HTMLButtonElement>("[data-mode-card]")
        .forEach((button) => button.classList.toggle("selected", button === modeCard));
      updateLightingPreview();
      queueLighting();
      return;
    }
    const ladderColor = node.closest<HTMLButtonElement>("[data-ladder-color]");
    if (ladderColor) {
      const stage = Number(ladderColor.dataset.ladderColor);
      content
        .querySelector<HTMLInputElement>(`[data-stage-color="${stage}"]`)
        ?.click();
      return;
    }
    const themePick = node.closest<HTMLElement>("[data-theme-pick]");
    if (themePick) {
      const id = themePick.dataset.themePick as ThemeId;
      applyTheme(id);
      api.setTheme(id).catch(showError);
      content
        .querySelectorAll("[data-theme-pick]")
        .forEach((card) => card.classList.toggle("selected", card === themePick));
      return;
    }
    const dialStep = node.closest<HTMLButtonElement>("[data-dial-step]");
    if (dialStep) {
      const dial = dialStep.closest<HTMLElement>("[data-dial]");
      if (dial) stepDial(dial.dataset.dial as DialKey, Number(dialStep.dataset.dialStep));
      return;
    }
    const swatch = node.closest<HTMLButtonElement>("[data-swatch]");
    if (swatch) {
      const input = content.querySelector<HTMLInputElement>("#led-color");
      if (input && swatch.dataset.swatch) {
        setWheelColor(swatch.dataset.swatch);
        queueLighting();
      }
      return;
    }
    const target = node.closest<HTMLButtonElement>("[data-action]");
    if (!target) return;
    const action = target.dataset.action;
    if (action === "rename-key") {
      const slot = Number(target.dataset.slot);
      const key = profile()?.keys[slot];
      if (key) {
        const current = actionAliases[key.code] ?? "";
        const name = window.prompt(t("buttons.renamePrompt"), current);
        if (name !== null) {
          const trimmed = name.trim();
          if (trimmed) actionAliases[key.code] = trimmed;
          else delete actionAliases[key.code];
          persistActionAliases();
          renderPage();
        }
      }
      return;
    }
    if (action === "activate-stage") activateStage(Number(target.dataset.stage));
    if (action === "add-stage") addStage();
    if (action === "confirm-delete-stage") {
      const index = Number(target.dataset.stage);
      const column = target.closest<HTMLElement>("[data-ladder-stage]");
      if (column && writeEnabled()) {
        column.classList.remove("armed");
        column.classList.add("stage-leaving");
        window.setTimeout(() => deleteStage(index), 220);
      } else {
        deleteStage(index);
      }
    }
    if (action === "cancel-delete-stage") disarmStage();
    if (action === "add-macro-event" && macroEvents) {
      if (macroEvents.events.length >= MACRO_EVENT_LIMIT) {
        showToast(t("macros.limitFull"), "error");
      } else {
        macroEvents.events.push({ key: 0x20, state: 1, delay_ms: 50 });
        renderPage();
      }
    }
    if (action === "restore-macro-events" && macroEvents && clearedMacro) {
      macroEvents.events = clearedMacro.events;
      clearedMacro = null;
      renderPage();
    }
    if (action === "record-macro") startRecording();
    if (action === "stop-record") stopRecording();
    if (action === "open-macro-picker") openMacroPicker();
    if (action === "delete-macro-event" && macroEvents) {
      macroEvents.events.splice(Number(target.dataset.eventIndex), 1);
      renderPage();
    }
    if (action === "save-macro") void saveMacro();
    if (action === "reconnect") void loadState();
  });

  content.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    const targetEl = event.target as Element;
    const shredHandle = targetEl.closest<HTMLElement>("[data-shredder-handle]");
    if (shredHandle) {
      const root = shredHandle.closest<HTMLElement>("[data-shredder]");
      if (!root || !root.isConnected) return;
      event.preventDefault();
      shredDrag = {
        startX: event.clientX,
        distance: Math.max(1, root.clientWidth - shredHandle.offsetWidth - 8),
        progress: 0,
        root,
      };
      root.style.setProperty("--travel", `${shredDrag.distance}px`);
      return;
    }
    const resize = targetEl.closest<HTMLElement>("[data-resize-index]");
    if (resize) {
      event.preventDefault();
      event.stopPropagation();
      const index = Number(resize.dataset.resizeIndex);
      timelineResize = {
        index,
        startX: event.clientX,
        startDelay: macroEvents?.events[index]?.delay_ms ?? 0,
      };
      return;
    }
    const block = targetEl.closest<HTMLElement>("[data-macro-block]");
    if (block && !targetEl.closest("button") && macroEvents) {
      event.preventDefault();
      timelineBlockDrag = {
        index: Number(block.dataset.macroBlock),
        startX: event.clientX,
        startY: event.clientY,
        active: false,
        block,
        dropIndex: Number(block.dataset.macroBlock),
      };
      return;
    }
    const stageEl = targetEl.closest<HTMLElement>("[data-ladder-stage]");
    if (stageEl && !targetEl.closest("button, input")) {
      startStageLongPress(Number(stageEl.dataset.ladderStage), event);
    }
    const track = targetEl.closest<HTMLElement>("[data-ladder-track]");
    if (!track) return;
    const stage = Number(track.dataset.ladderTrack);
    ladderDrag = { stage, track, startY: event.clientY, moved: false };
    track.setPointerCapture?.(event.pointerId);
    event.preventDefault();
  });

  content.addEventListener(
    "wheel",
    (event) => {
      const dial = (event.target as Element).closest<HTMLElement>("[data-dial]");
      if (dial) {
        event.preventDefault();
        const horizontal = Math.abs(event.deltaX) > Math.abs(event.deltaY);
        dialWheelAccum += horizontal ? event.deltaX : -event.deltaY;
        if (Math.abs(dialWheelAccum) >= DIAL_WHEEL_STEP) {
          stepDial(dial.dataset.dial as DialKey, dialWheelAccum > 0 ? 1 : -1);
          dialWheelAccum = 0;
        }
        return;
      }
      const track = (event.target as Element).closest<HTMLElement>(
        "[data-ladder-track]",
      );
      if (!track) return;
      event.preventDefault();
      const stage = Number(track.dataset.ladderTrack);
      const current = profile()?.dpi_stages[stage]?.x ?? 800;
      const step = event.shiftKey ? 500 : 50;
      setStageValue(stage, current + (event.deltaY < 0 ? step : -step));
      bump(content.querySelector(`[data-ladder-stage="${stage}"] .ladder-value`));
      markStagePending(stage, "dpi");
    },
    { passive: false },
  );

  content.addEventListener("input", (event) => {
    const input = event.target as HTMLInputElement;
    const macroDelay = input.dataset.macroDelay;
    if (macroDelay !== undefined && macroEvents) {
      setEventDelay(Number(macroDelay), Number(input.value) || 0);
    }
  });

  content.addEventListener("change", (event) => {
    const target = event.target as HTMLInputElement | HTMLSelectElement;
    const keySlot = target.dataset.keyPanelSelect;
    if (keySlot !== undefined) {
      queueKey(Number(keySlot), target.value);
      return;
    }
    if (["led-speed", "led-brightness", "led-color"].includes(target.id)) {
      queueLighting();
      return;
    }
    const macroKey = (target as HTMLSelectElement).dataset.macroKey;
    if (macroKey !== undefined && macroEvents) {
      const item = macroEvents.events[Number(macroKey)];
      if (item) item.key = Number((target as HTMLSelectElement).value);
      return;
    }
    const stageColor = (target as HTMLInputElement).dataset.stageColor;
    if (stageColor !== undefined) {
      const stage = content.querySelector<HTMLElement>(
        `[data-ladder-stage="${stageColor}"]`,
      );
      stage?.style.setProperty("--stage-color", (target as HTMLInputElement).value);
      markStagePending(Number(stageColor), "color");
      return;
    }
    const macroState = (target as HTMLSelectElement).dataset.macroState;
    if (macroState !== undefined && macroEvents) {
      const item = macroEvents.events[Number(macroState)];
      if (item) item.state = Number((target as HTMLSelectElement).value);
      return;
    }
  });
}

function updateChrome(): void {
  const p = profile();
  const connected = Boolean(state?.connected);
  const connection = connected ? state?.connection ?? "offline" : "offline";
  const write = writeEnabled();

  const titles: Record<Page, [MessageKey, MessageKey]> = {
    dpi: ["page.dpi.title", "page.dpi.subtitle"],
    buttons: ["page.buttons.title", "page.buttons.subtitle"],
    lighting: ["page.lighting.title", "page.lighting.subtitle"],
    macros: ["page.macros.title", "page.macros.subtitle"],
    themes: ["page.themes.title", "page.themes.subtitle"],
  };
  const [titleKey, subtitleKey] = titles[page];
  const title = document.querySelector<HTMLElement>("#page-title");
  const subtitle = document.querySelector<HTMLElement>("#page-subtitle");
  if (title) title.textContent = t(titleKey);
  if (subtitle) subtitle.textContent = t(subtitleKey);

  const banner = document.querySelector<HTMLDivElement>("#banner");
  if (banner) {
    if (!connected) {
      banner.className = "banner error";
      banner.innerHTML = `<span>${esc(state?.error ?? t("common.notConnected"))}<br/><span class="hint">${esc(t("common.offlineHint"))}</span></span><button class="btn tiny ghost" data-action="reconnect" id="reconnect-btn">${esc(t("common.refresh"))}</button>`;
      document
        .querySelector<HTMLButtonElement>("#reconnect-btn")
        ?.addEventListener("click", () => void loadState());
    } else {
      banner.className = "banner hidden";
      banner.innerHTML = "";
    }
  }

  const resetButton = document.querySelector<HTMLButtonElement>("#reset-btn");
  if (resetButton) resetButton.disabled = !write || !connected;
  const testButton = document.querySelector<HTMLButtonElement>("#test-write-btn");
  if (testButton) testButton.disabled = !write || !connected;
  const importLabel = document.querySelector<HTMLLabelElement>(".import-label");
  importLabel?.classList.toggle("disabled", !write || !connected);
  const exportButton = document.querySelector<HTMLButtonElement>("#export-btn");
  if (exportButton) exportButton.disabled = !connected;
  const backupButton = document.querySelector<HTMLButtonElement>("#backup-btn");
  if (backupButton) backupButton.disabled = !connected;



  const bar = document.querySelector<HTMLElement>("#statusbar");
  if (bar) {
    const battery = state?.battery ?? status?.battery ?? 0;
    const charging = state?.charging ?? status?.charging ?? false;
    bar.innerHTML = `
      <span class="pill ${connectionClass(connection)}"><span class="dot ${connectionClass(connection)}"></span>${connected ? esc(connection) : esc(t("status.offline"))}</span>
      ${connected ? batteryGlyph(battery, charging) : `<span>${esc(t("status.battery"))}: —</span>`}
      ${lastBackup ? `<span class="backup-path" title="${esc(lastBackup)}">backup: ${esc(lastBackup.split(/[\\/]/).pop())}</span>` : ""}
      <span class="spacer"></span>
      ${connected && state?.firmware ? `<span class="status-fw">FW ${firmwareVersion(state.firmware)}</span>` : ""}
      <span class="status-model">AJ199</span>`;
  }

  if (!write || !connected) {
    const hint = connected
      ? t("common.writeDisabledHint")
      : t("common.notConnected");
    document
      .querySelectorAll<HTMLButtonElement>("#content button:disabled")
      .forEach((button) => {
        button.title = hint;
      });
    document
      .querySelector<HTMLLabelElement>(".import-label.disabled")
      ?.setAttribute("title", hint);
  }

  if (p) {
    const active = activeStage(p);
    const hero = document.querySelector<HTMLElement>(".hero-value");
    if (hero) hero.textContent = `${active.x} × ${active.y}`;
  }
}

function playPageEnter(content: HTMLElement): void {
  if (!pageEnter) return;
  pageEnter = false;
  content.classList.remove("page-enter");
  void content.offsetWidth;
  content.classList.add("page-enter");
  window.setTimeout(() => content.classList.remove("page-enter"), 1200);
}

function renderPage(): void {
  const content = document.querySelector<HTMLElement>("#content");
  if (!content) return;
  if (page === "themes") {
    content.innerHTML = renderThemes();
    playPageEnter(content);
    updateChrome();
    return;
  }
  if (!state?.connected || !state.profile) {
    content.innerHTML = offlineCard();
    playPageEnter(content);
    return;
  }
  if (page === "dpi") content.innerHTML = renderDpi(state.profile);
  if (page === "buttons") content.innerHTML = renderButtons(state.profile);
  if (page === "lighting") content.innerHTML = renderLighting(state.profile);
  if (page === "macros") {
    content.innerHTML = renderMacros();
    if (macros === null && !macrosLoading) void loadMacros();
  }
  playPageEnter(content);
  syncOutputs();
  syncMacroForm();
  updateChrome();
}

function offlineCard(): string {
  return `<div class="card empty-state">
    <div class="empty-icon">?</div>
    <h2>${esc(t("status.offline"))}</h2>
    <p>${esc(state?.error ?? t("common.notConnected"))}</p>
    <button class="btn primary" data-action="reconnect">${esc(t("common.refresh"))}</button>
  </div>`;
}

function dialValue(p: Profile, key: DialKey): number {
  if (key === "polling") return p.polling_hz ?? 1000;
  if (key === "debounce") return p.debounce;
  return Math.min(p.lod, 2);
}

function dialText(key: DialKey, value: number): string {
  if (key === "polling") return `${value} ${t("dpi.hz")}`;
  if (key === "debounce") return `${value} ${t("dpi.ms")}`;
  return String(value);
}

function renderDial(p: Profile, key: DialKey, label: string): string {
  const value = dialValue(p, key);
  const steps = DIAL_STEPS[key];
  const index = steps.indexOf(value);
  return `<div class="stat-dial ${pendingTasks.has(`dial:${key}`) ? "pending" : ""}" data-dial="${key}" title="${esc(t("dpi.dialHint"))}">
      <span>${esc(label)}</span>
      <div class="stat-dial-line">
        <button class="dial-arrow" data-dial-step="-1" ${index === 0 ? "disabled" : ""} aria-label="-">‹</button>
        <strong data-dial-value>${esc(dialText(key, value))}</strong>
        <button class="dial-arrow" data-dial-step="1" ${index === steps.length - 1 ? "disabled" : ""} aria-label="+">›</button>
      </div>
    </div>`;
}

function renderDpi(p: Profile): string {
  const active = activeStage(p);
  const activeIndex = activeStageIndex(p);
  const activeCount = Math.max(1, Math.min(DPI_VISIBLE_STAGES, p.dpi_stage_count));
  const visible = p.dpi_stages.slice(0, activeCount);
  const stageLabel = `${t("dpi.stage")} ${activeIndex + 1} ${t("dpi.of")} ${visible.length}`;
  const dis = writeEnabled() ? "" : "disabled";
  const ladder = visible
    .map((stage, index) => {
      const color = p.dpi_colors[index] ?? { r: 255, g: 255, b: 255 };
      const isActive = index === activeIndex;
      return `<div class="ladder-stage ${isActive ? "active" : ""} ${newStageIndex === index ? "stage-new" : ""}" data-ladder-stage="${index}" style="--stage-color:${toHexColor(color)}">
        <span class="ladder-value">${stage.x}</span>
        <div class="ladder-track" data-ladder-track="${index}" title="${esc(t("dpi.ladderDrag"))}">
          <div class="ladder-fill" style="height:${dpiLadderPercent(stage.x)}%"></div>
        </div>
        <button class="ladder-color" data-ladder-color="${index}" title="${esc(t("dpi.color"))}" aria-label="${esc(t("dpi.color"))} ${index + 1}"></button>
        <input class="ladder-color-input" type="color" value="${toHexColor(color)}" data-stage-color="${index}" tabindex="-1" />
        <button class="ladder-index" ${isActive ? "" : `data-action="activate-stage" data-stage="${index}" title="${esc(t("common.makeActive"))}"`} ${dis}>${index + 1}${isActive ? " · " + esc(t("dpi.active")) : ""}</button>
        <div class="ladder-delete-overlay">
          <span class="delete-cross" aria-hidden="true">×</span>
          <button class="btn danger small" data-action="confirm-delete-stage" data-stage="${index}">${esc(t("macros.delete"))}</button>
          <button class="btn ghost tiny" data-action="cancel-delete-stage">${esc(t("common.cancel"))}</button>
        </div>
      </div>`;
    })
    .join("");
  newStageIndex = null;
  const addTile =
    activeCount < DPI_VISIBLE_STAGES
      ? `<div class="ladder-stage ladder-add-stage">
        <span class="ladder-value">&nbsp;</span>
        <button class="ladder-add" data-action="add-stage" title="${esc(t("dpi.addStage"))}" aria-label="${esc(t("dpi.addStage"))}" ${dis}><span class="plus-icon" aria-hidden="true"></span></button>
        <span class="ladder-color-spacer"></span>
        <span class="ladder-index">&nbsp;</span>
      </div>`
      : "";

  return `<div class="dpi-grid">
    <div class="card hero">
      <div>
        <span class="eyebrow">${esc(t("dpi.current"))}</span>
        <div class="hero-value">${active.x} × ${active.y}</div>
        <span class="muted">${esc(stageLabel)}</span>
      </div>
      <div class="hero-stats">
        ${renderDial(p, "polling", t("dpi.polling"))}
        ${renderDial(p, "debounce", t("dpi.debounce"))}
        ${renderDial(p, "lod", t("dpi.lod"))}
      </div>
    </div>
    <div class="card stages-card">
      <div class="card-head"><h2>${esc(t("dpi.stages"))}</h2><span class="hint">${esc(t("dpi.activeCount", { count: String(activeCount) }))}</span></div>
      <div class="dpi-ladder">${ladder}${addTile}</div>
    </div>

    <div class="card">
      <div class="card-head"><h2>${esc(t("dpi.params"))}</h2></div>
      <div class="param-grid">
        <div class="field"><span>${esc(t("dpi.angle"))}</span><button class="pill-toggle ${p.param_af !== 0 ? "on" : ""}" data-pill="angle">${p.param_af !== 0 ? esc(t("common.on")) : esc(t("common.off"))}</button></div>
        <div class="field"><span>${esc(t("dpi.ripple"))}</span><button class="pill-toggle ${p.param_b1 !== 0 ? "on" : ""}" data-pill="ripple">${p.param_b1 !== 0 ? esc(t("common.on")) : esc(t("common.off"))}</button></div>
      </div>
    </div>
  </div>`;
}

const MOUSE_BODY_PATH =
  "M277 80 C232 80 203 96 196 126 C191 150 189 190 189 236 C189 262 192 280 192 300 C190 330 188 352 192 378 C200 414 238 432 277 432 C316 432 354 414 362 378 C366 352 364 330 362 300 C362 280 365 262 365 236 C365 190 363 150 358 126 C351 96 322 80 277 80 Z";
const MOUSE_SOFT_LINES_PATH = "M197 300 C200 336 203 362 214 384 M357 300 C354 336 351 362 340 384";

function renderButtons(p: Profile): string {
  const selected = Math.min(
    Math.max(selectedKeySlot, 0),
    Math.max(0, Math.min(p.keys.length, 6) - 1),
  );
  selectedKeySlot = selected;
  if (macros === null && !macrosLoading && !macrosFailed) void loadMacros();
  const selectedKey = p.keys[selected];
  const code = selectedKey?.code ?? 0;
  const knownSelected =
    KNOWN_ACTIONS.some((item) => item.code === code) || isMacroCode(code);
  // Физические кнопки по mouse_nr.png -> слот матрицы. Номер на картинке
  // совпадает с Kn в Cfg.ini, слот = matrixIndex - 1 (K4 -> 5, K5 -> 4).
  const physical: {
    number: number;
    slot: number;
    shape: string;
    dot: [number, number];
    label: [number, number];
  }[] = [
    {
      number: 1,
      slot: 0,
      shape: `<path d="M268 86 C232 88 207 102 200 128 L199 236 L268 236 Z"/>`,
      dot: [234, 150],
      label: [36, 150],
    },
    {
      number: 2,
      slot: 1,
      shape: `<path d="M286 86 C322 88 347 102 354 128 L355 236 L286 236 Z"/>`,
      dot: [320, 150],
      label: [518, 150],
    },
    {
      number: 3,
      slot: 2,
      shape: `<rect x="265" y="120" width="24" height="52" rx="10"/>`,
      dot: [277, 146],
      label: [277, 22],
    },
    {
      number: 4,
      slot: 4,
      shape: `<rect x="184" y="186" width="10" height="44" rx="5"/>`,
      dot: [189, 208],
      label: [36, 208],
    },
    {
      number: 5,
      slot: 3,
      shape: `<rect x="184" y="244" width="10" height="46" rx="5"/>`,
      dot: [189, 267],
      label: [36, 267],
    },
    {
      number: 6,
      slot: 5,
      shape: `<rect x="270" y="196" width="14" height="32" rx="7"/>`,
      dot: [277, 212],
      label: [518, 212],
    },
  ];
  const slotTitle = (slot: number): string => {
    const item = physical.find((entry) => entry.slot === slot);
    return String(item?.number ?? slot + 1);
  };
  const zones = physical
    .map((item) => {
      const isSelected = selected === item.slot;
      const [dx, dy] = item.dot;
      const [lx, ly] = item.label;
      const top = item.number === 3;
      const line = top
        ? `M${lx} ${ly + 11} L${dx} ${dy}`
        : `M${lx + (lx < 275 ? 11 : -11)} ${ly} L${dx} ${dy}`;
      const action = esc(actionLabel(p.keys[item.slot]?.code ?? 0));
      return `<g class="mouse-zone ${isSelected ? "selected" : ""}" data-key-zone="${item.slot}" role="button" tabindex="0" aria-label="${esc(t("buttons.slot"))} ${item.number}: ${action}">
        <title>${esc(t("buttons.slot"))} ${item.number} · ${action}</title>
        <g class="zone-shape">${item.shape}</g>
        <path class="zone-leader" d="${line}"/>
        <circle class="zone-dot" cx="${dx}" cy="${dy}" r="3.2"/>
        <rect class="zone-tag" x="${lx - 11}" y="${ly - 11}" width="22" height="22" rx="3"/>
        <text class="zone-num" x="${lx}" y="${ly + 4.5}" text-anchor="middle">${item.number}</text>
      </g>`;
    })
    .join("");
  const svg = `<svg class="mouse-schematic" viewBox="0 0 554 450" role="img" aria-label="${esc(t("page.buttons.title"))}">
    <path class="mouse-body" d="${MOUSE_BODY_PATH}"/>
    <path class="mouse-line" d="M199 128 L199 236 M355 128 L355 236 M199 236 L355 236 M277 82 L277 120 M277 172 L277 196 M277 228 L277 236"/>
    <path class="mouse-line soft" d="${MOUSE_SOFT_LINES_PATH}"/>
    ${zones}
  </svg>`;
  const panelRename = knownSelected
    ? ""
    : `<div class="mouse-panel-actions"><button class="btn tiny ghost" data-action="rename-key" data-slot="${selected}">${esc(t("buttons.rename"))}</button></div>`;
  const panel = `<div class="mouse-panel">
    <span class="eyebrow">${esc(t("buttons.selected"))}</span>
    <strong title="${esc(t("buttons.codeHint", { code: `0x${hex(code, 6)}` }))}">${esc(t("buttons.slot"))} ${esc(slotTitle(selected))} · ${esc(actionLabel(code))}</strong>
    <select class="key-select" data-key-panel-select="${selected}">${actionOptions(code)}</select>
    ${panelRename}
  </div>`;
  return `<div class="card mouse-card">
    <div class="card-head"><h2>${esc(t("page.buttons.title"))}</h2></div>
    <div class="mouse-layout">
      <div class="mouse-visual">${svg}</div>
      ${panel}
    </div>
  </div>`;
}

function renderLighting(p: Profile): string {
  const effect = ledEffect(p.main_led[0] ?? 0);
  const brightness = ledBrightnessLevel(p.main_led[2] ?? 0);
  const speed = ledSpeedLevel(p.main_led[4] ?? 3);
  const color = p.dpi_colors[activeStageIndex(p)] ?? { r: 255, g: 255, b: 255 };
  const colorHex = toHexColor(color);
  const effectCards = LED_EFFECTS.map(
    (item) =>
      `<button class="effect-card ${effect === item.effect ? "selected" : ""}" data-mode-card="${item.effect}">
        <span class="effect-thumb fx-${item.effect}" style="--effect-dot: ${colorHex}"></span>
        <span class="effect-name">${esc(t(item.key))}</span>
      </button>`,
  ).join("");
  const swatches = ["#ff3344", "#ff8a3d", "#ffd166", "#45e6a4", "#6ee7ff", "#4a8cff", "#8b5cf6", "#ff5cf4", "#ffffff"]
    .map((value) => `<button class="swatch" data-swatch="${value}" style="--swatch:${value}" title="${value}"></button>`)
    .join("");
  const ticks = (max: number, value: number, id: string) =>
    `<span class="level-ticks" id="${id}">${Array.from({ length: max }, (_, index) => `<span class="${index + 1 <= value ? "lit" : ""}">${index + 1}</span>`).join("")}</span>`;
  return `<div class="light-page">
    <div class="card light-hero fx-${effect}" data-lighting-preview style="--led-color: ${colorHex}">
      <div class="light-stage">
        <div class="light-halo"></div>
        <svg class="light-mouse" viewBox="150 60 254 390" aria-hidden="true">
          <path class="lm-body" d="${MOUSE_BODY_PATH}"/>
          <path class="lm-line" d="M199 128 L199 236 M355 128 L355 236 M199 236 L355 236 M277 82 L277 116 M277 176 L277 196 M277 228 L277 236"/>
          <path class="lm-line soft" d="${MOUSE_SOFT_LINES_PATH}"/>
          <rect class="lm-side" x="184" y="186" width="10" height="44" rx="5"/>
          <rect class="lm-side" x="184" y="244" width="10" height="46" rx="5"/>
          <rect class="lm-side" x="270" y="196" width="14" height="32" rx="7"/>
          <rect class="lm-wheel-glow" x="259" y="112" width="36" height="68" rx="16"/>
          <rect class="lm-wheel" x="265" y="120" width="24" height="52" rx="10"/>
        </svg>
      </div>
      <div class="light-summary">
        <span class="eyebrow">${esc(t("lighting.effect"))}</span>
        <strong class="light-mode-name" id="led-mode-caption"></strong>
        <div class="light-facts">
          <div><span id="led-param-label"></span><b id="led-param-fact"></b></div>
          <div><span>${esc(t("lighting.colorShort"))}</span><b class="light-color-fact"><i style="background:${colorHex}" id="led-color-chip"></i><span id="led-color-fact">${colorHex.toUpperCase()}</span></b></div>
        </div>
      </div>
    </div>
    <div class="card">
      <div class="card-head"><h2>${esc(t("lighting.modeCards"))}</h2></div>
      <div class="effect-grid led-effects">${effectCards}</div>
    </div>
    <div class="light-controls">
      <div class="card light-levels">
        <div class="card-head"><h2>${esc(t("lighting.levels"))}</h2></div>
        <label class="level-field" data-led-field="steady">
          <span class="level-head"><span>${esc(t("lighting.brightness"))}</span><output id="led-brightness-output">${brightness}</output></span>
          <input id="led-brightness" type="range" min="1" max="${LED_BRIGHTNESS_MAX}" step="1" value="${brightness}"/>
          ${ticks(LED_BRIGHTNESS_MAX, brightness, "led-brightness-ticks")}
        </label>
        <label class="level-field" data-led-field="breathing">
          <span class="level-head"><span>${esc(t("lighting.speed"))}</span><output id="led-speed-output">${speed}</output></span>
          <input id="led-speed" type="range" min="1" max="${LED_SPEED_MAX}" step="1" value="${speed}"/>
          ${ticks(LED_SPEED_MAX, speed, "led-speed-ticks")}
        </label>
        <p class="level-empty" data-led-field="off">${esc(t("lighting.offNoSettings"))}</p>
      </div>
      <div class="card light-color-card">
        <div class="card-head"><h2>${esc(t("lighting.color"))}</h2></div>
        <div class="color-wheel-line">
          <div class="color-wheel" id="led-color-wheel"><span class="color-wheel-cursor" id="led-color-cursor"></span></div>
          <div class="color-wheel-side">
            <label class="color-current">
              <input id="led-color" type="color" class="native-color" value="${colorHex}" />
              <code id="led-color-value">${colorHex.toUpperCase()}</code>
            </label>
            <div class="swatch-row">${swatches}</div>
          </div>
        </div>
      </div>
    </div>
  </div>`;
}

function renderMacros(): string {
  const selected = macros?.find((macro) => macro.index === macroEditIndex);
  const loadedName =
    macroEventsIndex === macroEditIndex ? macroEvents?.name : undefined;
  const displayName =
    (loadedName ?? selected?.name ?? "").trim() || t("macros.none");
  const saveDis = writeEnabled() && macroEvents ? "" : "disabled";
  return `<div class="card">
    <div class="card-head"><h2>${esc(t("macros.edit"))}</h2></div>
    <div class="macro-picker-row">
      <button class="macro-picker" data-action="open-macro-picker">
        <span class="macro-picker-id">#${macroEditIndex}</span>
        <span class="macro-picker-copy"><strong>${esc(displayName)}</strong><span>${esc(t("macros.change"))}</span></span>
        <span class="macro-picker-chevron">›</span>
      </button>
    </div>
    <div class="macro-events">
      <div class="card-head">
        <h3>${esc(t("macros.events"))}</h3>
        <div class="card-actions">
          <button class="btn ghost small" data-action="add-macro-event" ${macroEvents && macroEvents.events.length < MACRO_EVENT_LIMIT ? "" : "disabled"}>${esc(t("macros.addEvent"))}</button>
        </div>
      </div>
      ${renderMacroEvents()}
      <div class="macro-save-row">
        <span></span>
        <button class="btn primary" data-action="save-macro" ${saveDis}>${esc(t("macros.saveMacro"))}</button>
      </div>
    </div>
  </div>`;
}

function renderMacroPickerList(): void {
  const content = document.querySelector<HTMLElement>("#macro-picker-content");
  if (!content) return;
  if (macrosFailed) {
    content.innerHTML = `<div class="empty-state slim"><p>${esc(t("macros.loadFailed"))}</p></div>`;
    return;
  }
  if (macros === null) {
    content.innerHTML = `<div class="empty-state slim"><p><span class="spinner dark tiny-spinner"></span> ${esc(t("macros.loadingList"))}</p></div>`;
    return;
  }
  const entries = [...macros];
  if (!entries.some((entry) => entry.index === macroEditIndex)) {
    const keys = state?.profile?.keys ?? [];
    const slot = keys.findIndex((key) => isMacroOn(key.code, macroEditIndex));
    if (slot >= 0) {
      entries.push({
        index: macroEditIndex,
        name: macroEvents?.name ?? null,
        assigned_slot: slot,
      });
    }
  }
  entries.sort((left, right) => left.index - right.index);
  const rows = entries
    .map(
      (macro) => `<div class="macro-pick-row" data-macro-pick="${macro.index}" role="row">
        <div class="macro-pick-content" role="presentation">
          <span class="macro-pick-id" role="cell">#${macro.index}</span>
          <span class="macro-pick-main" role="cell">${
            renamingMacro === macro.index
              ? `<input class="macro-rename-input" data-macro-rename="${macro.index}" maxlength="15" value="${esc(renameDraft ?? macro.name ?? "")}" placeholder="${esc(macro.name ?? "")}" aria-label="${esc(t("macros.name"))}" />`
              : `<strong>${esc(macro.name ?? t("macros.none"))}</strong>`
          }</span>
          <span class="macro-pick-slot" role="cell">${macro.assigned_slot === null ? esc(t("macros.notAssigned")) : `${esc(t("buttons.slot"))} ${macro.assigned_slot + 1}`}</span>
          <span class="macro-pick-select" role="cell" aria-hidden="true">›</span>
        </div>
        <div class="macro-pick-delete-overlay">
          <span class="delete-cross" aria-hidden="true">×</span>
          <button class="btn danger small" data-action="confirm-delete-macro" data-index="${macro.index}">${esc(t("macros.delete"))}</button>
          <button class="btn ghost tiny" data-action="cancel-delete-macro">${esc(t("common.cancel"))}</button>
        </div>
      </div>`,
    )
    .join("");
  content.innerHTML = `<div class="macro-table" role="table">
    <div class="macro-table-head" role="row">
      <span role="columnheader">${esc(t("macros.index"))}</span>
      <span role="columnheader">${esc(t("macros.name"))}</span>
      <span role="columnheader">${esc(t("macros.assigned"))}</span>
      <span aria-hidden="true"></span>
    </div>
    <div class="macro-table-body" role="rowgroup">${rows}</div>
  </div>
  <button class="macro-pick-add" data-action="create-macro" aria-label="${esc(t("macros.create"))}" title="${esc(t("macros.create"))}"><span class="macro-pick-plus">+</span></button>`;
  const rename = content.querySelector<HTMLInputElement>("[data-macro-rename]");
  if (rename) {
    rename.focus();
    if (renameDraft === null) rename.select();
    else rename.setSelectionRange(rename.value.length, rename.value.length);
    rename.scrollIntoView({ block: "nearest" });
  }
}

function openMacroPicker(): void {
  const picker = document.querySelector<HTMLDivElement>("#macro-picker");
  if (!picker) return;
  if (macros === null && !macrosLoading) void loadMacros();
  renderMacroPickerList();
  picker.classList.remove("hidden");
}

function closeMacroPicker(): void {
  document.querySelector<HTMLDivElement>("#macro-picker")?.classList.add("hidden");
  document
    .querySelectorAll(".macro-pick-row.armed")
    .forEach((row) => row.classList.remove("armed"));
  macroLongPress = null;
}

function selectMacro(index: number): void {
  macroEditIndex = index;
  macroEvents = null;
  macroEventsIndex = -1;
  closeMacroPicker();
  void loadMacroEvents(index);
  renderPage();
}

async function deleteMacroAt(index: number): Promise<void> {
  if (!writeEnabled()) {
    showToast(t("common.writeDisabled"), "error");
    return;
  }
  setBusy(true);
  try {
    const next = await api.deleteMacro(index);
    setState(next);
    macroEvents = null;
    macroEventsIndex = -1;
    macros = null;
    const list = await loadMacros();
    macroEditIndex = list[0]?.index ?? 0;
    await loadMacroEvents(macroEditIndex);
    showToast(t("macros.deleted"));
    renderMacroPickerList();
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
    renderPage();
  }
}

function nextMacroName(list: MacroDto[]): string {
  const base = t("macros.defaultNameBase");
  const pattern = new RegExp(
    `^${base.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?:\\s+(\\d+))?$`,
  );
  let max = 0;
  for (const macro of list) {
    const match = (macro.name ?? "").trim().match(pattern);
    if (!match) continue;
    const number = match[1] ? Number(match[1]) : 1;
    if (Number.isFinite(number)) max = Math.max(max, number);
  }
  return `${base} ${max + 1}`;
}

async function finishMacroRename(input: HTMLInputElement, save: boolean): Promise<void> {
  const index = Number(input.dataset.macroRename);
  if (renamingMacro !== index) return;
  renamingMacro = null;
  renameDraft = null;
  const current = macros?.find((macro) => macro.index === index)?.name ?? "";
  const name = input.value.trim();
  if (save && name && name !== current) {
    setBusy(true);
    try {
      setState(await api.setMacroName(index, name));
      const entry = macros?.find((macro) => macro.index === index);
      if (entry) entry.name = name;
      if (macroEvents && macroEventsIndex === index) macroEvents.name = name;
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  }
  closeMacroPicker();
  renderPage();
}

async function createMacro(): Promise<void> {
  if (!writeEnabled()) {
    showToast(t("common.writeDisabled"), "error");
    return;
  }
  setBusy(true);
  try {
    if (macros === null) await loadMacros();
    const name = nextMacroName(macros ?? []);
    const slot = await api.findEmptyMacroSlot();
    if (slot === null) {
      showToast(t("macros.noFreeSlot"), "error");
      return;
    }
    const next = await api.writeMacro(slot, name, []);
    setState(next);
    macros = null;
    await loadMacros();
    macroEditIndex = slot;
    macroEvents = null;
    macroEventsIndex = -1;
    await loadMacroEvents(slot);
    showToast(t("macros.created"));
    renderPage();
    renamingMacro = slot;
    renameDraft = null;
    renderMacroPickerList();
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
  }
}

function isRecordableMacroKey(key: number): boolean {
  return MACRO_KEYS.some((item) => item.code === key);
}

function macroBlockWidth(delayMs: number): number {
  return Math.max(64, Math.min(280, Math.round(64 + delayMs * 0.15)));
}

function clampMacroDelays(events: SoftwareMacroEvent[]): void {
  for (const event of events) {
    event.delay_ms = clamp(Math.round(event.delay_ms), 0, MACRO_DELAY_LIMIT);
  }
}

function setEventDelay(index: number, value: number): void {
  if (!macroEvents) return;
  const event = macroEvents.events[index];
  if (!event) return;
  const next = clamp(Math.round(value), 0, MACRO_DELAY_LIMIT);
  event.delay_ms = next;
  const block = document.querySelector<HTMLElement>(
    `[data-macro-block="${index}"]`,
  );
  if (block) {
    block.style.width = `${macroBlockWidth(next)}px`;
    const label = block.querySelector<HTMLElement>(".macro-block-delay");
    if (label) label.textContent = `${next} ${t("dpi.ms")}`;
  }
  const input = document.querySelector<HTMLInputElement>(
    `[data-macro-delay="${index}"]`,
  );
  if (input && document.activeElement !== input) {
    input.value = String(next);
  }
}

function startRecording(): void {
  if (!macroEvents) {
    showToast(t("macros.noMacro"), "error");
    return;
  }
  if (recording || recordCountdown > 0) return;
  if (recordTimer !== undefined) {
    window.clearInterval(recordTimer);
    recordTimer = undefined;
  }
  recordedDown.clear();
  clearedMacro = null;
  recordCountdown = 3;
  renderPage();
  recordTimer = window.setInterval(() => {
    recordCountdown -= 1;
    if (recordCountdown <= 0) {
      if (recordTimer !== undefined) {
        window.clearInterval(recordTimer);
        recordTimer = undefined;
      }
      recordCountdown = 0;
      recording = true;
      lastRecordedAt = performance.now();
    }
    renderPage();
  }, 1000);
}

function stopRecording(): void {
  if (recordTimer !== undefined) {
    window.clearInterval(recordTimer);
    recordTimer = undefined;
  }
  const wasRecording = recording;
  // Клавиша без «отпускания» осталась бы зажатой при воспроизведении.
  for (const key of [...recordedDown]) pushRecordedEvent(key, 0);
  recordCountdown = 0;
  recording = false;
  recordedDown.clear();
  if (wasRecording) showToast(t("macros.recordStopped"));
  renderPage();
}

function pushRecordedEvent(key: number, state: number): void {
  if (!recording || !macroEvents || !isRecordableMacroKey(key)) return;
  // Место держим и под «отпускание» всех зажатых клавиш.
  if (state === 1 && macroEvents.events.length + recordedDown.size >= MACRO_EVENT_LIMIT) {
    recordedDown.delete(key);
    showToast(t("macros.limitFull"), "error");
    stopRecording();
    return;
  }
  const now = performance.now();
  const delay = clamp(Math.round(now - lastRecordedAt), 0, MACRO_DELAY_LIMIT);
  lastRecordedAt = now;
  // Мышь выдерживает задержку после события, поэтому пауза перед клавишей
  // принадлежит предыдущему событию (иначе она становится удержанием).
  const previous = macroEvents.events[macroEvents.events.length - 1];
  if (previous) previous.delay_ms = delay;
  macroEvents.events.push({ key, state, delay_ms: 0 });
  renderPage();
}

function renderMacroTimeline(): string {
  if (!macroEvents) return "";
  const count = macroEvents.events.length;
  const atLimit = count >= MACRO_EVENT_LIMIT;
  const blocked = count / MACRO_EVENT_LIMIT;
  const recordLabel =
    recordCountdown > 0
      ? t("macros.countdown", { n: recordCountdown })
      : recording
        ? t("macros.recording")
        : t("macros.record");
  const blocks = macroEvents.events
    .map((event, index) => {
      const stateLabel =
        event.state === 2
          ? t("macros.wheel")
          : event.state & 1
            ? t("macros.down")
            : t("macros.up");
      return `<div class="macro-block state-${event.state === 2 ? "wheel" : event.state & 1 ? "down" : "up"}" data-macro-block="${index}" style="width:${macroBlockWidth(event.delay_ms)}px">
        <div class="macro-block-head"><strong>${esc(macroKeyLabel(event.key))}</strong><span class="macro-block-delay">${event.delay_ms} ${esc(t("dpi.ms"))}</span></div>
        <span class="macro-block-state">${esc(stateLabel)}</span>
        <button class="macro-block-delete" data-action="delete-macro-event" data-event-index="${index}" title="${esc(t("macros.deleteEvent"))}">×</button>
        <span class="macro-block-resize" data-resize-index="${index}" title="${esc(t("macros.resizeHint"))}"></span>
      </div>`;
    })
    .join("");
  return `<div class="macro-timeline">
    <div class="timeline-toolbar">
      <div class="timeline-record">
        <button class="btn ${recording || recordCountdown > 0 ? "danger" : "primary"} small" data-action="${recording || recordCountdown > 0 ? "stop-record" : "record-macro"}" ${macroEvents ? "" : "disabled"}>${esc(recordLabel)}</button>
        ${recording || recordCountdown > 0 ? `<span class="record-hint">${esc(t("macros.recordHint"))}</span>` : ""}
      </div>
      <div class="macro-limit ${atLimit ? "full" : ""}">
        <div class="macro-limit-bar"><span style="width:${Math.min(100, blocked * 100)}%"></span></div>
        <span>${count}/${MACRO_EVENT_LIMIT}</span>
      </div>
    </div>
    <div class="timeline-track">${blocks}</div>
    ${count > 0 ? renderShredder() : ""}
  </div>`;
}

function renderShredder(): string {
  const label = currentTheme() === "phosphor" ? "rm -rf events/*" : t("macros.clearAll");
  return `<div class="shredder" data-shredder>
    <span class="shredder-trail"></span>
    <span class="shredder-label">${esc(label)}</span>
    <button class="shredder-handle" data-shredder-handle aria-label="${esc(t("macros.clearAll"))}" title="${esc(t("macros.clearAll"))}"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12M10.5 11v5M13.5 11v5"/></svg></button>
  </div>`;
}

function setShredProgress(drag: NonNullable<typeof shredDrag>, progress: number): void {
  drag.progress = progress;
  drag.root.style.setProperty("--p", progress.toFixed(3));
  drag.root.classList.toggle("armed", progress >= 0.97);
  const track = document.querySelector<HTMLElement>(".timeline-track");
  const blocks = track?.querySelectorAll<HTMLElement>("[data-macro-block]");
  if (!track || !blocks?.length) return;
  const cut = Math.ceil(progress * blocks.length);
  blocks.forEach((block, index) => block.classList.toggle("shredded", index < cut));
  const edge = blocks[Math.min(cut, blocks.length - 1)];
  track.scrollLeft = Math.max(0, edge.offsetLeft - track.offsetLeft - track.clientWidth / 3);
}

function finishShred(): void {
  const drag = shredDrag;
  shredDrag = null;
  if (!drag) return;
  if (drag.progress < 0.97 || !macroEvents) {
    drag.root.classList.add("returning");
    setShredProgress(drag, 0);
    window.setTimeout(() => drag.root.classList.remove("returning"), 320);
    return;
  }
  drag.root.classList.add("done");
  const index = macroEditIndex;
  const events = macroEvents.events;
  window.setTimeout(() => {
    if (macroEvents?.events !== events) return;
    clearedMacro = { index, events };
    macroEvents.events = [];
    renderPage();
  }, 280);
}

function renderMacroEvents(): string {
  if (macroEventsLoading) {
    return `<div class="empty-state slim"><p><span class="spinner dark tiny-spinner"></span> ${esc(t("macros.loadingEvents"))}</p></div>`;
  }
  if (macrosFailed) {
    return `<div class="empty-state slim"><p>${esc(t("macros.loadFailed"))}</p></div>`;
  }
  if (!macroEvents || macroEventsIndex !== macroEditIndex) {
    return `<div class="empty-state slim"><p>${esc(t("macros.loadEventsHint"))}</p></div>`;
  }
  if (macroEvents.events.length === 0) {
    return `${renderMacroTimeline()}<div class="macro-empty">
      <p>${esc(t("macros.noEvents"))}</p>
      ${clearedMacro?.index === macroEditIndex ? `<button class="btn ghost small macro-restore" data-action="restore-macro-events">${esc(t("macros.restoreEvents"))}</button>` : ""}
    </div>`;
  }
  const rows = macroEvents.events
    .map((event, index) => {
      const stateOptions = [
        { value: 1, label: t("macros.down") },
        { value: 0, label: t("macros.up") },
        ...(event.state === 2 ? [{ value: 2, label: t("macros.wheel") }] : []),
      ]
        .map(
          (item) =>
            `<option value="${item.value}" ${event.state === item.value ? "selected" : ""}>${esc(item.label)}</option>`,
        )
        .join("");
      return `<tr>
        <td><select class="event-key-select" data-macro-key="${index}">${macroKeyOptions(event.key)}</select></td>
        <td><select data-macro-state="${index}">${stateOptions}</select></td>
        <td><input type="number" min="0" max="${MACRO_DELAY_LIMIT}" data-macro-delay="${index}" value="${Math.min(MACRO_DELAY_LIMIT, event.delay_ms)}" /></td>
        <td><button class="btn tiny ghost danger" data-action="delete-macro-event" data-event-index="${index}" title="${esc(t("macros.deleteEvent"))}">×</button></td>
      </tr>`;
    })
    .join("");
  return `${renderMacroTimeline()}<details class="macro-event-details">
    <summary>
      <span>${esc(t("macros.allActions"))}</span>
      <span class="macro-event-count">${macroEvents.events.length}</span>
      <span class="macro-event-arrow" aria-hidden="true">▾</span>
    </summary>
    <div class="macro-event-details-body">
      <table class="table event-table"><thead><tr><th>${esc(t("macros.key"))}</th><th>${esc(t("macros.state"))}</th><th>${esc(t("macros.delay"))}</th><th></th></tr></thead><tbody>${rows}</tbody></table>
    </div>
  </details>`;
}

function syncMacroForm(): void {
  if (
    !macrosLoading &&
    macros !== null &&
    !macrosFailed &&
    macroEventsIndex !== macroEditIndex &&
    !macroEventsLoading
  ) {
    void loadMacroEvents(macroEditIndex);
  }
}

async function loadMacros(): Promise<MacroDto[]> {
  if (macrosLoading) return macros ?? [];
  macrosLoading = true;
  macrosFailed = false;
  if (page === "macros") renderPage();
  try {
    const list = await api.macros();
    const keys = state?.profile?.keys ?? [];
    macros = list.map((macro) => {
      const slot = keys.findIndex((key) => isMacroOn(key.code, macro.index));
      return { ...macro, assigned_slot: slot >= 0 ? slot : null };
    });
    if (macros.length && !macros.some((macro) => macro.index === macroEditIndex)) {
      macroEditIndex = macros[0].index;
    }
  } catch (error) {
    macros = [];
    macrosFailed = true;
    showError(error);
  } finally {
    macrosLoading = false;
    if (page === "macros" || page === "buttons") renderPage();
    if (!document.querySelector("#macro-picker")?.classList.contains("hidden")) {
      renderMacroPickerList();
    }
  }
  return macros ?? [];
}

async function loadMacroEvents(index: number): Promise<void> {
  if (macroEventsLoading && macroEventsIndex === index) return;
  macroEventsLoading = true;
  macroEventsIndex = index;
  macroEvents = null;
  if (page === "macros") renderPage();
  try {
    const data = await api.readMacro(index);
    if (macroEventsIndex === index) {
      clampMacroDelays(data.events);
      macroEvents = data;
    }
  } catch (error) {
    if (macroEventsIndex === index) {
      macroEvents = { name: "", events: [] };
      showError(error);
    }
  } finally {
    if (macroEventsIndex === index) {
      macroEventsLoading = false;
    }
    if (page === "macros") renderPage();
  }
}



function downloadBytes(bytes: number[], filename: string): void {
  const blob = new Blob([new Uint8Array(bytes)], {
    type: "application/octet-stream",
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.click();
  URL.revokeObjectURL(url);
}

async function saveMacro(): Promise<void> {
  if (!writeEnabled()) {
    showToast(t("common.writeDisabled"), "error");
    return;
  }
  if (!macroEvents) {
    showToast(t("macros.loadEventsHint"), "error");
    return;
  }
  const index = macroEditIndex;
  const name = macroEvents.name.trim();
  setBusy(true);
  try {
    const next = await api.writeMacro(index, name, macroEvents.events);
    setState(next);
    macroEvents.name = name;
    macros = null;
    clearedMacro = null;
    showToast(t("macros.savedToast"));
    await loadMacros();
    await loadMacroEvents(index);
  } catch (error) {
    showError(error);
  } finally {
    setBusy(false);
    renderPage();
  }
}

async function exportMacro(): Promise<void> {
  if (!macroEvents) return;
  try {
    const name = macroEvents.name.trim();
    const bytes = await api.encodeJmm(name, macroEvents.events);
    const safeName = (name || "macro").replace(
      /[^\w.-]+/g,
      "_",
    );
    downloadBytes(bytes, `${safeName}.jmm`);
    showToast(t("macros.exportedToast"));
  } catch (error) {
    showError(error);
  }
}

async function importMacro(file: File): Promise<void> {
  try {
    const buffer = await file.arrayBuffer();
    const data = await api.parseJmm(Array.from(new Uint8Array(buffer)));
    clampMacroDelays(data.events);
    macroEvents = data;
    macroEventsIndex = macroEditIndex;
    showToast(t("macros.importedToast"));
    renderPage();
  } catch (error) {
    showError(error);
  }
}

function queueDeviceWrite(task: () => Promise<void>): Promise<void> {
  deviceWrites = deviceWrites.then(task, task);
  return deviceWrites;
}

function hasPendingWrites(): boolean {
  return pendingTasks.size > 0;
}

function reapplyPending(): void {
  const p = state?.profile;
  if (!p) return;
  for (const task of inflightTasks) task.apply(p);
  for (const task of pendingTasks.values()) task.apply(p);
}

function queueTask(key: string, task: PendingTask): void {
  const p = profile();
  if (!p) return;
  // Повторное изменение того же ключа сохраняет его место в очереди.
  pendingTasks.set(key, task);
  task.apply(p);
  schedulePendingWrite();
}

function schedulePendingWrite(): void {
  window.clearTimeout(pendingTimer);
  pendingTimer = window.setTimeout(() => void flushPendingWrites(), WRITE_COALESCE_MS);
}

function stopPendingTimers(): void {
  window.clearTimeout(pendingTimer);
  pendingTimer = undefined;
}

// Тихий индикатор записи в шапке: точка во время записи, галочка после.
function setSaveIndicator(phase: "saving" | "saved" | "idle"): void {
  const node = document.querySelector<HTMLElement>("#save-indicator");
  if (!node) return;
  window.clearTimeout(saveIndicatorTimer);
  node.dataset.phase = phase;
  node.querySelector<HTMLElement>("[data-save-text]")!.textContent =
    phase === "saving" ? t("save.saving") : phase === "saved" ? t("save.saved") : "";
  if (phase === "saved") {
    saveIndicatorTimer = window.setTimeout(() => setSaveIndicator("idle"), 1800);
  }
}

function canRenderNow(): boolean {
  return !ladderDrag && !stageLongPress?.armed && !timelineResize && !timelineBlockDrag && !shredDrag;
}

async function flushPendingWrites(): Promise<void> {
  if (!hasPendingWrites()) return;
  stopPendingTimers();
  if (!writeEnabled()) {
    cancelPendingWrites();
    showToast(t("common.writeDisabled"), "error");
    return;
  }
  const tasks = [...pendingTasks.values()];
  pendingTasks.clear();
  setSaveIndicator("saving");
  let failed = false;
  await queueDeviceWrite(async () => {
    inflightTasks = tasks;
    while (inflightTasks.length) {
      const task = inflightTasks.shift()!;
      try {
        await task.run();
      } catch (error) {
        failed = true;
        showError(error);
      }
    }
  });
  if (failed) {
    setSaveIndicator("idle");
    await loadState(false);
  } else if (!hasPendingWrites()) {
    setSaveIndicator("saved");
    document
      .querySelectorAll(".stat-dial.pending")
      .forEach((dial) => dial.classList.remove("pending"));
  }
}

function cancelPendingWrites(): void {
  stopPendingTimers();
  if (deviceProfile && state) state.profile = structuredClone(deviceProfile);
  pendingTasks.clear();
  reapplyPending();
  renderPage();
}

function markStagePending(stage: number, kind: "dpi" | "color"): void {
  const p = profile();
  const item = p?.dpi_stages[stage];
  if (!p || !item) return;
  const key = `stage:${stage}:${structSeq}`;
  const previous = (pendingTasks.get(key)?.meta ?? {}) as { dpi?: boolean; color?: boolean };
  const dpi = kind === "dpi" || Boolean(previous.dpi);
  const colorTouched = kind === "color" || Boolean(previous.color);
  const x = clampDpi(item.x);
  const y = clampDpi(item.y);
  const colorValue =
    document.querySelector<HTMLInputElement>(`[data-stage-color="${stage}"]`)?.value ??
    toHexColor(p.dpi_colors[stage] ?? { r: 255, g: 255, b: 255 });
  const [r, g, b] = parseColor(colorValue);
  queueTask(key, {
    meta: { dpi, color: colorTouched },
    apply: (target) => {
      const value = target.dpi_stages[stage];
      if (dpi && value) {
        value.x = x;
        value.y = y;
      }
      if (colorTouched) target.dpi_colors[stage] = { r, g, b, checksum_ok: true };
    },
    run: async () => {
      if (dpi) setState(await api.setDpi(stage, x, y));
      if (colorTouched) setState(await api.setDpiColor(stage, r, g, b));
    },
  });
}

function queueKey(slot: number, value: string): void {
  const p = profile();
  const key = p?.keys[slot];
  if (!p || !key) return;
  const code = value.startsWith("a:")
    ? KNOWN_ACTIONS.find((item) => item.action === value.slice(2))?.code
    : Number(value.slice(2));
  if (code === undefined || !Number.isFinite(code)) return;
  const leftCount = p.keys.filter((item) => item.code === 0x0101).length;
  if (key.code === 0x0101 && code !== 0x0101 && leftCount <= 1) {
    showToast(t("buttons.hint"), "error");
    renderPage();
    return;
  }
  queueTask(`key:${slot}`, {
    apply: (target) => {
      const item = target.keys[slot];
      if (item) item.code = code;
    },
    run: async () => {
      setState(
        value.startsWith("a:")
          ? await api.setKey(slot, value.slice(2))
          : await api.setKey(slot, undefined, code),
      );
    },
  });
  renderPage();
}

function queueParams(): void {
  const p = profile();
  if (!p) return;
  const content = document.querySelector<HTMLElement>("#content")!;
  const angle =
    content.querySelector('[data-pill="angle"]')?.classList.contains("on") ?? false;
  const ripple =
    content.querySelector('[data-pill="ripple"]')?.classList.contains("on") ?? false;
  const base = (pendingTasks.get("params")?.meta as { af: number; b1: number } | undefined) ?? {
    af: p.param_af,
    b1: p.param_b1,
  };
  queueTask("params", {
    meta: base,
    apply: (target) => {
      target.param_af = angle ? 1 : 0;
      target.param_b1 = ripple ? 1 : 0;
    },
    run: async () => {
      if ((base.af !== 0) !== angle) setState(await api.setAngleSnap(angle));
      if ((base.b1 !== 0) !== ripple) setState(await api.setRipple(ripple));
    },
  });
}

function queueLighting(): void {
  const p = profile();
  if (!p) return;
  const content = document.querySelector<HTMLElement>("#content")!;
  const effect = selectedLedEffect();
  const mode = ledModeOf(effect);
  const brightness = Number(
    content.querySelector<HTMLInputElement>("#led-brightness")?.value ?? LED_BRIGHTNESS_MAX,
  );
  const speed = Number(content.querySelector<HTMLInputElement>("#led-speed")?.value ?? 3);
  const a =
    effect === "steady" ? ledBrightnessByte(brightness) : effect === "breathing" ? 0xff : (p.main_led[2] ?? 0x80);
  const b = effect === "steady" ? 3 : effect === "breathing" ? speed : (p.main_led[4] ?? 3);
  const [r, g, bl] = parseColor(
    content.querySelector<HTMLInputElement>("#led-color")?.value ?? "#ffffff",
  );
  const stage = activeStageIndex(p);
  const base =
    (pendingTasks.get("lighting")?.meta as DpiColor | undefined) ??
    { ...(p.dpi_colors[stage] ?? { r: 255, g: 255, b: 255, checksum_ok: true }) };
  queueTask("lighting", {
    meta: base,
    apply: (target) => {
      target.main_led[0] = mode;
      target.main_led[2] = a;
      target.main_led[4] = b;
      target.dpi_colors[stage] = { r, g, b: bl, checksum_ok: true };
    },
    run: async () => {
      setState(await api.setLed(mode, a, b));
      if (base.r !== r || base.g !== g || base.b !== bl) {
        setState(await api.setDpiColor(stage, r, g, bl));
      }
    },
  });
}

function stepDial(key: DialKey, direction: number): void {
  const p = profile();
  if (!p) return;
  const steps = DIAL_STEPS[key];
  const current = dialValue(p, key);
  const position = steps.indexOf(current);
  const nextIndex =
    position < 0
      ? steps.findIndex((value) => value >= current)
      : position + direction;
  const next = steps[clamp(nextIndex, 0, steps.length - 1)];
  if (next === undefined || next === current) return;
  queueTask(`dial:${key}`, {
    apply: (target) => {
      if (key === "polling") target.polling_hz = next;
      else if (key === "debounce") target.debounce = next;
      else target.lod = next;
    },
    run: async () => {
      if (key === "polling") setState(await api.setPolling(next));
      else if (key === "debounce") setState(await api.setDebounce(next));
      else setState(await api.setLod(next));
    },
  });
  const dial = document.querySelector<HTMLElement>(`[data-dial="${key}"]`);
  if (dial) {
    dial.classList.add("pending");
    const valueNode = dial.querySelector<HTMLElement>("[data-dial-value]");
    if (valueNode) valueNode.textContent = dialText(key, next);
    bump(valueNode);
    const index = steps.indexOf(next);
    dial.querySelectorAll<HTMLButtonElement>("[data-dial-step]").forEach((button) => {
      const step = Number(button.dataset.dialStep);
      button.disabled = step < 0 ? index === 0 : index === steps.length - 1;
    });
  }
}

function startStageLongPress(index: number, event: PointerEvent): void {
  disarmStage();
  const timer = window.setTimeout(() => {
    if (!stageLongPress || stageLongPress.index !== index) return;
    const p = profile();
    if (!writeEnabled()) {
      stageLongPress = null;
      showToast(t("common.writeDisabled"), "error");
      return;
    }
    if (!p || Math.min(DPI_VISIBLE_STAGES, p.dpi_stage_count) <= 1) {
      stageLongPress = null;
      showToast(t("dpi.lastStage"), "error");
      return;
    }
    stageLongPress.armed = true;
    ladderDrag = null;
    document
      .querySelector(`[data-ladder-stage="${index}"]`)
      ?.classList.add("armed");
  }, STAGE_LONG_PRESS_MS);
  stageLongPress = {
    index,
    timer,
    startX: event.clientX,
    startY: event.clientY,
    armed: false,
  };
}

function cancelStageLongPress(): void {
  if (!stageLongPress) return;
  window.clearTimeout(stageLongPress.timer);
  stageLongPress = null;
}

function disarmStage(): void {
  cancelStageLongPress();
  document
    .querySelectorAll(".ladder-stage.armed")
    .forEach((node) => node.classList.remove("armed"));
}

function stageCount(p: Profile): number {
  return Math.max(1, Math.min(DPI_VISIBLE_STAGES, p.dpi_stage_count));
}

function addStage(): void {
  const p = profile();
  if (!p) return;
  if (!writeEnabled()) {
    showToast(t("common.writeDisabled"), "error");
    return;
  }
  if (stageCount(p) >= DPI_VISIBLE_STAGES) return;
  newStageIndex = stageCount(p);
  structSeq += 1;
  queueTask(`struct:${structSeq}`, {
    apply: (target) => {
      target.dpi_stage_count = Math.min(DPI_VISIBLE_STAGES, stageCount(target) + 1);
    },
    run: async () => {
      setState(await api.addDpiStage());
    },
  });
  renderPage();
}

function deleteStage(index: number): void {
  disarmStage();
  const p = profile();
  if (!p || !Number.isFinite(index)) return;
  if (!writeEnabled()) {
    showToast(t("common.writeDisabled"), "error");
    return;
  }
  if (stageCount(p) <= 1 || index >= stageCount(p)) return;
  structSeq += 1;
  queueTask(`struct:${structSeq}`, {
    apply: (target) => {
      const count = stageCount(target);
      if (count <= 1 || index >= count) return;
      const [stage] = target.dpi_stages.splice(index, 1);
      target.dpi_stages.splice(count - 1, 0, stage);
      const [color] = target.dpi_colors.splice(index, 1);
      if (color) target.dpi_colors.splice(count - 1, 0, color);
      const current = target.current_dpi_stage;
      target.dpi_stage_count = count - 1;
      target.current_dpi_stage =
        current > index ? current - 1 : Math.min(current, count - 2);
    },
    run: async () => {
      setState(await api.deleteDpiStage(index));
    },
  });
  renderPage();
}

function activateStage(index: number | undefined): void {
  const p = profile();
  if (index === undefined || !p) return;
  if (!writeEnabled()) {
    showToast(t("common.writeDisabled"), "error");
    return;
  }
  structSeq += 1;
  queueTask(`struct:${structSeq}`, {
    apply: (target) => {
      target.current_dpi_stage = index;
    },
    run: async () => {
      setState(await api.setCurrentStage(index));
    },
  });
  renderPage();
}

function positionWheelCursor(hex: string): void {
  const wheel = document.querySelector<HTMLElement>("#led-color-wheel");
  const cursor = document.querySelector<HTMLElement>("#led-color-cursor");
  if (!wheel || !cursor) return;
  const [r, g, b] = parseColor(hex);
  const { h, s } = rgbToHsv(r, g, b);
  const size = wheel.clientWidth || 150;
  const radius = size / 2 - 10;
  const angle = (h * Math.PI) / 180;
  cursor.style.left = `${size / 2 + Math.sin(angle) * s * radius}px`;
  cursor.style.top = `${size / 2 - Math.cos(angle) * s * radius}px`;
  cursor.style.background = hex;
}

function selectedLedEffect(): LedEffect {
  const value = document.querySelector<HTMLButtonElement>("[data-mode-card].selected")
    ?.dataset.modeCard;
  if (value === "steady" || value === "breathing" || value === "off") return value;
  return ledEffect(profile()?.main_led[0] ?? 0);
}

function updateLightingPreview(): void {
  const preview = document.querySelector<HTMLElement>("[data-lighting-preview]");
  if (!preview) return;
  const effect = selectedLedEffect();
  const color =
    document.querySelector<HTMLInputElement>("#led-color")?.value ?? "#ffffff";
  const brightness = Number(
    document.querySelector<HTMLInputElement>("#led-brightness")?.value ?? LED_BRIGHTNESS_MAX,
  );
  const speed = Number(
    document.querySelector<HTMLInputElement>("#led-speed")?.value ?? 3,
  );
  const setText = (id: string, value: string) => {
    const node = document.getElementById(id);
    if (node) node.textContent = value;
  };
  setText(
    "led-mode-caption",
    t(LED_EFFECTS.find((item) => item.effect === effect)?.key ?? "led.off"),
  );
  setText(
    "led-param-label",
    effect === "breathing" ? t("lighting.speed") : t("lighting.brightness"),
  );
  setText(
    "led-param-fact",
    effect === "steady"
      ? `${brightness} / ${LED_BRIGHTNESS_MAX}`
      : effect === "breathing"
        ? `${speed} / ${LED_SPEED_MAX}`
        : "—",
  );
  setText("led-color-fact", color.toUpperCase());
  const chip = document.getElementById("led-color-chip");
  if (chip) chip.style.background = color;
  document.querySelectorAll<HTMLElement>("[data-led-field]").forEach((field) => {
    field.hidden = field.dataset.ledField !== effect;
  });
  const fill = (id: string, value: number, min: number, max: number) =>
    document
      .querySelector<HTMLInputElement>(id)
      ?.style.setProperty("--fill", `${((value - min) / (max - min)) * 100}%`);
  fill("#led-brightness", brightness, 1, LED_BRIGHTNESS_MAX);
  fill("#led-speed", speed, 1, LED_SPEED_MAX);
  document
    .querySelectorAll<HTMLElement>("#led-brightness-ticks span")
    .forEach((tick, index) => tick.classList.toggle("lit", index + 1 <= brightness));
  document
    .querySelectorAll<HTMLElement>("#led-speed-ticks span")
    .forEach((tick, index) => tick.classList.toggle("lit", index + 1 <= speed));
  preview.style.setProperty("--led-color", color);
  preview.style.setProperty(
    "--led-opacity",
    effect === "steady" ? String(0.15 + 0.85 * (brightness / LED_BRIGHTNESS_MAX)) : "1",
  );
  preview.style.setProperty("--led-duration", `${ledBreathSeconds(speed)}s`);
  preview.classList.remove("fx-steady", "fx-breathing", "fx-off");
  preview.classList.add(`fx-${effect}`);
}

function setWheelColor(hex: string): void {
  const input = document.querySelector<HTMLInputElement>("#led-color");
  const value = document.querySelector<HTMLElement>("#led-color-value");
  if (input) input.value = hex;
  if (value) value.textContent = hex.toUpperCase();
  positionWheelCursor(hex);
  updateLightingPreview();
}

function syncOutputs(): void {
  const speed = document.querySelector<HTMLInputElement>("#led-speed");
  const speedOut = document.querySelector<HTMLOutputElement>(
    "#led-speed-output",
  );
  if (speed && speedOut) {
    speed.addEventListener("input", () => {
      speedOut.value = speed.value;
      updateLightingPreview();
    });
  }
  const brightness = document.querySelector<HTMLInputElement>("#led-brightness");
  const brightnessOut = document.querySelector<HTMLOutputElement>(
    "#led-brightness-output",
  );
  if (brightness && brightnessOut) {
    brightness.addEventListener("input", () => {
      brightnessOut.value = brightness.value;
      updateLightingPreview();
    });
  }

  const colorInput = document.querySelector<HTMLInputElement>("#led-color");
  colorInput?.addEventListener("input", () => {
    if (colorInput.value) setWheelColor(colorInput.value);
  });
  const wheel = document.querySelector<HTMLElement>("#led-color-wheel");
  const updateWheel = (event: PointerEvent) => {
    if (!wheel) return;
    const rect = wheel.getBoundingClientRect();
    const radius = rect.width / 2;
    const dx = event.clientX - (rect.left + radius);
    const dy = event.clientY - (rect.top + radius);
    const saturation = Math.min(1, Math.hypot(dx, dy) / radius);
    const hue = (Math.atan2(dx, -dy) * 180) / Math.PI;
    setWheelColor(hsvToHex((hue + 360) % 360, saturation, 1));
  };
  if (wheel) {
    wheel.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      wheel.setPointerCapture(event.pointerId);
      updateWheel(event);
    });
    wheel.addEventListener("pointermove", (event) => {
      if (wheel.hasPointerCapture(event.pointerId)) updateWheel(event);
    });
    wheel.addEventListener("pointerup", (event) => {
      if (wheel.hasPointerCapture(event.pointerId)) {
        wheel.releasePointerCapture(event.pointerId);
        queueLighting();
      }
    });
    wheel.addEventListener("pointercancel", (event) => {
      if (wheel.hasPointerCapture(event.pointerId)) {
        wheel.releasePointerCapture(event.pointerId);
      }
    });
  }
  if (colorInput) {
    positionWheelCursor(colorInput.value);
    updateLightingPreview();
  }
}

function scheduleStatusPolling(): void {
  if (statusTimer !== undefined) {
    window.clearInterval(statusTimer);
    statusTimer = undefined;
  }
  if (document.hidden || !document.hasFocus()) return;
  statusTimer = window.setInterval(() => {
    if (!document.hidden && document.hasFocus()) void refreshStatus();
  }, 2500);
}

function setUiPaused(paused: boolean): void {
  document.body.classList.toggle("app-paused", paused);
  if (paused) {
    if (statusTimer !== undefined) {
      window.clearInterval(statusTimer);
      statusTimer = undefined;
    }
    return;
  }
  void refreshStatus();
  scheduleStatusPolling();
}

function clearMacroDragVisuals(): void {
  document
    .querySelectorAll<HTMLElement>(
      ".macro-block.dragging, .macro-block.drag-before, .macro-block.drag-after",
    )
    .forEach((node) =>
      node.classList.remove("dragging", "drag-before", "drag-after"),
    );
  document.body.classList.remove("macro-dragging");
}

function updateMacroDragTarget(clientX: number): void {
  if (!timelineBlockDrag || !macroEvents) return;
  const drag = timelineBlockDrag;
  const blocks = Array.from(
    document.querySelectorAll<HTMLElement>("[data-macro-block]"),
  ).filter((node) => node !== drag.block);
  let dropIndex = blocks.length;
  let before: HTMLElement | null = null;
  let after: HTMLElement | null = null;
  for (const [index, node] of blocks.entries()) {
    const rect = node.getBoundingClientRect();
    if (clientX < rect.left + rect.width / 2) {
      dropIndex = index;
      before = node;
      break;
    }
    after = node;
  }
  drag.dropIndex = Math.max(
    0,
    Math.min(macroEvents.events.length - 1, dropIndex),
  );
  blocks.forEach((node) => node.classList.remove("drag-before", "drag-after"));
  before?.classList.add("drag-before");
  if (!before && after) after.classList.add("drag-after");
}

function wireTitlebar(): void {
  const appWindow = getCurrentWindow();
  document.addEventListener("click", (event) => {
    const button = (event.target as Element).closest<HTMLElement>("[data-win]");
    if (!button) return;
    const action = button.dataset.win;
    if (action === "min") void appWindow.minimize();
    if (action === "max") void appWindow.toggleMaximize();
    if (action === "close") void appWindow.close();
  });
}

async function boot(): Promise<void> {
  document.documentElement.lang = lang();
  wireTitlebar();
  await restoreTheme();
  batteryAlerts = await api.getBatteryAlerts().catch(() => true);
  // Любое нажатие в интерфейсе завершает запись: действие (например,
  // сохранение) выполнится уже над законченным макросом.
  document.addEventListener(
    "click",
    (event) => {
      if (!recording && recordCountdown <= 0) return;
      const target = event.target as Element;
      if (target.closest("button, label, summary, a, [data-action], [data-key-zone]")) {
        stopRecording();
      }
    },
    true,
  );
  document.addEventListener(
    "pointerdown",
    (event) => {
      if (!recording && recordCountdown <= 0) return;
      const target = event.target as Element;
      if (target.closest("select, input, .macro-block, .shredder")) {
        stopRecording();
      }
    },
    true,
  );
  // Неактивная кнопка не даёт события click, но нажатие на неё — тоже нажатие.
  document.addEventListener(
    "pointerdown",
    (event) => {
      if (!recording && recordCountdown <= 0) return;
      const pressed = document
        .elementsFromPoint(event.clientX, event.clientY)
        .some((node) => node.matches("button:disabled"));
      if (pressed) stopRecording();
    },
    true,
  );
  document.addEventListener("click", (event) => {
    const wrap = document.querySelector<HTMLElement>("#menu-wrap");
    if (wrap && !wrap.contains(event.target as Node)) {
      document.querySelector<HTMLElement>("#menu-panel")?.classList.add("hidden");
    }
  });
  renderShell();
  await loadState(false);
  renderPage();
  setUiPaused(document.hidden || !document.hasFocus());
  document.addEventListener("visibilitychange", () =>
    setUiPaused(document.hidden || !document.hasFocus()),
  );
  window.addEventListener("pointermove", (event) => {
    if (shredDrag) {
      setShredProgress(
        shredDrag,
        clamp((event.clientX - shredDrag.startX) / shredDrag.distance, 0, 1),
      );
      return;
    }
    if (timelineBlockDrag) {
      const distance = Math.hypot(
        event.clientX - timelineBlockDrag.startX,
        event.clientY - timelineBlockDrag.startY,
      );
      if (!timelineBlockDrag.active && distance > 5) {
        timelineBlockDrag.active = true;
        timelineBlockDrag.block.classList.add("dragging");
        document.body.classList.add("macro-dragging");
      }
      if (timelineBlockDrag.active) {
        event.preventDefault();
        updateMacroDragTarget(event.clientX);
      }
      return;
    }
    if (timelineResize) {
      const delta = event.clientX - timelineResize.startX;
      setEventDelay(
        timelineResize.index,
        timelineResize.startDelay + delta * MACRO_RESIZE_MS_PER_PX,
      );
      return;
    }
    if (
      stageLongPress &&
      !stageLongPress.armed &&
      Math.hypot(
        event.clientX - stageLongPress.startX,
        event.clientY - stageLongPress.startY,
      ) > 4
    ) {
      cancelStageLongPress();
    }
    if (!ladderDrag) return;
    if (!ladderDrag.moved) {
      if (Math.abs(event.clientY - ladderDrag.startY) < 4) return;
      ladderDrag.moved = true;
    }
    const rect = ladderDrag.track.getBoundingClientRect();
    setStageValue(
      ladderDrag.stage,
      dpiFromLadderRatio(1 - (event.clientY - rect.top) / rect.height),
    );
  });
  window.addEventListener("pointerup", () => {
    if (shredDrag) {
      finishShred();
      return;
    }
    if (timelineBlockDrag) {
      const drag = timelineBlockDrag;
      timelineBlockDrag = null;
      clearMacroDragVisuals();
      if (drag.active && macroEvents) {
        const to = Math.max(
          0,
          Math.min(macroEvents.events.length - 1, drag.dropIndex),
        );
        if (to !== drag.index) {
          const [item] = macroEvents.events.splice(drag.index, 1);
          macroEvents.events.splice(to, 0, item);
          renderPage();
        }
      }
      return;
    }
    if (timelineResize) {
      timelineResize = null;
      renderPage();
      return;
    }
    if (stageLongPress && !stageLongPress.armed) cancelStageLongPress();
    if (ladderDrag) {
      const { stage, moved } = ladderDrag;
      ladderDrag = null;
      if (moved) markStagePending(stage, "dpi");
    }
  });
  window.addEventListener("pointercancel", () => {
    if (shredDrag) {
      shredDrag.progress = 0;
      finishShred();
    }
    ladderDrag = null;
    if (stageLongPress && !stageLongPress.armed) cancelStageLongPress();
    timelineResize = null;
    timelineBlockDrag = null;
    clearMacroDragVisuals();
  });
  window.addEventListener("keydown", (event) => {
    if (!recording || !macroEvents) return;
    if (event.key === "Escape") {
      event.preventDefault();
      stopRecording();
      return;
    }
    const target = event.target as HTMLElement;
    if (target.matches("input, select, textarea")) return;
    if (event.repeat) return;
    const key = event.keyCode;
    if (!isRecordableMacroKey(key)) return;
    event.preventDefault();
    recordedDown.add(key);
    pushRecordedEvent(key, 1);
  });
  window.addEventListener("keyup", (event) => {
    if (!recording) return;
    const key = event.keyCode;
    if (!recordedDown.has(key)) return;
    recordedDown.delete(key);
    if (!isRecordableMacroKey(key)) return;
    event.preventDefault();
    pushRecordedEvent(key, 0);
  });
  // Мягкий блик карточки под курсором (координаты в CSS-переменных).
  let spotlightFrame = 0;
  document.addEventListener("pointermove", (event) => {
    if (spotlightFrame) return;
    spotlightFrame = window.requestAnimationFrame(() => {
      spotlightFrame = 0;
      const card = (event.target as Element | null)?.closest?.<HTMLElement>(".card");
      if (!card) return;
      const rect = card.getBoundingClientRect();
      card.style.setProperty("--mx", `${event.clientX - rect.left}px`);
      card.style.setProperty("--my", `${event.clientY - rect.top}px`);
    });
  });
  let blurredAt = 0;
  window.addEventListener("blur", () => {
    if (recording || recordCountdown > 0) stopRecording();
    blurredAt = Date.now();
    setUiPaused(true);
  });
  window.addEventListener("focus", () => {
    setUiPaused(false);
    // Профиль живёт в памяти программы: после возврата в окно сверяемся
    // с мышью — её могли перенастроить родной программой.
    if (
      blurredAt &&
      Date.now() - blurredAt > 2000 &&
      state?.connected &&
      !hasPendingWrites() &&
      canRenderNow()
    ) {
      void loadState(false);
    }
  });
}

void boot();
