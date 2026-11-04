import { LodestoneContext } from 'data/LodestoneContext';
import { useContext } from 'react';
import MyNavigate from './MyNavigate';

export default function RequireCore({
  children,
  redirect = '/first_setup',
}: {
  children: React.ReactNode;
  redirect?: string;
}) {
  const { coreList, core } = useContext(LodestoneContext);
  const hasCore = coreList.length > 0 || Boolean(core.sameOrigin);
  return hasCore ? <>{children}</> : <MyNavigate to={redirect} />;
}
