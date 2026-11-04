import { useContext, useEffect, useMemo, useState } from 'react';
import axios from 'axios';
import { InstanceContext } from 'data/InstanceContext';
import { LodestoneContext } from 'data/LodestoneContext';
import { ClientEvent } from 'bindings/ClientEvent';
import { EventLevel } from 'bindings/EventLevel';
import DashboardCard from 'components/DashboardCard';
import { coreWsBase, errorToString, getSnowflakeTimestamp } from 'utils/util';
import { useDocumentTitle } from 'usehooks-ts';

const LEVELS: EventLevel[] = ['Info', 'Warning', 'Error'];
const MONTH_MS = 30 * 24 * 60 * 60 * 1000;

const levelClass: Record<EventLevel, string> = {
  Info: 'text-white/50',
  Warning: 'text-yellow',
  Error: 'text-red-200',
};

type AnyRecord = Record<string, unknown>;

function asRecord(value: unknown): AnyRecord {
  return value && typeof value === 'object' ? (value as AnyRecord) : {};
}

function playerNames(value: unknown): string {
  if (!Array.isArray(value)) return '';
  return value
    .map((player) => {
      const name = asRecord(player).name;
      return typeof name === 'string' ? name : '';
    })
    .filter(Boolean)
    .join(', ');
}

function readableType(type: string): string {
  return type.replace(/([A-Z])/g, ' $1').trim();
}

export function describeEvent(event: ClientEvent): { title: string; details: string } {
  const details = event.details?.trim() ?? '';
  const inner = asRecord(event.event_inner);
  const type = typeof inner.type === 'string' ? inner.type : 'Event';

  if (type === 'InstanceEvent') {
    const name = typeof inner.instance_name === 'string' ? inner.instance_name : 'Instance';
    const body = asRecord(inner.instance_event_inner);
    const bodyType = typeof body.type === 'string' ? body.type : 'Event';
    const message = typeof body.message === 'string' ? body.message : '';
    if (bodyType === 'StateTransition') {
      const to = typeof body.to === 'string' ? body.to : '';
      const sentence: Record<string, string> = {
        Starting: `${name} is starting`,
        Running: `${name} started`,
        Stopping: `${name} is stopping`,
        Stopped: `${name} stopped`,
        Error: `${name} encountered an error`,
      };
      return { title: sentence[to] ?? `${name} changed state to ${to || 'unknown'}`, details };
    }
    if (bodyType === 'PlayerChange') {
      const joined = playerNames(body.players_joined);
      const left = playerNames(body.players_left);
      if (joined && !left) return { title: `${joined} joined ${name}`, details };
      if (left && !joined) return { title: `${left} left ${name}`, details };
      if (joined || left) {
        const parts = [joined && `${joined} joined`, left && `${left} left`].filter(Boolean);
        return { title: parts.join(', '), details };
      }
      return { title: `Players on ${name} changed`, details };
    }
    if (bodyType === 'PlayerMessage') {
      const player = typeof body.player === 'string' ? body.player : 'A player';
      const said = typeof body.player_message === 'string' ? body.player_message : '';
      return { title: said ? `${player} said ${said}` : `${player} said something`, details };
    }
    if (message) return { title: message, details };
    return { title: `${name}: ${readableType(bodyType)}`, details };
  }

  if (type === 'ProgressionEvent') {
    const prog = asRecord(inner.progression_event_inner);
    const progType = typeof prog.type === 'string' ? prog.type : '';
    const message = typeof prog.message === 'string' ? prog.message : '';
    if (progType === 'ProgressionEnd') {
      if (prog.success === false) return { title: message || 'The task failed', details };
      return { title: message || 'Task finished', details };
    }
    if (progType === 'ProgressionStart') {
      const name = typeof prog.progression_name === 'string' ? prog.progression_name : '';
      return { title: name || 'Task started', details };
    }
    const progress = typeof prog.progress_message === 'string' ? prog.progress_message : '';
    return { title: progress || 'Task updated', details };
  }

  if (type === 'MacroEvent') {
    const macro = asRecord(inner.macro_event_inner);
    const macroType = typeof macro.type === 'string' ? macro.type : '';
    const taskName = typeof inner.task_name === 'string' ? inner.task_name : '';
    const named = taskName ? `Macro ${taskName}` : 'Macro';
    if (macroType === 'Stopped') {
      const status = asRecord(macro.exit_status);
      const failed = status.type === 'Error' || status.type === 'Killed';
      const error = typeof status.error_msg === 'string' ? status.error_msg : '';
      return {
        title: failed ? `${named} failed` : `${named} finished`,
        details: error || details,
      };
    }
    if (macroType === 'Started') return { title: `${named} started`, details };
    if (macroType === 'Detach') return { title: `${named} detached`, details };
    return { title: `${named} updated`, details };
  }

  if (type === 'UserEvent') {
    const userEvent = asRecord(inner.user_event_inner);
    const userType = typeof userEvent.type === 'string' ? userEvent.type : 'UserEvent';
    return { title: readableType(userType), details };
  }

  if (type === 'FSEvent') {
    const operation = typeof inner.operation === 'string' ? inner.operation : 'changed';
    const target = asRecord(inner.target);
    const path = typeof target.path === 'string' ? target.path : '';
    return { title: path ? `${operation} ${path}` : `File ${operation}`, details };
  }

  return { title: readableType(type), details };
}

