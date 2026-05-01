import { Suspense, lazy, useEffect } from 'react';
import { Provider } from 'react-redux';
import { Route, Routes, useNavigate } from 'react-router-dom';

import PageBackground from '@/components/page_background';
import PageLoading from '@/components/page_loading';
import Toaster from '@/components/toaster';
import DesktopClosePrompt from '@/components/desktop_close_prompt';

import DialogProvider from '@/contexts/dialog';

import useAuth from '@/hooks/auth';

import store from '@/store';

const WebLoginPage = lazy(() => import('@/pages/web_login'));
const IndexPage = lazy(() => import('@/pages/index'));
const DashboardIndexPage = lazy(() => import('@/pages/dashboard'));
const AboutPage = lazy(() => import('@/pages/dashboard/about'));
const ConfigPage = lazy(() => import('@/pages/dashboard/config'));
const DebugPage = lazy(() => import('@/pages/dashboard/debug'));
const HttpDebug = lazy(() => import('@/pages/dashboard/debug/http'));
const WSDebug = lazy(() => import('@/pages/dashboard/debug/websocket'));
const CapabilitiesPage = lazy(() => import('@/pages/dashboard/capabilities'));
const YukiFlowPage = lazy(() => import('@/pages/dashboard/yuki_flow'));
const FileManagerPage = lazy(() => import('@/pages/dashboard/file_manager'));
const LogsPage = lazy(() => import('@/pages/dashboard/logs'));
const NetworkPage = lazy(() => import('@/pages/dashboard/network'));
const TerminalPage = lazy(() => import('@/pages/dashboard/terminal'));
const PluginPage = lazy(() => import('@/pages/dashboard/plugin'));
const PluginStorePage = lazy(() => import('@/pages/dashboard/plugin_store'));
const ExtensionPage = lazy(() => import('@/pages/dashboard/extension'));

function App () {
  return (
    <DialogProvider>
      <Provider store={store}>
        <PageBackground />
        <Toaster />
        <DesktopClosePrompt />
        <Suspense fallback={<PageLoading />}>
          <AuthChecker>
            <AppRoutes />
          </AuthChecker>
        </Suspense>
      </Provider>
    </DialogProvider>
  );
}

function AuthChecker ({ children }: { children: React.ReactNode; }) {
  const { isAuth } = useAuth();
  const navigate = useNavigate();

  useEffect(() => {
    if (!isAuth) {
      const search = new URLSearchParams(window.location.search);
      const token = search.get('token');
      let url = '/web_login';

      if (token) {
        url += `?token=${token}`;
      }
      navigate(url, { replace: true });
    }
  }, [isAuth, navigate]);

  return <>{children}</>;
}

function AppRoutes () {
  return (
    <Routes>
      <Route path='/' element={<IndexPage />}>
        <Route index element={<DashboardIndexPage />} />
        <Route path='network' element={<NetworkPage />} />
        <Route path='config' element={<ConfigPage />} />
        <Route path='logs' element={<LogsPage />} />
        <Route path='debug' element={<DebugPage />}>
          <Route path='ws' element={<WSDebug />} />
          <Route path='http' element={<HttpDebug />} />
        </Route>
        <Route path='capabilities' element={<CapabilitiesPage />} />
        <Route path='yuki_flow' element={<YukiFlowPage />} />
        <Route path='file_manager' element={<FileManagerPage />} />
        <Route path='terminal' element={<TerminalPage />} />
        <Route path='plugins' element={<PluginPage />} />
        <Route path='plugin_store' element={<PluginStorePage />} />
        <Route path='extension' element={<ExtensionPage />} />
        <Route path='about' element={<AboutPage />} />
      </Route>
      <Route path='/web_login' element={<WebLoginPage />} />
    </Routes>
  );
}

export default App;
