import { useContext, useEffect, useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import axios, { AxiosError } from 'axios';
import { InstanceContext } from 'data/InstanceContext';
import { useUserAuthorized } from 'data/UserInfo';
import { Game } from 'bindings/Game';
import { HandlerGameType } from 'bindings/HandlerGameType';
import { MinecraftVariant } from 'bindings/MinecraftVariant';
import { SetupManifest } from 'bindings/SetupManifest';
import Button from 'components/Atoms/Button';
import ConfirmDialog from 'components/Atoms/ConfirmDialog';
import { errorToString } from 'utils/util';
import { toast } from 'react-toastify';

function minecraftVariant(game: Game | undefined): MinecraftVariant['type'] | null {
  if (!game || game.type !== 'MinecraftJava') return null;
  return game.variant.type;
}

function manifestGame(variant: MinecraftVariant['type'] | null): HandlerGameType | null {
  switch (variant) {
    case 'Vanilla':
      return 'MinecraftJavaVanilla';
    case 'Fabric':
      return 'MinecraftFabric';
    case 'Paper':
      return 'MinecraftPaper';
    case 'Forge':
      return 'MinecraftForge';
    default:
      return null;
  }
}

function versionOptions(manifest: SetupManifest | undefined): string[] {
  const setting = manifest?.setting_sections?.section_1?.settings?.version;
  if (!setting || setting.value_type.type !== 'Enum') return [];
  return setting.value_type.options;
}

export default function InstanceLoaderUpdate() {
  const { selectedInstance: instance } = useContext(InstanceContext);
  const uuid = instance?.uuid;
  const variant = minecraftVariant(instance?.game_type);
  const gameType = manifestGame(variant);
  const stopped = instance?.state === 'Stopped';
  const canUpdate = useUserAuthorized('can_access_instance_setting', uuid);
  const queryClient = useQueryClient();
  const [version, setVersion] = useState(instance?.version ?? '');
  const [loaderVersion, setLoaderVersion] = useState('');
  const [installerVersion, setInstallerVersion] = useState('');
  const [build, setBuild] = useState('');
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);

  useEffect(() => {
    setVersion(instance?.version ?? '');
    setLoaderVersion('');
    setInstallerVersion('');
    setBuild('');
  }, [instance?.uuid, instance?.version]);

  const versions = useQuery<SetupManifest, AxiosError>(
    ['setup_manifest', gameType],
    async () => {
      const response = await axios.get<SetupManifest>(`/setup_manifest/${gameType}`);
      return response.data;
    },
    { enabled: Boolean(gameType) }
  );

  const options = useMemo(() => {
    const listed = versionOptions(versions.data);
    if (!listed.length) return [];
    if (version && !listed.includes(version)) return [version, ...listed];
    return listed;
  }, [versions.data, version]);

  if (!instance || !uuid || !variant) return null;

  const apply = async () => {
    setBusy(true);
    try {
      await axios.put(
        `/instance/${uuid}/loader`,
        {
          version: version.trim(),
          loader_version: loaderVersion.trim() || null,
          installer_version: installerVersion.trim() || null,
          build: build.trim() || null,
        },
        { timeout: 0 }
      );
      toast.success('Server jar updated');
      await queryClient.invalidateQueries(['instances', 'list']);
    } catch (err) {
      toast.error(errorToString(err));
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };

  return (
    <div className="mb-16 flex flex-col gap-4">
      <div className="w-full shrink-0">
        <h2 className="text-h2 font-extrabold tracking-medium">Server jar</h2>
        <h3 className="text-h3 font-medium italic tracking-medium text-white/50">
          {variant === 'Spigot' || variant === 'Other'
            ? 'This loader cannot be updated from here.'
            : 'Replace the server jar or loader. The world, server.properties, and mods stay.'}
        </h3>
      </div>
      {variant === 'Spigot' || variant === 'Other' ? null : (
        <div className="flex flex-col gap-4 rounded-lg border border-gray-faded/30 bg-gray-800 p-4">
          <label className="flex flex-col gap-1">
            <span className="text-medium font-medium text-gray-300">Minecraft version</span>
            {options.length ? (
              <select
                className="rounded border border-gray-faded/30 bg-gray-850 px-2 py-1 text-medium text-gray-300"
                value={version}
                disabled={!canUpdate || busy}
                onChange={(event) => setVersion(event.target.value)}
              >
                {options.map((option) => (
                  <option key={option} value={option}>
                    {option}
                  </option>
                ))}
              </select>
            ) : (
              <input
                className="rounded border border-gray-faded/30 bg-gray-850 px-2 py-1 text-medium text-gray-300"
                value={version}
                disabled={!canUpdate || busy}
                onChange={(event) => setVersion(event.target.value)}
              />
            )}
          </label>
          {variant === 'Fabric' ? (
            <div className="flex flex-col gap-3 sm:flex-row">
              <label className="flex grow flex-col gap-1">
                <span className="text-medium font-medium text-gray-300">Loader version</span>
                <input
                  className="rounded border border-gray-faded/30 bg-gray-850 px-2 py-1 text-medium text-gray-300"
                  value={loaderVersion}
                  placeholder="Latest stable"
                  disabled={!canUpdate || busy}
                  onChange={(event) => setLoaderVersion(event.target.value)}
                />
              </label>
              <label className="flex grow flex-col gap-1">
                <span className="text-medium font-medium text-gray-300">Installer version</span>
                <input
                  className="rounded border border-gray-faded/30 bg-gray-850 px-2 py-1 text-medium text-gray-300"
                  value={installerVersion}
                  placeholder="Latest stable"
                  disabled={!canUpdate || busy}
                  onChange={(event) => setInstallerVersion(event.target.value)}
                />
              </label>
            </div>
          ) : null}
          {variant === 'Paper' || variant === 'Forge' ? (
            <label className="flex flex-col gap-1">
              <span className="text-medium font-medium text-gray-300">
                {variant === 'Paper' ? 'Paper build' : 'Forge build'}
              </span>
              <input
                className="rounded border border-gray-faded/30 bg-gray-850 px-2 py-1 text-medium text-gray-300"
                value={build}
                placeholder={variant === 'Paper' ? 'Latest stable' : 'Latest'}
                disabled={!canUpdate || busy}
                onChange={(event) => setBuild(event.target.value)}
              />
            </label>
          ) : null}
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <p className="text-medium text-white/50">
              {stopped
                ? variant === 'Forge'
                  ? 'A blank Forge build uses the newest build.'
                  : variant === 'Vanilla'
                  ? 'This replaces server.jar for the selected Minecraft version.'
                  : 'A blank loader or build uses the latest stable one.'
                : 'Stop the server before updating the jar.'}
            </p>
            <Button
              label="Update jar"
              disabled={!canUpdate || !stopped || busy || version.trim() === ''}
              loading={busy}
              onClick={() => setConfirm(true)}
            />
          </div>
          {versions.error ? (
            <p className="text-medium text-red-200">
              Version list is unavailable. Type a version instead.
            </p>
          ) : null}
        </div>
      )}
      <ConfirmDialog
        isOpen={confirm}
        type="info"
        title="Update the server jar?"
        confirmButtonText="Update"
        onClose={() => {
          if (!busy) setConfirm(false);
        }}
        onConfirm={() => {
          void apply();
        }}
      >
        This downloads {variant} {version.trim() || 'the selected version'} and replaces the server
        jar. The world, server.properties, and mods are kept.
        {variant === 'Forge'
          ? ' Forge also runs its installer, then those folders are put back if the installer changes them.'
          : ''}
      </ConfirmDialog>
    </div>
  );
}
