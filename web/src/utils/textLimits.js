export const textInputLimits = Object.freeze({
  accountEmail: 254,
  accountSecret: 4096,
  aiInstruction: 4000,
  aiMessage: 4000,
  aiSessionSearch: 100,
  composeBody: 10000,
  composeSubject: 200,
  contactSearch: 200,
  linkUrl: 2048,
  mailSearch: 200,
  mailServerHost: 253,
  providerApiKey: 4096,
  providerBaseUrl: 2048,
  providerModelName: 256,
  providerName: 96,
  providerSearch: 100,
  recipientAddress: 254,
});

export function textCharacterCount(value) {
  return Array.from(String(value ?? "")).length;
}

export function limitText(value, maximum) {
  const text = String(value ?? "");
  if (textCharacterCount(text) <= maximum) return text;
  return Array.from(text).slice(0, maximum).join("");
}
