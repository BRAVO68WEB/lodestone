import { Outlet } from 'react-router-dom';

const LoginLayout = () => {
  return (
    <div
      className="flex h-screen flex-col overflow-y-auto p-4 sm:p-16 lg:p-32"
      style={{
        background: "url('/login_background.svg')",
        backgroundSize: 'cover',
        backgroundPosition: 'center',
      }}
    >
      <div className="m-auto w-full">
        <Outlet />
      </div>
    </div>
  );
};

export default LoginLayout;
