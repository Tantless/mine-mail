export function formatMailTime(value) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  const now = new Date();
  const sameDay = date.toDateString() === now.toDateString();
  if (sameDay) {
    return new Intl.DateTimeFormat("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    }).format(date);
  }

  const sameYear = date.getFullYear() === now.getFullYear();
  return new Intl.DateTimeFormat("zh-CN",
    sameYear
      ? { month: "numeric", day: "numeric" }
      : { year: "2-digit", month: "numeric", day: "numeric" },
  ).format(date);
}

export function formatFullDate(value) {
  if (!value) return "时间未知";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}

export function senderLabel(message) {
  return message.sender?.name || message.sender?.email || "未知发件人";
}

export function initials(value = "?") {
  const cleaned = value.trim();
  if (!cleaned) return "?";
  const latinParts = cleaned.split(/\s+/).filter(Boolean);
  if (latinParts.length > 1) {
    return `${latinParts[0][0]}${latinParts.at(-1)[0]}`.toUpperCase();
  }
  return cleaned.slice(0, 2).toUpperCase();
}

export function hasFlag(message, flag) {
  return (message.flags || []).some(
    (value) => value.toLowerCase() === flag.toLowerCase(),
  );
}

export function splitAddresses(value) {
  return value
    .split(/[;,，；]/)
    .map((address) => address.trim())
    .filter(Boolean);
}
