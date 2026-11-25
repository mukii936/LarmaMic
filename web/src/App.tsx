import { Controls } from './Controls';
import { Meter } from './Meter';
import { useMic, type Status } from './useMic';
import { chosen, LANGS, NAMES, setLang, t, useLang, type Key, type Lang } from './i18n';

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

export function App() {
  const {
    status, message, params, preset, profiles, profile, ptt, meter,
    setParam, choosePreset, chooseProfile, calibrate, enablePtt, setMuteNow, start, stop,
  } = useMic();
  useLang();
  const running = status !== 'idle' && status !== 'error';
  const talking = ptt && params ? !params.mute : false;

  return (
    <main data-s={status}>
      <header>
        <div className="top">
          <p className="app">LarmaMic</p>
          <LanguagePicker />
        </div>
        <h1>{t(TITLE[status])}</h1>
        <p className="sub" role="status">
          {message ? t(message.k, ...(message.a ?? [])) : SUB[status] ? t(SUB[status]) : ''}
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
        <button type="button" className="go" onClick={running ? stop : start}>
          {t(running ? 'go.stop' : 'go.start')}
        </button>
      </footer>
    </main>
  );
}
