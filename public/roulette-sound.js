/**
 * What the wheel sounds like.
 *
 * Two ingredients, both built from one second of noise: a filtered rush that
 * follows the ball round the track, and a click for every impact the plan
 * recorded. The clicks are the point -- a bounce you can hear is a bounce you
 * believe -- so they are pitched by what was struck and by how hard.
 *
 * Nothing is created until the first spin, which is a click on a button, so
 * there is no autoplay to be blocked and no audio graph on a page nobody spun.
 */

const VOICES = {
  // frequency, Q, decay, gain -- a diamond is a low knock, a fret a bright tick.
  deflector: [1150, 4, 0.1, 0.5],
  fret: [2500, 7, 0.045, 0.28],
  bowl: [700, 3, 0.08, 0.2],
};

export function createRouletteSound() {
  let audio = null;
  let master = null;
  let noise = null;
  let roll = null;
  let rollGain = null;
  let rollFilter = null;
  let muted = false;

  function ensure() {
    if (audio || muted) return audio;
    const Ctor = window.AudioContext || window.webkitAudioContext;
    if (!Ctor) return null;
    try {
      audio = new Ctor();
      master = audio.createGain();
      master.gain.value = 0.55;
      master.connect(audio.destination);
      noise = audio.createBuffer(1, audio.sampleRate, audio.sampleRate);
      const data = noise.getChannelData(0);
      for (let i = 0; i < data.length; i++) data[i] = Math.random() * 2 - 1;
    } catch {
      audio = null;
    }
    return audio;
  }

  function startRoll() {
    if (!audio || roll) return;
    roll = audio.createBufferSource();
    roll.buffer = noise;
    roll.loop = true;
    rollFilter = audio.createBiquadFilter();
    rollFilter.type = "bandpass";
    rollFilter.Q.value = 1.4;
    rollGain = audio.createGain();
    rollGain.gain.value = 0;
    roll.connect(rollFilter).connect(rollGain).connect(master);
    roll.start();
  }

  return {
    get muted() {
      return muted;
    },
    setMuted(next) {
      muted = next;
      if (muted && rollGain) rollGain.gain.value = 0;
    },
    /** Called on the spin click, which is the gesture the browser wants. */
    begin() {
      if (!ensure()) return;
      audio.resume?.().catch(() => {});
      startRoll();
    },
    /** The rush of a ball on wood, tracking its speed in metres per second. */
    roll(speed) {
      if (!audio || !rollGain || muted) return;
      const level = Math.min(1, Math.max(0, speed / 5));
      rollFilter.frequency.value = 240 + 1600 * level;
      rollGain.gain.setTargetAtTime(0.07 * level * level, audio.currentTime, 0.04);
    },
    strike(kind, strength) {
      if (!audio || muted) return;
      const [frequency, q, decay, gain] = VOICES[kind] || VOICES.fret;
      const at = audio.currentTime;
      const source = audio.createBufferSource();
      source.buffer = noise;
      // Start somewhere random in the buffer so repeated clicks are not clones.
      const filter = audio.createBiquadFilter();
      filter.type = "bandpass";
      filter.frequency.value = frequency * (0.85 + Math.random() * 0.3);
      filter.Q.value = q;
      const envelope = audio.createGain();
      envelope.gain.setValueAtTime(gain * (0.35 + 0.65 * strength), at);
      envelope.gain.exponentialRampToValueAtTime(0.0001, at + decay);
      source.connect(filter).connect(envelope).connect(master);
      source.start(at, Math.random() * 0.5, decay + 0.02);
    },
    stop() {
      if (rollGain) rollGain.gain.setTargetAtTime(0, audio.currentTime, 0.08);
    },
  };
}
