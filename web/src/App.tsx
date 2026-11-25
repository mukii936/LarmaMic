import { useEffect, useRef, useState } from 'react';
import { Controls } from './Controls';
import { Meter } from './Meter';
import { useMic, type Status } from './useMic';
import { browserName, chosen, LANGS, NAMES, setLang, t, useLang, type Key, type Lang } from './i18n';

const TITLE: Record<Status, Key> = {
  idle: 'title.idle',
  connecting: 'title.connecting',
  live: 'title.live',
  retry: 'title.retry',
  error: 'title.error',
};

const SUB: Record<Status, Key | null> = {
  idle: 'sub.idle',
  connecting: 'sub.connecting',
  live: 'sub.live',
  retry: 'sub.retry',
  error: null,
};

function LanguagePicker() {
  const lang = useLang();
  return (
    <label className="lang">
      <span className="sr">{t('lang.title')}</span>
      <select
        value={chosen()}
        aria-label={t('lang.title')}
        onChange={(e) => setLang(e.currentTarget.value as Lang | 'auto')}
        data-cur={lang}
      >
        <option value="auto">{`${t('lang.auto')} · ${NAMES[lang]}`}</option>
        {LANGS.map((l) => (
          <option key={l} value={l}>
            {NAMES[l]}
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * Chế độ tiết kiệm pin: cả màn hình đen hẳn (màn OLED tắt điểm ảnh nên đỡ tốn pin), mic vẫn truyền bình thường.
 * Nhấp đúp ở bất kỳ đâu để hiện lại. Dòng gợi ý chỉ hiện mờ vài giây rồi tắt hẳn.
 */
function SaverScreen({ onExit }: { onExit: () => void }) {
  const last = useRef({ at: 0, x: 0, y: 0 });
  const [hint, setHint] = useState(true);
  useLang();

  useEffect(() => {
    const meta = document.querySelector('meta[name="theme-color"]');
    const old = meta?.getAttribute('content') ?? null;
    meta?.setAttribute('content', '#000000');
    const id = window.setTimeout(() => setHint(false), 3500);
    return () => {
      window.clearTimeout(id);
      if (meta && old) meta.setAttribute('content', old);
    };
  }, []);

  return (
    <div
      className="saver"
      onPointerUp={(e) => {
        const now = Date.now();
        const p = last.current;
        if (now - p.at < 400 && Math.hypot(e.clientX - p.x, e.clientY - p.y) < 60) {
          last.current = { at: 0, x: 0, y: 0 };
          onExit();
        } else {
          last.current = { at: now, x: e.clientX, y: e.clientY };
        }
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {hint && <p>{t('saver.hint')}</p>}
    </div>
  );
}

export function App() {
  const {
    status, message, params, preset, profiles, profile, ptt, meter,
    setParam, choosePreset, chooseProfile, calibrate, enablePtt, setMuteNow, start, stop,
  } = useMic();
  useLang();
  const [saver, setSaver] = useState(false);
  const running = status !== 'idle' && status !== 'error';
  const talking = ptt && params ? !params.mute : false;

  // Mất kết nối hẳn hoặc đã dừng thì thoát chế độ tiết kiệm để người dùng thấy chuyện gì xảy ra
  useEffect(() => {
    if (status === 'idle' || status === 'error') setSaver(false);
  }, [status]);

  if (saver) return <SaverScreen onExit={() => setSaver(false)} />;

  return (
    <main data-s={status}>
      <header>
        <div className="top">
          <p className="app">LarmaMic</p>
          <LanguagePicker />
        </div>
        <h1>{t(TITLE[status])}</h1>
        <p className="sub" role="status">
          {message ? t(message.k, ...(message.a ?? [])) : SUB[status] ? t(SUB[status], browserName()) : ''}
        </p>
      </header>

      <Meter meter={meter} thr={params?.thr ?? -52} disabled={!params} onThr={(v) => setParam('thr', v)} />

      {params ? (
        <Controls
          params={params}
          preset={preset}
          profiles={profiles}
          profile={profile}
          ptt={ptt}
          meter={meter}
          onNum={setParam}
          onBool={setParam}
          onPreset={choosePreset}
          onProfile={chooseProfile}
          onPtt={enablePtt}
          onCalibrate={calibrate}
        />
      ) : (
        <p className="hint">{t('hint.keepOn')}</p>
      )}

      <footer>
        {ptt && params && (
          <button
            type="button"
            className="talk"
            data-on={talking}
            onPointerDown={(e) => {
              e.currentTarget.setPointerCapture(e.pointerId);
              setMuteNow(false);
            }}
            onPointerUp={() => setMuteNow(true)}
            onPointerCancel={() => setMuteNow(true)}
            onContextMenu={(e) => e.preventDefault()}
          >
            {t(talking ? 'talk.on' : 'talk.off')}
          </button>
        )}
        {(running || import.meta.env.DEV) && !ptt && (
          <button type="button" className="saverbtn" onClick={() => setSaver(true)}>
            {t('saver.btn')}
          </button>
        )}
        <button type="button" className="go" onClick={running ? stop : start}>
          {t(running ? 'go.stop' : 'go.start')}
        </button>
      </footer>
    </main>
  );
}
