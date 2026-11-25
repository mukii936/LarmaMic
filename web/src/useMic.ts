import { useCallback, useEffect, useRef, useState } from 'react';
import {
  EMPTY_METER,
  PREVIEW_METER,
  PREVIEW_PARAMS,
  PREVIEW_PROFILES,
  type BoolKey,
  type MeterData,
  type NumKey,
  type Params,
} from './types';
import { adoptPcLang, isIos, type Key } from './i18n';

export type Status = 'idle' | 'connecting' | 'live' | 'retry' | 'error';

/** Thông báo lưu dưới dạng khóa dịch (kèm tham số) để đổi ngôn ngữ thì chữ đổi theo. */
export interface Msg {
  k: Key;
  a?: string[];
}

interface TapMessage {
  pcm: ArrayBuffer;
  peak: number;
}

/** Mã ghép nối lấy từ địa chỉ trong mã QR (?k=...). */
export const TOKEN = (() => {
  const fromUrl = new URLSearchParams(location.search).get('k');
  try {
    if (fromUrl) localStorage.setItem('larma-k', fromUrl);
    return fromUrl ?? localStorage.getItem('larma-k') ?? '';
  } catch {
    return fromUrl ?? '';
  }
})();

type ServerMessage =
  | { t: 'ready' }
  | { t: 'denied' }
  | { t: 'ping'; ts: number }
  | { t: 'state'; p: Params; preset: string | null; profiles: string[]; profile: string | null; lang?: string }
  | ({ t: 'meter' } & MeterData);

interface Session {
  ctx: AudioContext | null;
  stream: MediaStream | null;
  ws: WebSocket | null;
  retry: number;
  wake: WakeLockSentinel | null;
  running: boolean;
  /** Đang bật lại mic sau khi quay về trang, tránh chạy chồng. */
  resuming: boolean;
}

const fresh = (): Session => ({
  ctx: null,
  stream: null,
  ws: null,
  retry: 0,
  wake: null,
  running: false,
  resuming: false,
});

/**
 * Lấy mic gửi PCM 16-bit sang app LarmaMic, và đồng bộ thông số xử lý hai chiều:
 * chỉnh trên iPhone thì PC đổi theo, chỉnh trên PC thì iPhone đổi theo.
 *
 * iOS dừng micro của trang web khi khóa màn hình hoặc thoát Safari. Khi quay lại trang,
 * hook tự mở lại micro và kết nối lại PC, không cần bấm nút.
 */
