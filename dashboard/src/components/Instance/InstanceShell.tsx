import { FormEvent, useContext, useEffect, useRef, useState } from 'react';
import axios from 'axios';
import { useDocumentTitle } from 'usehooks-ts';
import { InstanceContext } from 'data/InstanceContext';
import { useUserAuthorized, useUserInfo } from 'data/UserInfo';
import { useGlobalSettings } from 'data/GlobalSettings';
import Button from 'components/Atoms/Button';
import { errorToString } from 'utils/util';

interface ShellOutput {
  stdout: string;
  stderr: string;
  exit_code: number | null;
  truncated: boolean;
}

interface ShellEntry extends ShellOutput {
  command: string;
}

export default function InstanceShell() {
  useDocumentTitle('Shell - Lodestone');
  const { selectedInstance: instance } = useContext(InstanceContext);
  const uuid = instance?.uuid;
  const canWrite = useUserAuthorized('can_write_instance_file', uuid);
  const { data: user } = useUserInfo();
  const { data: settings } = useGlobalSettings();
  const safeBlocked = Boolean(settings?.safe_mode && user && !user.is_owner);
  const [line, setLine] = useState('');
  const [running, setRunning] = useState(false);
  const [entries, setEntries] = useState<ShellEntry[]>([]);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: 'nearest' });
  }, [entries, running]);

  if (!uuid) return null;

  const disabled = running || !user || safeBlocked || !canWrite;

  const run = async (event: FormEvent) => {
    event.preventDefault();
    const command = line.trim();
    if (!command || disabled) return;
    setLine('');
    setRunning(true);
    try {
      const response = await axios.post<ShellOutput>(
        `/instance/${uuid}/shell`,
        { command },
        { timeout: 120000 }
      );
      setEntries((current) => [...current, { command, ...response.data }]);
    } catch (err) {
      setEntries((current) => [
        ...current,
        {
          command,
          stdout: '',
          stderr: errorToString(err),
          exit_code: null,
          truncated: false,
        },
      ]);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <p className="text-medium text-white/50">
        Runs one allowlisted command in this instance folder. Allowed: cat, cp, mv, mkdir, ls, rm,
        zip, unzip, curl, wget. Pipes and shell syntax are rejected.
      </p>
      {safeBlocked && (
        <p className="text-medium text-yellow">
          Safe mode only lets the owner run shell commands.
        </p>
      )}
      {user && !safeBlocked && !canWrite && (
        <p className="text-medium text-yellow">
          You need file write access on this instance to run commands.
        </p>
      )}
      <form onSubmit={run} className="flex flex-col gap-2 sm:flex-row">
        <input
          value={line}
          onChange={(event) => setLine(event.target.value)}
          disabled={disabled}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          placeholder="cat server.properties"
          aria-label="Shell command"
          className="w-full rounded border border-gray-faded/30 bg-gray-900 px-3 py-2 font-mono text-medium text-gray-300 outline-none focus:ring-2 focus:ring-blue-faded/50 disabled:opacity-50"
        />
        <Button
          type="submit"
          label={running ? 'Running' : 'Run'}
          loading={running}
          disabled={disabled || !line.trim()}
          intention="info"
        />
      </form>
      <div className="flex flex-col gap-3">
        {entries.map((entry, index) => (
          <article
            key={`${entry.command}-${index}`}
            className="rounded-lg border border-gray-faded/30 bg-gray-850 p-3"
          >
            <div className="mb-2 flex items-center justify-between gap-2 font-mono text-small">
              <span className="break-all text-gray-300">$ {entry.command}</span>
              <span className="shrink-0 text-white/50">
                {entry.exit_code === null ? 'not run' : `exit ${entry.exit_code}`}
              </span>
            </div>
            {entry.stdout && (
              <pre className="whitespace-pre-wrap break-all font-mono text-small text-gray-300">
                {entry.stdout}
              </pre>
            )}
            {entry.stderr && (
              <pre className="whitespace-pre-wrap break-all font-mono text-small text-red-200">
                {entry.stderr}
              </pre>
            )}
          </article>
        ))}
        <div ref={endRef} />
      </div>
    </div>
  );
}
