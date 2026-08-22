const notificationSoundFiles = {
  minimal: "minimal.wav",
  melody: "melody.wav",
  gentle: "gentle.wav",
  double_chime: "double-chime.wav",
  waterdrop: "waterdrop.wav",
  bubble: "bubble.wav",
};

const notificationAudio = new Map();

function audioForPreset(preset) {
  const normalizedPreset = notificationSoundFiles[preset] ? preset : "minimal";
  if (!notificationAudio.has(normalizedPreset)) {
    const audio = new Audio(
      `/sounds/notifications/${notificationSoundFiles[normalizedPreset]}`,
    );
    audio.preload = "auto";
    notificationAudio.set(normalizedPreset, audio);
  }
  return notificationAudio.get(normalizedPreset);
}

export async function playWebNotificationSound(preset) {
  if (!preset || typeof Audio === "undefined") return;
  const audio = audioForPreset(preset);
  audio.currentTime = 0;
  await audio.play();
}
