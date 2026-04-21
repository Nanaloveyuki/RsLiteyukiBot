import { Suspense, lazy } from "react";
import { Navigate, Route, Routes } from "react-router-dom";

import Layout from "@/layouts/default";

const OverviewPage = lazy(() => import("@/pages/overview"));
const RuntimePage = lazy(() => import("@/pages/runtime"));
const AdaptersPage = lazy(() => import("@/pages/adapters"));
const LlmPage = lazy(() => import("@/pages/llm"));
const CommandsPage = lazy(() => import("@/pages/commands"));
const PluginsPage = lazy(() => import("@/pages/plugins"));
const LogsPage = lazy(() => import("@/pages/logs"));
const DiagnosticsPage = lazy(() => import("@/pages/diagnostics"));

export default function App() {
  return (
    <Suspense
      fallback={
        <div className="flex min-h-screen items-center justify-center bg-white/70 text-sm text-default-500 backdrop-blur-xs dark:bg-slate-950/65 dark:text-slate-300">
          loading page...
        </div>
      }
    >
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<OverviewPage />} />
          <Route path="/runtime" element={<RuntimePage />} />
          <Route path="/adapters" element={<AdaptersPage />} />
          <Route path="/llm" element={<LlmPage />} />
          <Route path="/commands" element={<CommandsPage />} />
          <Route path="/plugins" element={<PluginsPage />} />
          <Route path="/logs" element={<LogsPage />} />
          <Route path="/diagnostics" element={<DiagnosticsPage />} />
          <Route path="*" element={<Navigate replace to="/" />} />
        </Route>
      </Routes>
    </Suspense>
  );
}
