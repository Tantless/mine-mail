export function messageNavigationKey(target) {
  const mailbox = target?.mailbox?.trim().toLocaleLowerCase();
  const uid = Number(target?.uid);
  if (!mailbox || !Number.isInteger(uid) || uid <= 0) return null;
  return `${mailbox}:${uid}`;
}
