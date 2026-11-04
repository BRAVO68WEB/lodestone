import { useEffect, useState } from 'react';
import { useDocumentTitle } from 'usehooks-ts';
import { useUserAuthorized, useUserInfo, useUserLoggedIn } from 'data/UserInfo';
import { useContext } from 'react';
import Spinner from 'components/DashboardLayout/Spinner';
import Button from 'components/Atoms/Button';
import { InstanceContext } from 'data/InstanceContext';
import { useInstanceList } from 'data/InstanceList';
import { useMonitorLatest } from 'data/PerformanceStream';
import { sumSamples } from 'data/monitorSeries';
import { formatCpu, formatMemoryGiB } from 'components/Instance/InstancePerformance';

const Home = () => {
  useDocumentTitle('Home - Lodestone');
  const { setShowCreateInstance } = useContext(InstanceContext);
  const userLoggedIn = useUserLoggedIn();
  const canCreateInstance = useUserAuthorized('can_create_instance');
  const { isLoading } = useUserInfo();
  const { data: instanceList } = useInstanceList(userLoggedIn);
  const { data: samples } = useMonitorLatest(userLoggedIn);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    //give time for user to load
    setTimeout(() => {
      setLoading(false);
    }, 1000);
  }, []);

  if (loading && isLoading) {
    return <Spinner />;
  }

  const instances = Object.values(instanceList ?? {});
  const rows = instances.map((instance) => {
    const sample = samples?.find((item) => item.uuid === instance.uuid);
    return {
      uuid: instance.uuid,
      name: instance.name,
      cpu_usage: sample?.cpu_usage ?? null,
      memory_usage: sample?.memory_usage ?? null,
    };
  });
  const totals = sumSamples(rows);

  return (
    // used to possibly center the content
    <div className="relative flex flex-col items-center justify-center overflow-auto px-4 text-gray-faded/30">
      {instances.length > 0 && (
        <div className="mb-8 w-full max-w-lg rounded-lg border border-gray-faded/30 bg-gray-800 px-4 py-3 text-left">
          <h2 className="text-h3 font-bold text-gray-300">Combined instance usage</h2>
          <p className="text-small text-white/50">
            Latest sample from each instance you can view.
            {totals.reporting > 0
              ? ` ${totals.reporting} reporting.`
              : ' No samples yet.'}
          </p>
          <p className="mt-2 text-medium font-medium text-gray-300">
            CPU {formatCpu(totals.cpu)} · Memory {formatMemoryGiB(totals.memory)}
          </p>
          <ul className="mt-2 flex flex-col gap-1">
            {rows.map((row) => (
              <li key={row.uuid} className="text-small text-white/50">
                {row.name}: {formatCpu(row.cpu_usage)} · {formatMemoryGiB(row.memory_usage)}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="flex h-28 w-28 items-center justify-center rounded-full border border-dashed border-gray-faded/30 align-middle">
        <img
          src="/assets/placeholder-cube.png"
          alt="placeholder"
          className="mx-auto w-20"
          style={{ imageRendering: 'pixelated' }}
        />
      </div>
      <div className="mt-4 text-h2 font-extrabold ">Manage your instances</div>
      <div className="text-medium font-bold">
        To start, select an instance from the menu or create a new instance
      </div>
      {userLoggedIn && (
        <div className="mt-5">
          <Button
            align="start"
            labelGrow={true}
            className="w-full"
            label="Create a new instance"
            onClick={() => setShowCreateInstance(true)}
            disabled={!canCreateInstance}
          />
        </div>
      )}
    </div>
  );
};

export default Home;
