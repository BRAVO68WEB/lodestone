import { FormEvent, useContext, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import axios from 'axios';
import { InstanceContext } from 'data/InstanceContext';
import { useUserAuthorized } from 'data/UserInfo';
import { Game } from 'bindings/Game';
import Button from 'components/Atoms/Button';
import ConfirmDialog from 'components/Atoms/ConfirmDialog';
import DashboardCard from 'components/DashboardCard';
import { errorToString } from 'utils/util';
import { toast } from 'react-toastify';

interface ModProject {
  project_id: string;
  title: string;
  description: string;
}

interface ModJar {
  name: string;
  bytes: number;
}

function loaderOf(game: Game | undefined): 'fabric' | 'forge' | 'other' | null {
  if (!game || game.type !== 'MinecraftJava') return null;
  if (game.variant.type === 'Fabric') return 'fabric';
  if (game.variant.type === 'Forge') return 'forge';
  return 'other';
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1024) return `${Math.max(0, Math.round(bytes))} B`;
  const units = ['KB', 'MB', 'GB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

export default function InstanceMods() {
  const { selectedInstance: instance } = useContext(InstanceContext);
  const uuid = instance?.uuid;
  const loader = loaderOf(instance?.game_type);
  const supported = loader === 'fabric' || loader === 'forge';
  const stopped = instance?.state === 'Stopped';
  const canRead = useUserAuthorized('can_read_instance_file', uuid);
  const canWrite = useUserAuthorized('can_write_instance_file', uuid);
  const queryClient = useQueryClient();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<ModProject[]>([]);
  const [searching, setSearching] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<
    { action: 'install'; project: ModProject } | { action: 'remove'; name: string } | null
  >(null);

  const installed = useQuery(
    ['instance', uuid, 'mods'],
    () => axios.get<ModJar[]>(`/instance/${uuid}/mods`).then((response) => response.data),
    { enabled: Boolean(uuid) && canRead && supported }
  );

  if (!instance || !uuid) return null;

  const refresh = async () => {
    await queryClient.invalidateQueries(['instance', uuid, 'mods']);
  };

  const search = async (event?: FormEvent) => {
    event?.preventDefault();
    setSearching(true);
    try {
      const response = await axios.get<ModProject[]>(`/instance/${uuid}/mods/search`, {
        params: { q: query.trim() },
      });
      setResults(response.data);
    } catch (err) {
      toast.error(errorToString(err));
    } finally {
      setSearching(false);
    }
  };

  const runPending = async () => {
    if (!pending) return;
    const current = pending;
    setBusy(true);
    try {
      if (current.action === 'install') {
        await axios.post(
          `/instance/${uuid}/mods`,
          { project_id: current.project.project_id },
          { timeout: 0 }
        );
        toast.success(`Installed ${current.project.title}`);
      } else {
        await axios.delete(`/instance/${uuid}/mods/${encodeURIComponent(current.name)}`);
        toast.success('Mod removed');
      }
      await refresh();
    } catch (err) {
      toast.error(errorToString(err));
    } finally {
      setBusy(false);
      setPending(null);
    }
  };

  return (
    <DashboardCard>
      {loader === null || loader === 'other' ? (
        <p className="text-medium text-white/50">
          {loader === null
            ? 'This instance has no mods folder.'
            : 'This loader does not use a mods folder.'}
        </p>
      ) : !canRead ? (
        <p className="text-medium text-white/50">You do not have permission to view mods.</p>
      ) : (
        <div className="flex flex-col gap-6">
          <form className="flex flex-col gap-3 sm:flex-row" onSubmit={(event) => void search(event)}>
            <input
              className="grow rounded border border-gray-faded/30 bg-gray-850 px-2 py-1 text-medium text-gray-300"
              value={query}
              placeholder={`Search Modrinth for ${loader}`}
              disabled={searching || busy}
              onChange={(event) => setQuery(event.target.value)}
            />
            <Button
              label={searching ? 'Searching...' : 'Search'}
              disabled={searching || busy || query.trim() === ''}
              onClick={() => void search()}
            />
          </form>
          <p className="text-medium text-white/50">
            {stopped
              ? 'Install puts the project and its required dependencies into mods. Optional dependencies are left out.'
              : 'Stop the server before installing or removing a mod.'}
          </p>
          {results.map((project) => (
            <div
              key={project.project_id}
              className="flex flex-col gap-2 rounded-md border border-gray-faded/30 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="min-w-0">
                <div className="truncate text-medium font-medium text-gray-300">{project.title}</div>
                <div className="text-small text-white/50">{project.description}</div>
              </div>
              <Button
                label="Install"
                size="slim"
                disabled={!canWrite || !stopped || busy}
                onClick={() => setPending({ action: 'install', project })}
              />
            </div>
          ))}
          <div className="flex flex-col gap-2">
            <h2 className="text-h3 font-bold tracking-medium text-gray-300">Installed jars</h2>
            {installed.isLoading ? (
              <p className="text-medium text-white/50">Loading mods...</p>
            ) : null}
            {installed.error ? (
              <p className="text-medium text-red-200">{errorToString(installed.error)}</p>
            ) : null}
            {installed.data && installed.data.length === 0 ? (
              <p className="text-medium text-white/50">No jars in mods yet.</p>
            ) : null}
            {installed.data?.map((mod) => (
              <div
                key={mod.name}
                className="flex flex-col gap-2 rounded-md border border-gray-faded/30 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0">
                  <div className="truncate text-medium font-medium text-gray-300">{mod.name}</div>
                  <div className="text-small text-white/50">{formatBytes(mod.bytes)}</div>
                </div>
                <Button
                  label="Remove"
                  size="slim"
                  intention="danger"
                  disabled={!canWrite || !stopped || busy}
                  onClick={() => setPending({ action: 'remove', name: mod.name })}
                />
              </div>
            ))}
          </div>
        </div>
      )}
      <ConfirmDialog
        isOpen={pending?.action === 'install'}
        type="info"
        title="Install this mod?"
        confirmButtonText="Install"
        onClose={() => {
          if (!busy) setPending(null);
        }}
        onConfirm={() => void runPending()}
      >
        {pending?.action === 'install' ? pending.project.title : 'This mod'} and its required
        dependencies will be downloaded into mods. Optional dependencies are not installed.
      </ConfirmDialog>
      <ConfirmDialog
        isOpen={pending?.action === 'remove'}
        type="danger"
        title="Remove this jar?"
        confirmButtonText="Remove"
        onClose={() => {
          if (!busy) setPending(null);
        }}
        onConfirm={() => void runPending()}
      >
        {pending?.action === 'remove' ? pending.name : 'This jar'} is the only file that will be
        deleted.
      </ConfirmDialog>
    </DashboardCard>
  );
}