function eventInstanceId(event: ClientEvent): string | null {
  const inner = asRecord(event.event_inner);
  if (inner.type === 'InstanceEvent' || inner.type === 'MacroEvent') {
    return typeof inner.instance_uuid === 'string' ? inner.instance_uuid : null;
  }
  if (inner.type !== 'ProgressionEvent') return null;
  const prog = asRecord(inner.progression_event_inner);
  const nested = asRecord(prog.inner);
  if (typeof nested.instance_uuid === 'string') return nested.instance_uuid;
  if (typeof nested.uuid === 'string') return nested.uuid;
  return null;
}

function isConsoleLine(event: ClientEvent): boolean {
  const inner = asRecord(event.event_inner);
  if (inner.type !== 'InstanceEvent') return false;
  const body = asRecord(inner.instance_event_inner).type;
  return body === 'InstanceOutput' || body === 'PlayerMessage' || body === 'SystemMessage';
}

function formatWhen(snowflake: string): string {
  try {
    const ms = getSnowflakeTimestamp(snowflake);
    if (!Number.isFinite(ms)) return '';
    return new Date(ms).toLocaleString();
  } catch {
    return '';
  }
}

export default function InstanceEventLog() {
  useDocumentTitle('Event Logs - Lodestone');
  const { selectedInstance: instance } = useContext(InstanceContext);
  const { token, core } = useContext(LodestoneContext);
  const uuid = instance?.uuid;
  const [events, setEvents] = useState<ClientEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [levels, setLevels] = useState<Record<EventLevel, boolean>>({
    Info: true,
    Warning: true,
    Error: true,
  });

  useEffect(() => {
    if (!uuid || !token) return;
    let closed = false;
    setLoading(true);
    setError('');
    setEvents([]);

    const filter = {
      bearer_token: token,
      event_levels: null,
      event_types: null,
      instance_event_types: null,
      user_event_types: null,
      event_user_ids: null,
      event_instance_ids: [uuid],
      time_range: {
        start: Date.now() - MONTH_MS,
        end: Date.now() + 60_000,
      },
      exclude_console: true,
    };
    const encoded = encodeURIComponent(JSON.stringify(filter));

    axios
      .get<ClientEvent[]>(`/events/search?filter=${encoded}`)
      .then((response) => {
        if (closed) return;
        setEvents(Array.isArray(response.data) ? response.data : []);
      })
      .catch((err) => {
        if (closed) return;
        setError(errorToString(err));
      })
      .finally(() => {
        if (!closed) setLoading(false);
      });

    const ws = new WebSocket(
      `${coreWsBase(core)}/events/${uuid}/stream?filter=${encoded}`
    );
    ws.onmessage = (message) => {
      if (closed) return;
      let event: ClientEvent;
      try {
        event = JSON.parse(String(message.data)) as ClientEvent;
      } catch {
        return;
      }
      if (!event?.snowflake || isConsoleLine(event)) return;
      if (eventInstanceId(event) !== uuid) return;
      setEvents((current) => {
        if (current.some((item) => item.snowflake === event.snowflake)) return current;
        return [event, ...current];
      });
    };

    return () => {
      closed = true;
      ws.close();
    };
  }, [uuid, token, core]);

  const visible = useMemo(() => {
    return events
      .filter((event) => levels[event.level] && eventInstanceId(event) === uuid)
      .sort((a, b) => {
        try {
          const left = BigInt(a.snowflake);
          const right = BigInt(b.snowflake);
          if (left === right) return 0;
          return left > right ? -1 : 1;
        } catch {
          return 0;
        }
      });
  }, [events, levels, uuid]);

  if (!uuid) return null;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap gap-2">
        {LEVELS.map((level) => (
          <button
            key={level}
            type="button"
            aria-pressed={levels[level]}
            className={`rounded-full border px-3 py-1 text-small font-medium ${
              levels[level]
                ? 'border-white/40 bg-gray-700 text-gray-300'
                : 'border-gray-faded/30 bg-gray-850 text-white/40'
            }`}
            onClick={() => setLevels((current) => ({ ...current, [level]: !current[level] }))}
          >
            {level}
          </button>
        ))}
      </div>
      {loading && <p className="text-medium text-white/50">Loading events…</p>}
      {error && <p className="text-medium text-red-200">{error}</p>}
      {!loading && !error && visible.length === 0 && (
        <DashboardCard className="items-center justify-center gap-2">
          <p className="text-center font-medium text-white/50">
            {events.length === 0
              ? 'No events for this instance yet.'
              : 'No events match these filters.'}
          </p>
        </DashboardCard>
      )}
      <div className="flex flex-col gap-2">
        {visible.map((event) => {
          const card = describeEvent(event);
          const showDetails = card.details && card.details !== card.title;
          return (
            <article
              key={event.snowflake}
              className="flex flex-col gap-1 rounded-lg border border-gray-faded/30 bg-gray-850 px-4 py-3"
            >
              <div className="flex items-start justify-between gap-3">
                <h3 className="text-medium font-medium text-gray-300">{card.title}</h3>
                <time className="shrink-0 text-small text-white/50">
                  {formatWhen(event.snowflake)}
                </time>
              </div>
              <p className={`text-small font-medium uppercase tracking-wide ${levelClass[event.level]}`}>
                {event.level}
              </p>
              {showDetails && (
                <p className="whitespace-pre-wrap break-words text-small text-white/70">
                  {card.details}
                </p>
              )}
            </article>
          );
        })}
      </div>
    </div>
  );
}
