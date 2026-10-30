export type ConnectionLabel = "USB" | "2.4G" | "offline";
export type Page = "dpi" | "buttons" | "lighting" | "macros" | "themes";
export type Lang = "ru" | "en";

export interface DpiStage {
  x: number;
  y: number;
  multiplier: number;
  checksum_ok: boolean;
  raw: number[];
}

export interface DpiColor {
  r: number;
  g: number;
  b: number;
  checksum_ok: boolean;
}

export interface KeySlot {
  code: number;
  checksum_ok: boolean;
}

export interface Profile {
  raw: number[];
  polling_code: number;
  polling_hz: number | null;
  dpi_stage_count: number;
  current_dpi_stage: number;
  lod: number;
  dpi_stages: DpiStage[];
  dpi_colors: DpiColor[];
  main_led: number[];
  logo_led: number[];
  led_extra: number[];
  keys: KeySlot[];
  mouse_params: number[];
  debounce: number;
  param_ab: number;
  sleep: number;
  param_af: number;
  param_b1: number;
  param_b3: number;
  param_b5: number;
  param_b7: number;
}

export interface StateDto {
  connected: boolean;
  error: string | null;
  connection: ConnectionLabel;
  vendor_id: number;
  pid: number;
  firmware: number;
  cid: number;
  mid: number;
  battery: number;
  charging: boolean;
  write_enabled: boolean;
  profile: Profile | null;
  backup: string | null;
}

export interface StatusDto {
  connected: boolean;
  error: string | null;
  connection: ConnectionLabel;
  pid: number;
  firmware: number;
  battery: number;
  charging: boolean;
  write_enabled: boolean;
}

export interface MacroDto {
  index: number;
  name: string | null;
  assigned_slot: number | null;
}

export interface SoftwareMacroEvent {
  key: number;
  state: number;
  delay_ms: number;
}

export interface SoftwareMacro {
  name: string;
  events: SoftwareMacroEvent[];
}
