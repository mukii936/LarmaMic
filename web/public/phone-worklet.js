// Chạy trong luồng âm thanh của Safari: float32 -> int16, gom khung 20 ms rồi gửi về trang.
class Tap extends AudioWorkletProcessor {
  constructor() {
    super();
    this.n = Math.round(sampleRate * 0.02);
    this.buf = new Int16Array(this.n);
    this.k = 0;
    this.peak = 0;
  }
  process(inputs) {
    const inp = inputs[0] && inputs[0][0];
    if (!inp) return true;
    for (let i = 0; i < inp.length; i++) {
      let x = inp[i];
      if (x > 1) x = 1; else if (x < -1) x = -1;
      const a = Math.abs(x);
      if (a > this.peak) this.peak = a;
      this.buf[this.k++] = x < 0 ? x * 32768 : x * 32767;
      if (this.k === this.n) {
        const pcm = this.buf.buffer;
        this.port.postMessage({ pcm, peak: this.peak }, [pcm]);
        this.buf = new Int16Array(this.n);
        this.k = 0;
        this.peak = 0;
      }
    }
    return true;
  }
}
registerProcessor('tap', Tap);
