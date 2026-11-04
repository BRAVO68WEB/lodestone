import { QueryClient, useQueryClient } from '@tanstack/react-query';
import axios from 'axios';
import { InstanceInfo } from 'bindings/InstanceInfo';
import { InstanceState } from 'bindings/InstanceState';
import { updateInstance } from 'data/InstanceList';
import { useEffect } from 'react';

const listeners = new Set<() => void>();

/** Ask every mounted dashboard to refresh instance states once. */
export function requestInstanceStateSync() {
  listeners.forEach((listener) => listener());
}

async function syncInstanceStates(queryClient: QueryClient) {
  const list = queryClient.getQueryData<{ [uuid: string]: InstanceInfo }>([
    'instances',
    'list',
  ]);
  if (!list) return;
  await Promise.all(
    Object.values(list).map(async (instance) => {
      const seen = instance.state;
      try {
        const response = await axios.get<InstanceState>(
          `/instance/${instance.uuid}/state`
        );
        const state = response.data;
        if (!state || state === seen) return;
        updateInstance(instance.uuid, queryClient, (old) => {
          if (old.state !== seen) return old;
          return { ...old, state };
        });
      } catch {
        // One instance failing should not stop the rest of the pass.
      }
    })
  );
}

/** While the dashboard is open and the tab is visible, catch state up every 5 seconds. */
export function useInstanceStateSync(enabled: boolean) {
  const queryClient = useQueryClient();
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const run = () => {
      if (cancelled) return;
      if (
        typeof document !== 'undefined' &&
        document.visibilityState === 'hidden'
      ) {
        return;
      }
      void syncInstanceStates(queryClient);
    };
    listeners.add(run);
    run();
    const interval = window.setInterval(run, 5000);
    const onVisible = () => {
      if (document.visibilityState === 'visible') run();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      cancelled = true;
      listeners.delete(run);
      window.clearInterval(interval);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [enabled, queryClient]);
}
