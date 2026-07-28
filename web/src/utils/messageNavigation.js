export function messageNavigationKey(target) {
  const id = target?.id;
  if (
    typeof id === "string" &&
    id.trim()
  ) {
    return `message:${id.trim()}`;
  }
  return null;
}
