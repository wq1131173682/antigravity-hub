import { createBrowserRouter, RouterProvider } from 'react-router-dom';
import Layout from './components/layout/Layout';
import Dashboard from './pages/Dashboard';
import Accounts from './pages/Accounts';
import Settings from './pages/Settings';
import ThemeManager from './components/common/ThemeManager';
import { useEffect } from 'react';
import { useConfigStore } from './stores/useConfigStore';
import { useTranslation } from 'react-i18next';

const router = createBrowserRouter([
  {
    path: '/',
    element: <Layout />,
    children: [
      { index: true, element: <Dashboard /> },
      { path: 'accounts', element: <Accounts /> },
      { path: 'settings', element: <Settings /> },
    ],
  },
]);

function App() {
  const { loadConfig } = useConfigStore();
  const { i18n } = useTranslation();

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  useEffect(() => {
    if (useConfigStore.getState().config?.language) {
      const lang = useConfigStore.getState().config!.language;
      i18n.changeLanguage(lang);
      document.documentElement.dir = lang === 'ar' ? 'rtl' : 'ltr';
    }
  }, [i18n]);

  return (
    <>
      <ThemeManager />
      <RouterProvider router={router} />
    </>
  );
}

export default App;