import DashboardCard from 'components/DashboardCard';
import Button from 'components/Atoms/Button';
import PerformanceGraph from 'components/Graphs/PerformanceGraph';
import { InstanceContext } from 'data/InstanceContext';
import { usePerformanceStream } from 'data/PerformanceStream';
import { useCoreInfo } from 'data/SystemInfo';
import {
  clampOffset,
  dragSamples,
  formatSpan,
  monitorWindows,
  seriesAverage,
  visibleSeries,
} from 'data/monitorSeries';
import { useContext, useRef, useState } from 'react';
import { round } from 'utils/util';

const bytesInGigabyte = 1024 * 1024 * 1024;

export function formatCpu(value: number | null | undefined): string {
  if (value == null || Number.isNaN(value)) return 'unavailable';
  return `${value.toFixed(1)}%`;
}

export function formatMemoryGiB(bytes: number | bigint | null | undefined): string {
  if (bytes == null) return 'unavailable';
  const amount = Number(bytes);
  if (Number.isNaN(amount)) return 'unavailable';
  return `${(amount / bytesInGigabyte).toFixed(1)} GiB`;
}

export default function InstancePerformance() {
  const { selectedInstance: instance } = useContext(InstanceContext);
  const uuid = instance?.uuid ?? '';
  const {
    buffer: performanceBuffer,
    counter,
  } = usePerformanceStream(uuid);
  const { data } = useCoreInfo();
  const [windowSeconds, setWindowSeconds] = useState(60);
  const [endOffset, setEndOffset] = useState(0);
  const drag = useRef<{ x: number; offset: number; width: number } | null>(null);
  if (!instance) return null;
  const total_ram = data?.total_ram ?? 32;
  const latest = performanceBuffer[performanceBuffer.length - 1];
  const cpuSeries = performanceBuffer.map((report) =>
    report.cpu_usage !== null ? round(report.cpu_usage, 1) : NaN
  );
  const memorySeries = performanceBuffer.map((report) =>
    report.memory_usage !== null
      ? round(Number(report.memory_usage) / bytesInGigabyte, 1)
      : NaN
  );
  const offset = clampOffset(endOffset, cpuSeries.length, windowSeconds);
  const visibleCpu = visibleSeries(cpuSeries, windowSeconds, offset);
  const visibleMemory = visibleSeries(memorySeries, windowSeconds, offset);
  const cpuAverage = seriesAverage(visibleCpu);
  const memoryAverage = seriesAverage(visibleMemory);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      x: event.clientX,
      offset,
      width: event.currentTarget.clientWidth,
    };
  };
  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (!start) return;
    setEndOffset(
      clampOffset(
        start.offset + dragSamples(start.x, event.clientX, windowSeconds, start.width),
        cpuSeries.length,
        windowSeconds
      )
    );
  };
  const onPointerUp = () => {
    drag.current = null;
  };

  return (
    <div>
      <h2 className="text-h2 font-extrabold tracking-medium">
        Instance Monitors
      </h2>
      <h3 className="text-h3 font-medium italic tracking-medium text-white/50">
        Instance’s usage of your Lodestone Core. Core keeps about 30 minutes.
        Drag a chart to move through it. The orange line is the average of the
        samples in view.
      </h3>
      <div className="mt-3 flex flex-row flex-wrap items-center gap-2">
        {monitorWindows.map((item) => (
          <Button
            key={item.seconds}
            size="slim"
            label={item.label}
            intention={windowSeconds === item.seconds ? 'primary' : 'info'}
            onClick={() => {
              setWindowSeconds(item.seconds);
              setEndOffset(0);
            }}
          />
        ))}
        <span className="text-small text-white/50">
          {offset === 0 ? 'Latest' : `${formatSpan(offset)} earlier`}
        </span>
        {offset > 0 && (
          <Button
            size="slim"
            label="Back to latest"
            intention="info"
            onClick={() => setEndOffset(0)}
          />
        )}
      </div>
      <div
        className="mt-4 grid cursor-grab select-none grid-cols-1 gap-10 active:cursor-grabbing @3xl:grid-cols-2"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      >
        <div>
          <h3 className="mb-2 text-small font-mediumbold text-gray-faded/50">
            CPU LOAD: {formatCpu(latest?.cpu_usage)} · avg {formatCpu(cpuAverage)}
          </h3>

          <DashboardCard className="border-0 bg-gray-900">
            <PerformanceGraph
              title="CPU Usage"
              color="#62DD76"
              backgroundColor="#61AE3240"
              data={visibleCpu}
              max={100}
              counter={counter}
              timeWindow_s={windowSeconds}
              unit="%"
              average={cpuAverage}
            />
          </DashboardCard>
        </div>
        <div>
          <h3 className="mb-2 text-small font-mediumbold text-gray-faded/50">
            MEMORY USED: {formatMemoryGiB(latest?.memory_usage)} · avg{' '}
            {formatMemoryGiB(memoryAverage == null ? null : memoryAverage * bytesInGigabyte)}
          </h3>

          <DashboardCard className="border-0 bg-gray-900">
            <PerformanceGraph
              title="Memory Usage"
              color="#62DD76"
              backgroundColor="#61AE3240"
              data={visibleMemory}
              max={round(total_ram / bytesInGigabyte, 1)}
              counter={counter}
              timeWindow_s={windowSeconds}
              unit="GiB"
              average={memoryAverage}
            />
          </DashboardCard>
        </div>
      </div>
    </div>
  );
}
