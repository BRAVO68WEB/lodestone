import { faExpand } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { RadioGroup } from '@headlessui/react';
import InstanceLoadingPill from 'components/InstanceLoadingPill';
import InstancePill from 'components/InstancePill';
import { InstanceContext } from 'data/InstanceContext';
import { NotificationContext } from 'data/NotificationContext';
import { useUserLoggedIn } from 'data/UserInfo';
import { useContext, useEffect } from 'react';
import useAnalyticsEventTracker from 'utils/hooks';
import { match, otherwise } from 'variant';
import { BrowserLocationContext } from 'data/BrowserLocationContext';
import { groupInstances } from 'data/instanceFolders';
import { InstanceInfo } from 'bindings/InstanceInfo';

function InstanceGroups({
  instances,
}: {
  instances: { [uuid: string]: InstanceInfo } | undefined;
}) {
  const groups = groupInstances(Object.values(instances ?? {}));
  const named = groups.some((group) => group.folder !== '');
  if (!named) {
    return (
      <>
        <RadioGroup.Label className="text-small font-bold leading-snug text-gray-faded/30">
          ALL INSTANCES
        </RadioGroup.Label>
        {(groups[0]?.items ?? []).map((instance) => (
          <RadioGroup.Option
            key={instance.uuid}
            value={instance}
            className="rounded-md outline-none child:w-full"
          >
            <InstancePill {...instance} />
          </RadioGroup.Option>
        ))}
      </>
    );
  }
  return (
    <>
      {groups.map((group) => (
        <div key={group.folder || 'ungrouped'} className="flex flex-col gap-y-1">
          <p className="mt-3 text-small font-bold leading-snug text-gray-faded/30">
            {group.folder || 'Ungrouped'}
          </p>
          {group.items.map((instance) => (
            <RadioGroup.Option
              key={instance.uuid}
              value={instance}
              className="rounded-md outline-none child:w-full"
            >
              <InstancePill {...instance} />
            </RadioGroup.Option>
          ))}
        </div>
      ))}
    </>
  );
}

export default function InstanceList({
  className = '',
  children,
}: {
  className?: string;
  children?: React.ReactNode;
}) {
  const gaEventTracker = useAnalyticsEventTracker('Instance List');
  const {
    instanceList: instances,
    selectedInstance,
    selectInstance,
    isReady,
  } = useContext(InstanceContext);
  const { ongoingNotifications } = useContext(NotificationContext);
  const userLoggedIn = useUserLoggedIn();
  const {
    location: { pathname },
  } = useContext(BrowserLocationContext);

  useEffect(() => {
    if (!isReady) return;
    gaEventTracker(
      'View',
      'Instance List',
      true,
      Object.keys(instances).length
    );
  }, [isReady, instances]);

  useEffect(() => {
    if (pathname == '/') {
      selectInstance(null);
    }
  }, [pathname]);

  return (
    <RadioGroup
      className={`mx-1 flex min-h-0 flex-col gap-y-1 overflow-y-auto px-1 child:w-full ${className}`}
      value={selectedInstance}
      onChange={selectInstance}
    >
      {userLoggedIn ? (
        <InstanceGroups instances={instances} />
      ) : (
        <div
          className={`mt-2 flex w-fit select-none flex-col items-stretch gap-4 rounded-xl border-2 border-dashed border-gray-faded/10 py-4 px-6 text-medium font-bold tracking-tight`}
        >
          <FontAwesomeIcon
            icon={faExpand}
            className="text-h1 text-gray-faded/30"
          />
          <p className="text-center text-medium italic text-gray-faded/30">
            Log in to view your server instances.
          </p>
        </div>
      )}
      {ongoingNotifications &&
        ongoingNotifications
          .map((notification) => {
            if (!notification.start_value) return null;
            if (notification.state === 'done') return null;
            return match(
              notification.start_value,
              otherwise(
                {
                  InstanceCreation: ({ instance_uuid }) => (
                    <InstanceLoadingPill
                      key={instance_uuid}
                      progress_percent={
                        notification.total
                          ? notification.progress / notification.total
                          : undefined
                      }
                    />
                  ),
                },
                (_) => null
              )
            );
          })
          .reverse()}
      {children}
    </RadioGroup>
  );
}
