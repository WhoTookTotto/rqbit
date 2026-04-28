import { TorrentStats } from "../api-types";

export const getTorrentProgressPercentage = (
  stats: TorrentStats | null | undefined,
): number => {
  if (!stats) {
    return 0;
  }

  if (stats.error || stats.finished) {
    return 100;
  }

  const totalBytes = stats.total_bytes ?? 0;
  const progressBytes = stats.progress_bytes ?? 0;

  if (totalBytes <= 0) {
    return 0;
  }

  const percentage = (progressBytes / totalBytes) * 100;
  return Math.min(100, Math.max(0, percentage));
};