export function useMic() {
  const [status, setStatus] = useState<Status>('idle');
  const [message, setMessage] = useState<Msg | null>(null);
  const [params, setParams] = useState<Params | null>(import.meta.env.DEV ? PREVIEW_PARAMS : null);
  const [preset, setPreset] = useState<string | null>(import.meta.env.DEV ? 'voice' : null);
  const [profiles, setProfiles] = useState<string[]>(import.meta.env.DEV ? PREVIEW_PROFILES : []);
  const [profile, setProfile] = useState<string | null>(null);
  /** Chế độ "bấm giữ để nói": mic chỉ mở khi giữ nút. */
  const [ptt, setPttState] = useState(false);
  /** Số đo mức âm, ghi qua ref để không render lại 15 lần mỗi giây. */
  const meter = useRef<MeterData>(import.meta.env.DEV ? { ...PREVIEW_METER } : { ...EMPTY_METER });
  const s = useRef<Session>(fresh());
  const pending = useRef<Partial<Params>>({});
  const flushTimer = useRef(0);
  const lastEdit = useRef(0);

  const send = useCallback((obj: unknown) => {
    const ws = s.current.ws;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(obj));
  }, []);

  const holdScreen = useCallback(async () => {
    try {
      if ('wakeLock' in navigator) s.current.wake = await navigator.wakeLock.request('screen');
    } catch {
      /* không bắt buộc */
    }
  }, []);

  const closeAudio = useCallback(() => {
    const cur = s.current;
    cur.stream?.getTracks().forEach((t) => t.stop());
    void cur.ctx?.close().catch(() => undefined);
    cur.stream = null;
    cur.ctx = null;
  }, []);

  const teardown = useCallback(() => {
    const cur = s.current;
    cur.running = false;
    window.clearTimeout(cur.retry);
    window.clearTimeout(flushTimer.current);
    flushTimer.current = 0;
    pending.current = {};
    if (cur.ws) {
      cur.ws.onclose = null;
      try {
        cur.ws.close();
      } catch {
        /* đã đóng */
      }
    }
    closeAudio();
    void cur.wake?.release().catch(() => undefined);
    s.current = fresh();
  }, [closeAudio]);

  const connect = useCallback(() => {
    const cur = s.current;
    if (!cur.running || !cur.ctx) return;
    const ws = new WebSocket(`wss://${location.host}/ws?k=${encodeURIComponent(TOKEN)}`);
    ws.binaryType = 'arraybuffer';
    cur.ws = ws;
    ws.onopen = () => ws.send(JSON.stringify({ t: 'hello', rate: cur.ctx?.sampleRate }));
    ws.onmessage = (e: MessageEvent<unknown>) => {
      if (typeof e.data !== 'string') return;
      let m: ServerMessage;
      try {
        m = JSON.parse(e.data) as ServerMessage;
      } catch {
        return;
      }
      if (m.t === 'ready') {
        setMessage(null);
        setStatus('live');
      } else if (m.t === 'denied') {
        // Sai mã ghép nối (hoặc PC vừa đổi mã): dừng hẳn, không thử lại
        teardown();
        meter.current = { ...EMPTY_METER };
        setParams(null);
        setPreset(null);
        setStatus('error');
        setMessage({ k: 'err.denied' });
      } else if (m.t === 'ping') {
        // PC đo thời gian khứ hồi: trả lời ngay để con số chính xác
        if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'pong', ts: m.ts }));
      } else if (m.t === 'state') {
        adoptPcLang(m.lang);
        setProfiles(m.profiles ?? []);
        // Bỏ qua bản "dội lại" của chính mình khi đang kéo thanh, để thanh không giật
        if (Date.now() - lastEdit.current > 450) {
          setParams(m.p);
          setPreset(m.preset);
          setProfile(m.profile ?? null);
        }
      } else if (m.t === 'meter') {
        meter.current = {
          in: m.in, out: m.out, gate: m.gate, buf: m.buf, under: m.under, drops: m.drops,
          cal: m.cal, calp: m.calp, caln: m.caln, lat: m.lat,
        };
      }
    };
    ws.onclose = () => {
      if (!s.current.running) return;
      meter.current = { ...EMPTY_METER };
      setStatus('retry');
      s.current.retry = window.setTimeout(connect, 1500);
    };
  }, [teardown]);

  /** Mở micro và chuỗi âm thanh. Dùng lúc bắt đầu và cả khi bật lại sau khi iOS tạm dừng mic. */
  const openAudio = useCallback(async () => {
    closeAudio();
    let ctx: AudioContext;
    try {
      ctx = new AudioContext({ latencyHint: 'interactive', sampleRate: 48000 });
    } catch {
      ctx = new AudioContext({ latencyHint: 'interactive' });
    }
    s.current.ctx = ctx;
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: false, noiseSuppression: false, autoGainControl: false },
    });
    // Người dùng bấm Dừng trong lúc chờ cấp quyền: dọn rồi thôi
    if (s.current.ctx !== ctx) {
      stream.getTracks().forEach((t) => t.stop());
      return;
    }
    s.current.stream = stream;
    await ctx.resume();
    await ctx.audioWorklet.addModule('/phone-worklet.js');

    const src = ctx.createMediaStreamSource(stream);
    const tap = new AudioWorkletNode(ctx, 'tap', {
      channelCount: 1,
      channelCountMode: 'explicit',
      outputChannelCount: [1],
    });
    const sink = ctx.createGain();
    sink.gain.value = 0;
    src.connect(tap).connect(sink).connect(ctx.destination);

    tap.port.onmessage = (e: MessageEvent<TapMessage>) => {
      const ws = s.current.ws;
      if (ws && ws.readyState === WebSocket.OPEN && ws.bufferedAmount < 65536) ws.send(e.data.pcm);
    };

    // Đang nối sẵn với PC: báo tần số lấy mẫu của luồng mới
    const ws = s.current.ws;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'hello', rate: ctx.sampleRate }));
  }, [closeAudio]);

  const failWith = useCallback((err: unknown) => {
    meter.current = { ...EMPTY_METER };
    const e = err as { name?: string; message?: string };
    setStatus('error');
    setMessage(
      e.name === 'NotAllowedError' || e.name === 'SecurityError'
        ? { k: isIos ? 'err.mic' : 'err.mic.generic' }
        : { k: 'err.open', a: [e.message ?? ''] },
    );
  }, []);

  const start = useCallback(async () => {
    if (s.current.running) return;
    if (!navigator.mediaDevices?.getUserMedia) {
      setStatus('error');
      setMessage({ k: 'err.https' });
      return;
    }
    if (!TOKEN) {
      setStatus('error');
      setMessage({ k: 'err.token' });
      return;
    }
    s.current.running = true;
    setStatus('connecting');
    setMessage(null);
    try {
      await openAudio();
      connect();
      void holdScreen();
    } catch (err) {
      teardown();
      failWith(err);
    }
  }, [connect, failWith, holdScreen, openAudio, teardown]);

  const stop = useCallback(() => {
    teardown();
    meter.current = { ...EMPTY_METER };
    setStatus('idle');
    setMessage(null);
    setParams(null);
    setPreset(null);
    setProfile(null);
    setPttState(false);
  }, [teardown]);

  /** Quay lại trang (mở khóa, về từ màn hình chính): mở lại micro và nối lại PC nếu cần. */
  const resume = useCallback(async () => {
    const cur = s.current;
    if (!cur.running || cur.resuming) return;
    void holdScreen();
    const track = cur.stream?.getAudioTracks()[0];
    const audioDead = !track || track.readyState === 'ended' || track.muted || cur.ctx?.state !== 'running';
    const wsDead = !cur.ws || cur.ws.readyState >= WebSocket.CLOSING;
    if (!audioDead && !wsDead) return;

    cur.resuming = true;
    setStatus('connecting');
    setMessage({ k: 'msg.resuming' });
    try {
      if (audioDead) await openAudio();
      if (s.current !== cur || !cur.running) return; // đã bấm Dừng trong lúc chờ
      if (wsDead) {
        window.clearTimeout(cur.retry);
        connect();
      }
    } catch (err) {
      teardown();
      failWith(err);
      setMessage({ k: 'err.resume' });
    } finally {
      cur.resuming = false;
    }
  }, [connect, failWith, holdScreen, openAudio, teardown]);

  /** Đổi một thông số: hiện ngay trên iPhone, gom lại rồi gửi sang PC mỗi ~40 ms. */
  const setParam = useCallback(
    (key: NumKey | BoolKey, value: number | boolean) => {
      lastEdit.current = Date.now();
      setPreset(null);
      setParams((p) => (p ? { ...p, [key]: value } : p));
      pending.current = { ...pending.current, [key]: value };
      if (!flushTimer.current) {
        flushTimer.current = window.setTimeout(() => {
          flushTimer.current = 0;
          send({ t: 'set', p: pending.current });
          pending.current = {};
        }, 40);
      }
    },
    [send],
  );

  const choosePreset = useCallback(
    (name: string) => {
      lastEdit.current = 0; // nhận lại trạng thái đầy đủ từ PC
      send({ t: 'preset', name });
    },
    [send],
  );

  /** Áp một hồ sơ riêng (do người dùng lưu trên PC). */
  const chooseProfile = useCallback(
    (name: string) => {
      lastEdit.current = 0;
      send({ t: 'profile', name });
    },
    [send],
  );

  /** Tắt/bật tiếng ngay, không chờ gom gói: dùng cho nút "giữ để nói". */
  const setMuteNow = useCallback(
    (mute: boolean) => {
      lastEdit.current = Date.now();
      setPreset(null);
      setParams((p) => (p ? { ...p, mute } : p));
      send({ t: 'set', p: { mute } });
    },
    [send],
  );

  /** Bật/tắt chế độ giữ để nói. Bật: mic im cho tới khi giữ nút. Tắt: mic mở lại. */
  const enablePtt = useCallback(
    (on: boolean) => {
      setPttState(on);
      setMuteNow(on);
    },
    [setMuteNow],
  );

  /** Nhờ PC đo nền ồn trong 3 giây im lặng rồi tự đặt ngưỡng cổng. */
  const calibrate = useCallback(() => {
    lastEdit.current = 0;
    send({ t: 'calibrate' });
  }, [send]);

  useEffect(() => {
    const onBack = () => {
      if (document.visibilityState === 'visible') void resume();
    };
    document.addEventListener('visibilitychange', onBack);
    window.addEventListener('pageshow', onBack);
    return () => {
      document.removeEventListener('visibilitychange', onBack);
      window.removeEventListener('pageshow', onBack);
      teardown();
    };
  }, [resume, teardown]);

  return {
    status, message, params, preset, profiles, profile, ptt, meter,
    setParam, choosePreset, chooseProfile, calibrate, enablePtt, setMuteNow, start, stop,
  };
}
