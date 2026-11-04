import { useContext, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import axios from 'axios';
import { InstanceContext } from 'data/InstanceContext';
import { useUserAuthorized } from 'data/UserInfo';
import Button from 'components/Atoms/Button';
import ConfirmDialog from 'components/Atoms/ConfirmDialog';
import DashboardCard from 'components/DashboardCard';
import { ClientError } from 'bindings/ClientError';
import { errorToString } from 'utils/util';
import { toast } from 'react-toastify';

interface InstanceBackup {
  name: string;
  bytes: number;
  modified_ms: number;
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 1024) return `${Math.max(0, Math.round(bytes))} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

async function errorMessage(error: unknown): Promise<string> {
  if (axios.isAxiosError(error) && error.response?.data instanceof Blob) {
    try {
      const text = await error.response.data.text();
      const parsed = JSON.parse(text) as Partial<ClientError>;
      if (parsed.kind || parsed.causes) return new ClientError(parsed).toString();
    } catch {
      return errorToString(error);
    }
  }
  return errorToString(error);
}

export default function InstanceBackups() {
  const { selectedInstance: instance } = useContext(InstanceContext);
  const uuid = instance?.uuid;
  const docker = Boolean(uuid?.startsWith('DOCKER-'));
  const stopped = instance?.state === 'Stopped';
  const canRead = useUserAuthorized('can_read_instance_file', uuid);
  const canWrite = useUserAuthorized('can_write_instance_file', uuid);
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [creating, setCreating] = useState(false);
  const [pending, setPending] = useState<{ action: 'delete' | 'restore'; name: string } | null>(
    null
  );

  const backups = useQuery(
    ['instance', uuid, 'backups'],
    () =>
      axios
        .get<InstanceBackup[]>(`/instance/${uuid}/backups`)
        .then((response) => response.data),
    { enabled: Boolean(uuid) && canRead && !docker }
  );

  if (!instance || !uuid) return null;

  const refresh = async () => {
    await queryClient.invalidateQueries(['instance', uuid, 'backups']);
  };

  const createBackup = async () => {
    setBusy(true);
    setCreating(true);
    try {
      await axios.post(`/instance/${uuid}/backups`, null, { timeout: 0 });
      toast.success('Backup saved');
      await refresh();
    } catch (err) {
      toast.error(await errorMessage(err));
    } finally {
      setCreating(false);
      setBusy(false);
    }
  };

  const downloadBackup = async (name: string) => {
    setBusy(true);
    try {
      const response = await axios.get(`/instance/${uuid}/backups/${encodeURIComponent(name)}`, {
        responseType: 'blob',
        timeout: 0,
      });
      const url = URL.createObjectURL(response.data);
      const link = document.createElement('a');
      link.href = url;
      link.download = name;
      link.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      toast.error(await errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const runPending = async () => {
    if (!pending) return;
    const current = pending;
    setBusy(true);
    try {
      if (current.action === 'delete') {
        await axios.delete(`/instance/${uuid}/backups/${encodeURIComponent(current.name)}`);
        toast.success('Backup deleted');
      } else {
        await axios.post(
          `/instance/${uuid}/backups/${encodeURIComponent(current.name)}/restore`,
          null,
          { timeout: 0 }
        );
        toast.success('Backup restored');
        await queryClient.invalidateQueries(['instances', 'list']);
      }
      await refresh();
    } catch (err) {
      toast.error(await errorMessage(err));
    } finally {
      setBusy(false);
      setPending(null);
    }
  };

  return (
    <DashboardCard>
      {docker ? (
        <p className="text-medium text-white/50">
          Docker instances cannot be backed up here.
        </p>
      ) : !canRead ? (
        <p className="text-medium text-white/50">
          You do not have permission to view backups for this instance.
        </p>
      ) : (
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <p className="text-medium text-white/50">
              {stopped
                ? 'A backup is a zip of this instance. Older zips in the backups folder are left out. The server stays stopped.'
                : 'Stop the server before taking or restoring a backup.'}
            </p>
            <Button
              label="Back up now"
              disabled={!canWrite || !stopped || busy}
              loading={creating}
              onClick={() => {
                void createBackup();
              }}
            />
          </div>
          {backups.isLoading ? (
            <p className="text-medium text-white/50">Loading backups...</p>
          ) : null}
          {backups.error ? (
            <p className="text-medium text-red-200">{errorToString(backups.error)}</p>
          ) : null}
          {backups.data && backups.data.length === 0 ? (
            <p className="text-medium text-white/50">No backups yet.</p>
          ) : null}
          {backups.data?.map((backup) => (
            <div
              key={backup.name}
              className="flex flex-col gap-3 rounded-md border border-gray-faded/30 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="min-w-0">
                <div className="truncate text-medium font-medium text-gray-300">{backup.name}</div>
                <div className="text-small text-white/50">
                  {formatBytes(backup.bytes)} · {new Date(backup.modified_ms).toLocaleString()}
                </div>
              </div>
              <div className="flex flex-row flex-wrap gap-2">
                <Button
                  label="Download"
                  size="slim"
                  disabled={busy}
                  onClick={() => {
                    void downloadBackup(backup.name);
                  }}
                />
                <Button
                  label="Restore"
                  size="slim"
                  disabled={!canWrite || !stopped || busy}
                  onClick={() => setPending({ action: 'restore', name: backup.name })}
                />
                <Button
                  label="Delete"
                  size="slim"
                  intention="danger"
                  disabled={!canWrite || busy}
                  onClick={() => setPending({ action: 'delete', name: backup.name })}
                />
              </div>
            </div>
          ))}
        </div>
      )}
      <ConfirmDialog
        isOpen={pending?.action === 'restore'}
        type="danger"
        title="Restore this backup?"
        confirmButtonText="Restore"
        onClose={() => {
          if (!busy) setPending(null);
        }}
        onConfirm={() => {
          void runPending();
        }}
      >
        This replaces the instance files with {pending?.name}. Backups already stored here are
        kept. The server must stay stopped.
      </ConfirmDialog>
      <ConfirmDialog
        isOpen={pending?.action === 'delete'}
        type="danger"
        title="Delete this backup?"
        confirmButtonText="Delete"
        onClose={() => {
          if (!busy) setPending(null);
        }}
        onConfirm={() => {
          void runPending();
        }}
      >
        {pending?.name} will be removed.
      </ConfirmDialog>
    </DashboardCard>
  );
}
