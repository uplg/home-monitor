/**
 * Format a minute count as a compact human duration:
 * "45 min", "1 h", "1 h 30", "2 j 4 h".
 */
export function formatMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes} min`;
  if (minutes < 1440) {
    const hours = Math.floor(minutes / 60);
    const remainder = minutes % 60;
    return remainder === 0 ? `${hours} h` : `${hours} h ${String(remainder).padStart(2, "0")}`;
  }
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  return hours === 0 ? `${days} j` : `${days} j ${hours} h`;
}
