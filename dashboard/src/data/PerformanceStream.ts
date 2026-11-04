import { useContext, useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import axios from 'axios';
import { LodestoneContext } from './LodestoneContext';
import { PerformanceReport } from 'bindings/PerformanceReport';
import { useInterval } from 'usehooks-ts';
import { coreWsBase } from 'utils/util';
import { monitorHistory } from 'data/monitorSeries';

export interface MonitorSample {
  uuid: string;
  cpu_usage: number | null;
  memory_usage: number | null;
}

export const useMonitorLatest = (enabled = true) =>
  useQuery<MonitorSample[]>(
    ['monitor', 'latest'],
    () => axios.get<MonitorSample[]>('/monitor/latest').then((response) => response.data),
    { enabled, refetchInterval: enabled ? 5000 : false }
  );

/**
 * A hook to get the performance monitor information of a specific instance.
 * The backend replays the stored samples, up to about 30 minutes, then sends one per second.
 *
 * @param uuid the uuid of the instance to subscribe to
 * @return whatever useQuery returns
 */
export const usePerformanceStream = (uuid: string) => {
  // assuming the backend pings every 1 second, we can buffer 60 seconds of events
  const [buffer, setBuffer] = useState<PerformanceReport[]>([]);
  const [lastPing, setLastPing] = useState(Date.now());
  const [latency_s, setLatency_s] = useState(0);
  const [counter, setCounter] = useState(-1);
  const { core } = useContext(LodestoneContext);
  const pending = useRef<PerformanceReport[]>([]);
  useInterval(() => {
    setLatency_s((Date.now() - lastPing) / 1000);
  }, 1000);

  useEffect(() => {
    if (!uuid) return;
    pending.current = [];
    try {
      const websocket = new WebSocket(
        `${coreWsBase(core)}/monitor/${uuid}`
      );

      websocket.onmessage = (messageEvent) => {
        try {
          pending.current.push(JSON.parse(messageEvent.data) as PerformanceReport);
        } catch {
          return;
        }
      };

      const flush = window.setInterval(() => {
        if (pending.current.length === 0) return;
        const batch = pending.current;
        pending.current = [];
        setBuffer((oldBuffer) => [...oldBuffer, ...batch].slice(-monitorHistory));
        setCounter((oldCounter) => oldCounter + batch.length);
        setLastPing(Date.now());
      }, 200);

      return () => {
        window.clearInterval(flush);
        websocket.close();
      };
    } catch (e) {
      console.error(e);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [core, uuid]);

  return {
    buffer,
    counter,
    latency_s,
  };
};
