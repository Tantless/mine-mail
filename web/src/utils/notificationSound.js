const notificationSoundPatterns = {
  default: [[740, 0, 0.14]],
  mail: [
    [660, 0, 0.13],
    [880, 0.14, 0.18],
  ],
  im: [
    [784, 0, 0.1],
    [1047, 0.11, 0.12],
  ],
  reminder: [
    [523, 0, 0.14],
    [659, 0.16, 0.14],
    [784, 0.32, 0.2],
  ],
};

export function playWebNotificationSound(preset) {
  if (!preset) return;
  const AudioContext = window.AudioContext || window.webkitAudioContext;
  if (!AudioContext) return;
  const pattern = notificationSoundPatterns[preset] || notificationSoundPatterns.mail;
  const context = new AudioContext();
  const startedAt = context.currentTime + 0.02;
  for (const [frequency, offset, duration] of pattern) {
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    oscillator.type = "sine";
    oscillator.frequency.value = frequency;
    gain.gain.setValueAtTime(0.0001, startedAt + offset);
    gain.gain.exponentialRampToValueAtTime(0.13, startedAt + offset + 0.018);
    gain.gain.exponentialRampToValueAtTime(
      0.0001,
      startedAt + offset + duration,
    );
    oscillator.connect(gain).connect(context.destination);
    oscillator.start(startedAt + offset);
    oscillator.stop(startedAt + offset + duration);
  }
  const totalDuration = Math.max(
    ...pattern.map(([, offset, duration]) => offset + duration),
  );
  window.setTimeout(() => void context.close(), (totalDuration + 0.2) * 1000);
}
