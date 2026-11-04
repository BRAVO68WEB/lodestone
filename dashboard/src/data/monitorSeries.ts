export const monitorHistory = 1800;

export const monitorWindows = [
  { seconds: 60, label: '1 min' },
  { seconds: 600, label: '10 min' },
  { seconds: 1800, label: '30 min' },
] as const;

export function formatSpan(seconds: number): string {
  if (seconds >= 60 && seconds % 60 === 0) return `${seconds / 60} min`;
  if (seconds >= 60) return `${Math.round(seconds / 60)} min`;
  return `${seconds}s`;
}

/** The slice ending `endOffset` samples before the newest point, padded on the left. */
export function visibleSeries(
  series: number[],
  windowSize: number,
  endOffset: number
): number[] {
  const size = Math.max(1, Math.floor(windowSize));
  const maxOffset = Math.max(0, series.length - size);
  const offset = Math.min(Math.max(0, Math.floor(endOffset)), maxOffset);
  const end = series.length - offset;
  const start = Math.max(0, end - size);
  const slice = series.slice(start, end);
  const pad = Array(Math.max(0, size - slice.length)).fill(NaN);
  return pad.concat(slice);
}

export function seriesAverage(series: number[]): number | null {
  const finite = series.filter((value) => Number.isFinite(value));
  if (finite.length === 0) return null;
  return finite.reduce((sum, value) => sum + value, 0) / finite.length;
}

/** Dragging left increases the offset so older samples move into view. */
export function dragSamples(
  startX: number,
  currentX: number,
  windowSize: number,
  chartWidth: number
): number {
  const width = Math.max(chartWidth, 1);
  return Math.round(((startX - currentX) / width) * Math.max(1, windowSize));
}

export function clampOffset(
  offset: number,
  seriesLength: number,
  windowSize: number
): number {
  const maxOffset = Math.max(0, seriesLength - Math.max(1, windowSize));
  return Math.min(Math.max(0, offset), maxOffset);
}

export interface UsageSample {
  cpu_usage: number | null;
  memory_usage: number | null;
}

/** Sum of the latest sample from each instance. Null means nothing has reported. */
export function sumSamples(samples: UsageSample[]): {
  cpu: number | null;
  memory: number | null;
  reporting: number;
} {
  let cpu = 0;
  let memory = 0;
  let cpuCount = 0;
  let memoryCount = 0;
  for (const sample of samples) {
    if (sample.cpu_usage != null && !Number.isNaN(sample.cpu_usage)) {
      cpu += sample.cpu_usage;
      cpuCount += 1;
    }
    if (sample.memory_usage != null && !Number.isNaN(sample.memory_usage)) {
      memory += sample.memory_usage;
      memoryCount += 1;
    }
  }
  return {
    cpu: cpuCount > 0 ? cpu : null,
    memory: memoryCount > 0 ? memory : null,
    reporting: Math.max(cpuCount, memoryCount),
  };
}
