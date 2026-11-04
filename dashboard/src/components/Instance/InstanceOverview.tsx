import ClipboardTextfield from 'components/ClipboardTextfield';
import Button from 'components/Atoms/Button';
import Label from 'components/Atoms/Label';
import ToggleBox from 'components/Atoms/Config/ToggleBox';
import InputBox from 'components/Atoms/Config/InputBox';
import { updateInstance } from 'data/InstanceList';
import { LodestoneContext } from 'data/LodestoneContext';
import { useContext, useState } from 'react';
import axios from 'axios';
import { axiosPutSingleValue, errorToString, stateToLabelColor } from 'utils/util';
import EditableTextfield from 'components/EditableTextfield';
import { useQueryClient } from '@tanstack/react-query';
import InstancePerformance from 'components/Instance/InstancePerformance';
import { InstanceContext } from 'data/InstanceContext';
import GameIcon from 'components/Atoms/GameIcon';
import { useGlobalSettings } from 'data/GlobalSettings';
import { useUserAuthorized } from 'data/UserInfo';
import { toast } from 'react-toastify';

import { useDocumentTitle } from 'usehooks-ts';
import InstancePlayerList from './InstancePlayerList';

const InstanceOverview = () => {
  useDocumentTitle('Instance Overview - Lodestone');
  const { core } = useContext(LodestoneContext);
  const { address } = core;
  const { selectedInstance: instance } = useContext(InstanceContext);
  const { data: globalSettings } = useGlobalSettings();
  const domain = (globalSettings?.domain ?? address) || 'localhost';
  const queryClient = useQueryClient();
  const uuid = instance?.uuid;
  const canChangeSettings = useUserAuthorized(
    'can_access_instance_setting',
    uuid ?? ''
  );
  const [powerLoading, setPowerLoading] = useState(false);

  if (!instance || !uuid) {
    return (
      <div
        className="relative flex h-full w-full flex-row justify-center overflow-y-auto px-4 pt-8 pb-10 @container"
        key={uuid}
      >
        <div className="flex h-fit min-h-full w-full grow flex-col items-start gap-2">
          <div className="flex min-w-0 flex-row items-center gap-4">
            <h1 className="dashboard-instance-heading truncate whitespace-pre">
              Instance not found
            </h1>
          </div>
        </div>
      </div>
    );
  }

  const labelColor = stateToLabelColor[instance.state];

  // tablist is map from GameType to tabs

  const setInstanceName = async (name: string) => {
    await axiosPutSingleValue<void>(`/instance/${uuid}/name`, name);
    updateInstance(uuid, queryClient, (oldData) => ({
      ...oldData,
      name,
    }));
  };

  const setInstanceDescription = async (description: string) => {
    await axiosPutSingleValue<void>(
      `/instance/${uuid}/description`,
      description
    );
    updateInstance(uuid, queryClient, (oldData) => ({
      ...oldData,
      description,
    }));
  };

  const setInstanceFolder = async (folder: string) => {
    const trimmed = folder.trim();
    await axiosPutSingleValue<void>(`/instance/${uuid}/folder`, trimmed);
    updateInstance(uuid, queryClient, (oldData) => ({
      ...oldData,
      folder: trimmed,
    }));
  };

  const setInstanceFlag = async (
    path: 'auto_start' | 'restart_on_crash',
    value: boolean
  ) => {
    await axiosPutSingleValue<void>(`/instance/${uuid}/${path}`, value);
    updateInstance(uuid, queryClient, (oldData) => ({
      ...oldData,
      [path]: value,
    }));
  };

  const powerLabel =
    instance.state === 'Running'
      ? 'Stop'
      : instance.state === 'Starting'
        ? 'Starting'
        : instance.state === 'Stopping'
          ? 'Stopping'
          : 'Start';
  const powerDisabled =
    powerLoading ||
    instance.state === 'Starting' ||
    instance.state === 'Stopping';

  const sendPower = async () => {
    if (instance.state === 'Starting' || instance.state === 'Stopping') return;
    const action = instance.state === 'Running' ? 'stop' : 'start';
    setPowerLoading(true);
    try {
      await axios.put(`/instance/${uuid}/${action}`);
    } catch (err) {
      toast.error(errorToString(err));
    } finally {
      setPowerLoading(false);
    }
  };

  return (
    <>
      <div
        className="relative flex h-full w-full max-w-2xl flex-col justify-center @container"
        key={uuid}
      >
        {/* main content container */}
        <div className="flex w-full grow flex-col items-stretch gap-2 ">
          <div className="flex w-full min-w-0 flex-row items-center gap-4">
            <EditableTextfield
              initialText={instance.name}
              type={'heading'}
              onSubmit={setInstanceName}
              placeholder="No name"
              containerClassName="min-w-0"
            />
          </div>
          <div className="-mt-2 flex flex-row flex-wrap items-center gap-4">
            <GameIcon game_type={instance.game_type} className="h-6 w-6" />
            <Label size="large" color={labelColor}>
              {instance.state}
            </Label>
            <Button
              label={powerLabel}
              intention={instance.state === 'Running' ? 'danger' : 'info'}
              disabled={powerDisabled}
              loading={powerLoading}
              onClick={() => {
                void sendPower();
              }}
            />
            <Label size="large" color={'blue'}>
              Version {instance.version}
            </Label>
            <Label size="large" color={'blue'}>
              Player Count {instance.player_count}/{instance.max_player_count}
            </Label>
            <Label size="large" color={'blue'}>
              <ClipboardTextfield
                text={`${domain}:${instance.port}`}
                color="blue"
                iconLeft={false}
              />
            </Label>
          </div>
          <div className="flex w-full flex-row items-center gap-2">
            <EditableTextfield
              initialText={instance.description}
              type={'description'}
              onSubmit={setInstanceDescription}
              placeholder="No description"
              containerClassName="min-w-0"
            />
          </div>
          <div className="mt-2 flex w-full flex-col overflow-hidden rounded-lg border border-gray-faded/30 child:border-b child:border-gray-faded/30 last:child:border-b-0">
            <InputBox
              label="Folder"
              value={instance.folder ?? ''}
              canRead={canChangeSettings}
              description="Groups this instance in the sidebar. Leave this empty to keep it with the other ungrouped instances."
              onSubmit={setInstanceFolder}
            />
            <ToggleBox
              label="Auto Start"
              value={instance.auto_start}
              canRead={canChangeSettings}
              description="Start this instance when Lodestone Core starts."
              onChange={(value) => setInstanceFlag('auto_start', value)}
            />
            <ToggleBox
              label="Restart on Crash"
              value={instance.restart_on_crash}
              canRead={canChangeSettings}
              description="The instance will restart automatically if it crashes."
              onChange={(value) => setInstanceFlag('restart_on_crash', value)}
            />
          </div>
        </div>
      </div>
      <InstancePerformance />
      <InstancePlayerList />
    </>
  );
};

export default InstanceOverview;
