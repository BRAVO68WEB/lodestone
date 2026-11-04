import InstanceList from './InstanceList';
import { Fragment, useContext, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import axios from 'axios';
import Button from 'components/Atoms/Button';
import { faSquarePlus } from '@fortawesome/free-solid-svg-icons';
import { Dialog, Transition } from '@headlessui/react';
import CreateInstanceFlow from 'components/Instance/InstanceCreateForm';
import { useUserAuthorized, useUserLoggedIn } from 'data/UserInfo';
import UserMenu from 'components/UserMenu';
import clsx from 'clsx';
import { SelectedInstanceInfo } from './SelectedInstanceInfo';
import { InstanceContext } from 'data/InstanceContext';
import IntegrationsList from './IntegrationsList';
import { PlayitggOverview } from 'components/PlayitggOverview';
import { toast } from 'react-toastify';
import { errorToString } from 'utils/util';
export default function LeftNav({ className }: { className?: string }) {
  const { showCreateInstance, setShowCreateInstance } =
    useContext(InstanceContext);
  const canCreateInstance = useUserAuthorized('can_create_instance');
  const userLoggedIn = useUserLoggedIn();
  const queryClient = useQueryClient();
  const importRef = useRef<HTMLInputElement>(null);
  const [importing, setImporting] = useState(false);

  const importServer = async (file: File | undefined) => {
    if (!file) return;
    const body = new FormData();
    body.append('file', file);
    body.append('name', file.name.replace(/\.zip$/i, ''));
    setImporting(true);
    try {
      await axios.post('/instance/import', body, { timeout: 120000 });
      await queryClient.invalidateQueries(['instances', 'list']);
      toast.success('Server imported');
    } catch (err) {
      toast.error(errorToString(err));
    } finally {
      setImporting(false);
      if (importRef.current) importRef.current.value = '';
    }
  };
  return (
    <div
      className={`overflow-y-overlay flex w-full flex-col items-center overflow-y-auto px-2 ${className}`}
    >
      <div className="mt-10 flex h-full w-full grow flex-col ">
        <UserMenu />
        <Transition
          appear
          show={showCreateInstance}
          as={Fragment}
          enter="ease-out duration-200"
          enterFrom="opacity-0"
          enterTo="opacity-100"
          leave="ease-in duration-150"
          leaveFrom="opacity-100"
          leaveTo="opacity-0"
        >
          <Dialog onClose={() => setShowCreateInstance(false)} className="z-10">
            <div className="fixed inset-0 bg-gray-900/60" />
            <div className="fixed inset-0 overflow-y-auto">
              <div className="flex min-h-full items-center justify-center p-4 text-center">
                <Dialog.Panel className="w-full max-w-[812px]">
                  <CreateInstanceFlow
                    onComplete={() => setShowCreateInstance(false)}
                  />
                </Dialog.Panel>
              </div>
            </div>
          </Dialog>
        </Transition>

        <div className="h-full">
          <SelectedInstanceInfo />

          <InstanceList className="mt-6">
            {userLoggedIn && (
              <div className="flex w-full flex-col items-stretch justify-center gap-2 pb-8">
                <input
                  ref={importRef}
                  type="file"
                  accept=".zip,application/zip"
                  className="hidden"
                  onChange={(event) => {
                    void importServer(event.target.files?.[0]);
                  }}
                />
                <Button
                  label={importing ? 'Importing...' : 'Import server...'}
                  className="w-full text-medium font-medium tracking-normal text-white/50"
                  intention="none"
                  variant="text"
                  disabled={!canCreateInstance || importing}
                  onClick={() => importRef.current?.click()}
                  align="start"
                />
                <Button
                  label={canCreateInstance ? "New instance..." : "No permission"}
                  className={clsx(
                    'w-full text-medium font-medium tracking-normal',
                    showCreateInstance
                      ? 'bg-gray-850 text-gray-300 outline outline-1 outline-fade-700/10'
                      : 'text-white/50',
                    'disabled:text-white/30',
                    'enabled:hover:bg-gray-800 enabled:focus-visible:outline-none',
                    'enabled:active:bg-gray-850 enabled:active:text-gray-300 enabled:active:outline enabled:active:outline-1 enabled:active:outline-fade-700/10'
                  )}
                  intention="none"
                  variant="text"
                  icon={faSquarePlus}
                  disabled={!canCreateInstance}
                  onClick={() => setShowCreateInstance(true)}
                  align="start"
                />
              </div>
            )}
          </InstanceList>
        </div>
      </div>
    </div>
  );
}
