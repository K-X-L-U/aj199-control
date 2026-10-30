import { invoke } from "@tauri-apps/api/core";
import type {
  MacroDto,
  SoftwareMacro,
  SoftwareMacroEvent,
  StateDto,
  StatusDto,
} from "./types";

export const api = {
  state: () => invoke<StateDto>("get_state"),
  status: () => invoke<StatusDto>("get_status"),
  backup: () => invoke<string>("backup"),
  exportProfile: () => invoke<string>("export_profile"),
  restoreProfile: (json: string) => invoke<StateDto>("restore_profile", { json }),
  macros: () => invoke<MacroDto[]>("get_macros"),
  setMacroName: (index: number, name: string) =>
    invoke<StateDto>("set_macro_name", { index, name }),
  findEmptyMacroSlot: () => invoke<number | null>("find_empty_macro_slot"),
  deleteMacro: (index: number) => invoke<StateDto>("delete_macro", { index }),
  readMacro: (index: number) => invoke<SoftwareMacro>("read_macro", { index }),
  writeMacro: (index: number, name: string, events: SoftwareMacroEvent[]) =>
    invoke<StateDto>("write_macro", { index, name, events }),
  encodeJmm: (name: string, events: SoftwareMacroEvent[]) =>
    invoke<number[]>("encode_jmm", { name, events }),
  parseJmm: (bytes: number[]) => invoke<SoftwareMacro>("parse_jmm", { bytes }),
  setDpi: (stage: number, x: number, y: number) =>
    invoke<StateDto>("set_dpi", { stage, x, y }),
  setCurrentStage: (stage: number) =>
    invoke<StateDto>("set_current_stage", { stage }),
  addDpiStage: () => invoke<StateDto>("add_dpi_stage"),
  deleteDpiStage: (index: number) =>
    invoke<StateDto>("delete_dpi_stage", { index }),
  testWrite: () => invoke<StateDto>("test_write"),
  getTheme: () => invoke<string | null>("get_theme"),
  saveThemeArt: (theme: string, logo: number[], low: number[], full: number[]) =>
    invoke<void>("save_theme_art", { theme, logo, low, full }),
  getBatteryAlerts: () => invoke<boolean>("get_battery_alerts"),
  setBatteryAlerts: (enabled: boolean) =>
    invoke<void>("set_battery_alerts", { enabled }),
  testBatteryToast: (kind: string) => invoke<void>("test_battery_toast", { kind }),
  setTheme: (theme: string) => invoke<void>("set_theme", { theme }),
  setThemeIcon: (rgba: number[], width: number, height: number) =>
    invoke<void>("set_theme_icon", { rgba, width, height }),
  setDpiColor: (stage: number, r: number, g: number, b: number) =>
    invoke<StateDto>("set_dpi_color", { stage, r, g, b }),
  setPolling: (hz: number) => invoke<StateDto>("set_polling", { hz }),
  setDebounce: (value: number) => invoke<StateDto>("set_debounce", { value }),
  setLod: (value: number) => invoke<StateDto>("set_lod", { value }),
  setAngleSnap: (enabled: boolean) =>
    invoke<StateDto>("set_angle_snap", { enabled }),
  setRipple: (enabled: boolean) => invoke<StateDto>("set_ripple", { enabled }),
  setLed: (mode: number, speed: number, brightness: number) =>
    invoke<StateDto>("set_led", { mode, speed, brightness }),
  setKey: (slot: number, action?: string, code?: number) =>
    invoke<StateDto>("set_key", {
      slot,
      action: action ?? null,
      code: code ?? null,
    }),
  resetProfile: () => invoke<StateDto>("reset_profile"),
};
