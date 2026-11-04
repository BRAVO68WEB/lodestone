import { coreWsBase, getSnowflakeTimestamp } from './../utils/util';
import { InstanceEvent } from './../bindings/InstanceEvent';
import { useUserAuthorized } from 'data/UserInfo';
import axios from 'axios';
import { useContext, useEffect, useRef, useState } from 'react';
import { LodestoneContext } from './LodestoneContext';
import { ClientEvent } from 'bindings/ClientEvent';

export type ConsoleStreamStatus =
  | 'no-permission'
  | 'loading'
  | 'buffered'
  | 'live'
  | 'live-no-buffer'
  | 'closed'
  | 'error';

// simplified version of a ClientEvent with just InstanceOutput
export type ConsoleEvent = {
  timestamp: number;
  snowflake: string;
  detail: string;
  uuid: string;
  name: string;
  message: string;
};

function consoleMessage(event: ClientEvent): string | null {
  if (event.event_inner.type !== 'InstanceEvent') return null;
  const inner = event.event_inner.instance_event_inner;
  // A normal log line is also stored as a system message. Drawing both prints it twice.
  if (inner.type === 'InstanceOutput') return inner.message;
  return null;
}

// function to convert a ClientEvent to a ConsoleEvent
const toConsoleEvent = (event: ClientEvent): ConsoleEvent | null => {
  const message = consoleMessage(event);
  if (message === null || event.event_inner.type !== 'InstanceEvent') return null;
  const event_inner: InstanceEvent = event.event_inner;

  return {
    timestamp: getSnowflakeTimestamp(event.snowflake),
    snowflake: event.snowflake,
    detail: event.details,
    uuid: event_inner.instance_uuid,
    name: event_inner.instance_name,
    message: message,
  };
};

function dedupeConsole(events: ConsoleEvent[]): ConsoleEvent[] {
  const seen = new Set<string>();
  const lines: ConsoleEvent[] = [];
  events.forEach((event) => {
    if (seen.has(event.snowflake)) return;
    seen.add(event.snowflake);
    lines.push(event);
  });
  return lines;
}

/**
 * Does two things:
 * 1. calls useEffect to fetch the console stream
 * 2. calls useEffect to open a websocket connection to the server and subscribe to the console stream
 *   the websocket will update the query cache with new console output
 *
 * Note that we don't use the useQuery hook here and we are managing the query cache manually
 *
 * @param uuid the uuid of the instance to subscribe to
 * @return whatever useQuery returns
 */
export const useConsoleStream = (uuid: string) => {
  const { core, token } = useContext(LodestoneContext);

  const [consoleLog, setConsoleLog] = useState<ConsoleEvent[]>([]);
  const olderCount = useRef(0);
  const [status, setStatusInner] = useState<ConsoleStreamStatus>('loading'); //callbacks should use statusRef.current instead of status
  const statusRef = useRef<ConsoleStreamStatus>('loading');
  statusRef.current = status;
  const setStatus = (newStatus: ConsoleStreamStatus) => {
    statusRef.current = newStatus;
    setStatusInner(newStatus);
  };
  const canAccessConsole = useUserAuthorized(
    'can_access_instance_console',
    uuid
  );

  const clearConsoleLog = () => {
    olderCount.current = 0;
    setConsoleLog([]);
  };

  const consoleEventsFrom = (events: ClientEvent[]): ConsoleEvent[] => {
    return events
      .map(toConsoleEvent)
      .filter((event): event is ConsoleEvent => event !== null);
  };

  const mergeConsoleLog = (newLog: ClientEvent[]) => {
    setConsoleLog((oldLog) => {
      const merged = dedupeConsole([...oldLog, ...consoleEventsFrom(newLog)]);
      const max = olderCount.current + 1024;
      if (merged.length <= max) return merged;
      const history = merged.slice(0, olderCount.current);
      const live = merged.slice(merged.length - 1024);
      return dedupeConsole([...history, ...live]);
    });
  };

  const prependConsoleLog = (older: ClientEvent[]) => {
    const incoming = consoleEventsFrom(older);
    setConsoleLog((oldLog) => {
      const fresh = incoming.filter(
        (event) => !oldLog.some((line) => line.snowflake === event.snowflake)
      );
      olderCount.current += fresh.length;
      return dedupeConsole([...fresh, ...oldLog]);
    });
    return incoming.length;
  };

  useEffect(() => {
    if (!canAccessConsole) {
      setStatus('no-permission');
      return;
    }
    setStatus('loading');

    try {
      const websocket = new WebSocket(
        `${coreWsBase(core)}/instance/${uuid}/console/stream?token=Bearer ${token}`
      );

      websocket.onopen = () => {
        if (statusRef.current === 'loading') setStatus('live-no-buffer');
        if (statusRef.current === 'buffered') setStatus('live');
      };

      websocket.onmessage = (messageEvent) => {
        const event: ClientEvent = JSON.parse(messageEvent.data);
        mergeConsoleLog([event]);
      };

      websocket.onclose = (event) => {
        setStatus(event.code === 1000 ? 'closed' : 'error');
        console.log("event: ", event)
      };

      return () => {
        websocket.close();
      };
    } catch (e) {
      console.error(e);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [core, uuid, canAccessConsole, token]);

  useEffect(() => {
    if (!canAccessConsole) return;
    let cancelled = false;
    olderCount.current = 0;
    setConsoleLog([]);
    axios
      .get(`/instance/${uuid}/console/buffer`)
      .then((response) => {
        if (cancelled) return;
        mergeConsoleLog(response.data);
        if (statusRef.current === 'loading') setStatus('buffered');
        if (statusRef.current === 'live-no-buffer') setStatus('live');
      })
      .catch((e) => {
        console.error(e);
      });
    return () => {
      cancelled = true;
    };
    // mergeConsoleLog closes over setters only
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [canAccessConsole, uuid]);

  const loadOlder = async (before: string): Promise<{ added: number; exhausted: boolean }> => {
    const response = await axios.get<{ lines: ClientEvent[]; exhausted: boolean }>(
      `/instance/${uuid}/console`,
      { params: { before, limit: 300 } }
    );
    const added = prependConsoleLog(response.data.lines);
    return { added, exhausted: response.data.exhausted };
  };

  return {
    consoleLog,
    consoleStatus: status,
    clearConsoleLog,
    loadOlder,
  };
};
