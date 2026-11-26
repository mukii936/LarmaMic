import type { Key } from './i18n';

/** Thông số xử lý, trùng tên với `Params` bên Rust (src/settings.rs). */
export interface Params {
  ai: boolean;
  ai_mix: number;
  gate_on: boolean;
  thr: number;
  depth: number;
  hp: number;
  low: number;
  mud: number;
  pres: number;
  air: number;
  comp: number;
  gain: number;
  buf_ms: number;
  mute: boolean;
  monitor: boolean;
  agc: boolean;
  aec: boolean;
  fx_pitch: number;
  fx_reverb: number;
  fx_robot: boolean;
}

export type NumKey = { [K in keyof Params]: Params[K] extends number ? K : never }[keyof Params];
export type BoolKey = { [K in keyof Params]: Params[K] extends boolean ? K : never }[keyof Params];

/** Số đo PC gửi về khoảng 15 lần mỗi giây. */
export interface MeterData {
  in: number;
  out: number;
  gate: boolean;
  buf: number;
  under: number;
  drops: number;
  /** Tự đặt ngưỡng: 0 rảnh, 1 đang đo, 2 xong, 3 thất bại. */
  cal: number;
  /** Tiến độ đo 0..1. */
  calp: number;
  /** Mức nền ồn đo được (dB). */
  caln: number;
  /** Độ trễ ước tính từ lúc nói đến lúc ra thiết bị (ms). */
  lat: number;
}

export const EMPTY_METER: MeterData = {
  in: -99, out: -99, gate: true, buf: 0, under: 0, drops: 0, cal: 0, calp: 0, caln: -99, lat: 0,
};

/** Chỉ dùng khi chạy `npm run dev`, để xem giao diện mà không cần nối với PC. Bị loại khỏi bản build. */
export const PREVIEW_PARAMS: Params = {
  ai: true, ai_mix: 85, gate_on: true, thr: -52, depth: 40, hp: 100, low: -1, mud: -3, pres: 3,
  air: 1, comp: 40, gain: 6, buf_ms: 60, mute: false, monitor: false,
  agc: false, aec: false, fx_pitch: 0, fx_reverb: 0, fx_robot: false,
};
export const PREVIEW_METER: MeterData = {
  in: -38, out: -14, gate: true, buf: 60, under: 0, drops: 0, cal: 0, calp: 0, caln: -99, lat: 96,
};
export const PREVIEW_PROFILES = ['Noisy room', 'Streaming'];

export const PRESETS = [
  { key: 'voice', label: 'preset.voice' },
  { key: 'game', label: 'preset.game' },
  { key: 'podcast', label: 'preset.podcast' },
  { key: 'raw', label: 'preset.raw' },
] as const satisfies readonly { key: string; label: Key }[];

export interface SwitchDef {
  kind: 'switch';
  key: BoolKey;
  label: Key;
  hint?: Key;
}

export interface SliderDef {
  kind: 'slider';
  key: NumKey;
  label: Key;
  hint?: Key;
  min: number;
  max: number;
  /** Đơn vị viết sẵn (" dB", "%"). */
  unit: string;
  /** Khóa dịch cho đơn vị phụ thuộc ngôn ngữ (thay cho `unit`). */
  unitKey?: Key;
}

export interface SectionDef {
  title: Key;
  lead?: Key;
  items: (SwitchDef | SliderDef)[];
}

export const SECTIONS: SectionDef[] = [
  {
    title: 'sec.noise',
    lead: 'sec.noise.lead',
    items: [
      { kind: 'switch', key: 'ai', label: 'ctl.ai' },
      { kind: 'slider', key: 'ai_mix', label: 'ctl.ai_mix', hint: 'ctl.ai_mix.hint', min: 0, max: 100, unit: '%' },
      { kind: 'switch', key: 'gate_on', label: 'ctl.gate', hint: 'ctl.gate.hint' },
      { kind: 'slider', key: 'thr', label: 'ctl.thr', hint: 'ctl.thr.hint', min: -72, max: -20, unit: ' dB' },
      { kind: 'slider', key: 'depth', label: 'ctl.depth', hint: 'ctl.depth.hint', min: 6, max: 80, unit: ' dB' },
      { kind: 'slider', key: 'hp', label: 'ctl.hp', hint: 'ctl.hp.hint', min: 40, max: 250, unit: ' Hz' },
      { kind: 'switch', key: 'aec', label: 'ctl.aec', hint: 'ctl.aec.hint' },
    ],
  },
  {
    title: 'sec.eq',
    lead: 'sec.eq.lead',
    items: [
      { kind: 'slider', key: 'low', label: 'ctl.low', hint: 'ctl.low.hint', min: -12, max: 12, unit: ' dB' },
      { kind: 'slider', key: 'mud', label: 'ctl.mud', hint: 'ctl.mud.hint', min: -12, max: 12, unit: ' dB' },
      { kind: 'slider', key: 'pres', label: 'ctl.pres', hint: 'ctl.pres.hint', min: -12, max: 12, unit: ' dB' },
      { kind: 'slider', key: 'air', label: 'ctl.air', hint: 'ctl.air.hint', min: -12, max: 12, unit: ' dB' },
    ],
  },
  {
    title: 'sec.vol',
    lead: 'sec.vol.lead',
    items: [
      { kind: 'switch', key: 'agc', label: 'ctl.agc', hint: 'ctl.agc.hint' },
      { kind: 'slider', key: 'comp', label: 'ctl.comp', hint: 'ctl.comp.hint', min: 0, max: 100, unit: '%' },
      { kind: 'slider', key: 'gain', label: 'ctl.gain', min: -12, max: 24, unit: ' dB' },
      { kind: 'slider', key: 'buf_ms', label: 'ctl.buf', hint: 'ctl.buf.hint', min: 30, max: 300, unit: ' ms' },
    ],
  },
  {
    title: 'sec.fx',
    lead: 'sec.fx.lead',
    items: [
      { kind: 'slider', key: 'fx_pitch', label: 'ctl.pitch', hint: 'ctl.pitch.hint', min: -12, max: 12, unit: '', unitKey: 'unit.semitones' },
      { kind: 'slider', key: 'fx_reverb', label: 'ctl.reverb', min: 0, max: 100, unit: '%' },
      { kind: 'switch', key: 'fx_robot', label: 'ctl.robot' },
    ],
  },
];

/** "−12 dB", "+3 dB", "55%" */
export function fmtValue(v: number, unit: string, signed: boolean): string {
  const n = Math.round(v);
  if (n < 0) return `−${-n}${unit}`;
  return `${signed && n > 0 ? '+' : ''}${n}${unit}`;
}

export function fmtDb(v: number): string {
  return v <= -90 ? '−∞' : `${fmtValue(v, ' dB', false)}`;
}
