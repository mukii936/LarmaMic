import { useEffect, useRef, type RefObject } from 'react';
import { EMPTY_METER, fmtDb, type MeterData } from './types';
import { t, useLang } from './i18n';

const MIN = -72;
const SEG = 48;
const THR_MIN = -72;
const THR_MAX = -20;

interface Props {
  meter: RefObject<MeterData>;
  thr: number;
  disabled: boolean;
  onThr: (db: number) => void;
}

/** Đồng hồ mức âm giống bên PC: vạch trắng là ngưỡng cổng ồn, kéo ngang để đặt. */
export function Meter({ meter, thr, disabled, onThr }: Props) {
  useLang();
  const cv = useRef<HTMLCanvasElement>(null);
  const readIn = useRef<HTMLSpanElement>(null);
  const readOut = useRef<HTMLSpanElement>(null);
  const readGate = useRef<HTMLSpanElement>(null);
  const readLat = useRef<HTMLElement>(null);
  const thrRef = useRef(thr);
  thrRef.current = thr;

  useEffect(() => {
    const canvas = cv.current;
    if (!canvas) return;
    const g = canvas.getContext('2d');
    if (!g) return;
    const css = getComputedStyle(document.documentElement);
    const col = (n: string) => css.getPropertyValue(n).trim();
    const [sig, warn, off, ghost, mute, ink] = ['--sig', '--warn', '--off', '--ghost', '--mute', '--ink'].map(col);

    let w = 0;
    let h = 0;
    const size = () => {
      const r = canvas.getBoundingClientRect();
      const d = window.devicePixelRatio || 1;
      w = r.width;
      h = r.height;
      canvas.width = Math.round(w * d);
      canvas.height = Math.round(h * d);
      g.setTransform(d, 0, 0, d, 0, 0);
    };
    size();
    const ro = new ResizeObserver(size);
    ro.observe(canvas);

    let dIn = -99;
    let dOut = -99;
    let peak = -99;
    let peakAt = 0;
    let last = performance.now();
    let tick = 0;
    let raf = 0;
    const xOf = (db: number) => ((Math.min(0, Math.max(MIN, db)) - MIN) / -MIN) * w;

    const frame = (now: number) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const m = meter.current ?? EMPTY_METER;
      dIn = Math.max(m.in, dIn - 40 * dt);
      dOut = Math.max(m.out, dOut - 48 * dt);
      if (dOut >= peak) {
        peak = dOut;
        peakAt = now;
      } else if (now - peakAt > 900) {
        peak = Math.max(dOut, peak - 30 * dt);
      }

      g.clearRect(0, 0, w, h);
      const top = 26;
      const bh = h - top - 20;
      const sw = w / SEG;
      const step = -MIN / SEG;
      for (let i = 0; i < SEG; i++) {
        const db = MIN + (i + 1) * step;
        g.fillStyle = db <= dOut ? (db > -3 ? warn : sig) : db <= dIn ? ghost : off;
        g.beginPath();
        g.roundRect(i * sw + 1, top, Math.max(1, sw - 2), bh, 2);
        g.fill();
      }
      if (peak > MIN) {
        g.fillStyle = peak > -3 ? warn : ink;
        g.fillRect(Math.min(w - 2, Math.max(0, xOf(peak) - 1)), top - 4, 2, bh + 8);
      }
      const tx = xOf(thrRef.current);
      g.fillStyle = '#fff';
      g.fillRect(tx - 1, top - 8, 2, bh + 16);
      g.beginPath();
      g.moveTo(tx - 6, top - 16);
      g.lineTo(tx + 6, top - 16);
      g.lineTo(tx, top - 8);
      g.closePath();
      g.fill();
      g.fillStyle = mute;
      g.font = '11px system-ui, sans-serif';
      g.textBaseline = 'alphabetic';
      for (const d of [-60, -48, -36, -24, -12, 0]) {
        g.textAlign = d === 0 ? 'right' : 'center';
        g.fillText(String(d).replace('-', '−'), xOf(d), h - 4);
      }

      if (tick++ % 6 === 0) {
        if (readIn.current) readIn.current.textContent = fmtDb(dIn);
        if (readOut.current) readOut.current.textContent = fmtDb(dOut);
        if (readGate.current) readGate.current.textContent = t(m.gate ? 'gate.open' : 'gate.closed');
        if (readLat.current) readLat.current.textContent = m.lat > 0 ? String(m.lat) : '—';
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, [meter]);

  const setFromPointer = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (disabled) return;
    const r = e.currentTarget.getBoundingClientRect();
    const db = MIN + ((e.clientX - r.left) / r.width) * -MIN;
    onThr(Math.round(Math.min(THR_MAX, Math.max(THR_MIN, db))));
  };

  return (
    <div className="meter">
      <canvas
        ref={cv}
        role="img"
        aria-label={t('meter.aria')}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          setFromPointer(e);
        }}
        onPointerMove={(e) => {
          if (e.buttons) setFromPointer(e);
        }}
      />
      <div className="read">
        <span>
          {t('meter.in')} <b ref={readIn}>−∞</b>
        </span>
        <span ref={readGate} />
        <span>
          {t('meter.out')} <b ref={readOut}>−∞</b>
        </span>
      </div>
      <div className="lat">
        {t('lat.pre')} <b ref={readLat}>—</b> {t('lat.post')}
      </div>
    </div>
  );
}
