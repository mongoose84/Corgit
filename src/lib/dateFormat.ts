/**
 * `dd-MM-yyyy HH:mm:ss`, always absolute, always local time, never a
 * locale-dependent formatter — the column is fixed-width and right-aligned,
 * so a locale swap must never reflow it (SPEC.md §5.3).
 */
export function formatCommitDate(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');

  const day = pad(date.getDate());
  const month = pad(date.getMonth() + 1);
  const year = date.getFullYear();
  const hours = pad(date.getHours());
  const minutes = pad(date.getMinutes());
  const seconds = pad(date.getSeconds());

  return `${day}-${month}-${year} ${hours}:${minutes}:${seconds}`;
}
