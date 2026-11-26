import { Fragment, useEffect, useState, type RefObject } from 'react';
import {
  fmtDb,
  fmtValue,
  PRESETS,
  SECTIONS,
  type BoolKey,
  type MeterData,
  type NumKey,
  type Params,
  type SliderDef,
} from './types';
import { t, useLang } from './i18n';

interface Props {
  params: Params;
  preset: string | null;
  profiles: string[];
  profile: string | null;
  ptt: boolean;
  meter: RefObject<MeterData>;
  onNum: (key: NumKey, v: number) => void;
  onBool: (key: BoolKey, v: boolean) => void;
  onPreset: (name: string) => void;
  onProfile: (name: string) => void;
  onPtt: (on: boolean) => void;
  onCalibrate: () => void;
  sounds: { id: number; name: string }[];
  onPlay: (id: number) => void;
  onStopSounds: () => void;
}

function SliderRow({ def, value, onChange }: { def: SliderDef; value: number; onChange: (v: number) => void }) {
  const bipolar = def.min < 0 && def.max > 0;
  const pct = (v: number) => ((v - def.min) / (def.max - def.min)) * 100;
  const mid = bipolar ? pct(0) : 0;
  const p = pct(value);
  const style = { '--a': `${Math.min(p, mid)}%`, '--b': `${Math.max(p, mid)}%` } as React.CSSProperties;
  const id = `c-${def.key}`;
  return (
    <div className="ctl">
      <div className="top">
        <label htmlFor={id}>{t(def.label)}</label>
        <output htmlFor={id}>{fmtValue(value, def.unitKey ? t(def.unitKey) : def.unit, bipolar)}</output>
      </div>
      <input
        id={id}
        type="range"
        min={def.min}
        max={def.max}
        step={1}
        value={value}
        style={style}
        onChange={(e) => onChange(Number(e.currentTarget.value))}
      />
      {def.hint && <span className="h">{t(def.hint)}</span>}
    </div>
  );
}

/** Tự đặt ngưỡng: im lặng 3 giây, PC đo nền ồn rồi đặt ngưỡng cổng ngay phía trên nó. */
function CalibrateRow({ meter, thr, onStart }: { meter: RefObject<MeterData>; thr: number; onStart: () => void }) {
  useLang();
  const [cal, setCal] = useState({ st: 0, p: 0, n: -99 });

  useEffect(() => {
    const id = window.setInterval(() => {
      const m = meter.current;
      if (!m) return;
      setCal((c) => (c.st === m.cal && c.n === m.caln && Math.abs(c.p - m.calp) < 0.04 ? c : { st: m.cal, p: m.calp, n: m.caln }));
    }, 150);
    return () => window.clearInterval(id);
  }, [meter]);

  if (cal.st === 1) {
    return (
      <div className="cal" role="status">
        <p>{t('cal.measuring')}</p>
        <div className="calbar">
          <div style={{ width: `${Math.round(cal.p * 100)}%` }} />
        </div>
      </div>
    );
  }
  if (cal.st === 2) {
    return (
      <div className="cal" role="status">
        <p>{t('cal.done', fmtDb(cal.n), fmtDb(thr))}</p>
        {cal.n > -35 && <p className="warn">{t('cal.noisy')}</p>}
      </div>
    );
  }
  if (cal.st === 3) {
    return (
      <div className="cal" role="status">
        <p className="warn">{t('cal.fail')}</p>
      </div>
    );
  }
  return (
    <button type="button" className="calbtn" onClick={onStart}>
      {t('cal.btn')}
    </button>
  );
}

export function Controls({
  params, preset, profiles, profile, ptt, meter, onNum, onBool, onPreset, onProfile, onPtt, onCalibrate, sounds, onPlay, onStopSounds,
}: Props) {
  useLang();
  return (
    <>
      <div className="presets" role="group" aria-label={t('presets.aria')}>
        {PRESETS.map((p) => (
          <button key={p.key} type="button" aria-pressed={preset === p.key} onClick={() => onPreset(p.key)}>
            {t(p.label)}
          </button>
        ))}
        <button
          type="button"
          className="mute"
          aria-pressed={params.mute}
          onClick={() => onBool('mute', !params.mute)}
        >
          {t('chip.mute')}
        </button>
      </div>

      {profiles.length > 0 && (
        <div className="profiles" role="group" aria-label={t('profiles')}>
          <span className="plabel">{t('profiles')}</span>
          {profiles.map((n) => (
            <button key={n} type="button" aria-pressed={profile === n} onClick={() => onProfile(n)}>
              {n}
            </button>
          ))}
        </div>
      )}

      {sounds.length > 0 && (
        <div className="profiles" role="group" aria-label={t('sounds.title')}>
          <span className="plabel">{t('sounds.title')}</span>
          {sounds.map((s) => (
            <button key={s.id} type="button" onClick={() => onPlay(s.id)}>
              {s.name}
            </button>
          ))}
          <button type="button" className="mute" onClick={onStopSounds}>
            {t('sounds.stop')}
          </button>
        </div>
      )}

      <label className="sw" htmlFor="c-ptt">
        <span>
          <span className="t">{t('ptt.title')}</span>
          <span className="h">{t('ptt.hint')}</span>
        </span>
        <input id="c-ptt" type="checkbox" role="switch" checked={ptt} onChange={(e) => onPtt(e.currentTarget.checked)} />
      </label>

      {SECTIONS.map((sec) => (
        <section key={sec.title}>
          <h2>{t(sec.title)}</h2>
          {sec.lead && <p className="lead">{t(sec.lead)}</p>}
          {sec.items.map((it) =>
            it.kind === 'switch' ? (
              <label className="sw" key={it.key} htmlFor={`c-${it.key}`}>
                <span>
                  <span className="t">{t(it.label)}</span>
                  {it.hint && <span className="h">{t(it.hint)}</span>}
                </span>
                <input
                  id={`c-${it.key}`}
                  type="checkbox"
                  role="switch"
                  checked={params[it.key]}
                  onChange={(e) => onBool(it.key, e.currentTarget.checked)}
                />
              </label>
            ) : (
              <Fragment key={it.key}>
                <SliderRow def={it} value={params[it.key]} onChange={(v) => onNum(it.key, v)} />
                {it.key === 'thr' && <CalibrateRow meter={meter} thr={params.thr} onStart={onCalibrate} />}
              </Fragment>
            ),
          )}
        </section>
      ))}
    </>
  );
}
