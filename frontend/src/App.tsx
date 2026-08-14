import { Routes, Route, Navigate } from "react-router-dom";
import { useAuth } from "./contexts/AuthContext";
import { LoginPage } from "./pages/LoginPage";
import { DashboardPage } from "./pages/DashboardPage";
import { DevicePage } from "./pages/DevicePage";
import { HueLampPage } from "./pages/HueLampPage";
import { MerossPlugPage } from "./pages/MerossPlugPage";
import { ZigbeeLampPage } from "./pages/ZigbeeLampPage";
import TempoPredictionPage from "./pages/TempoPredictionPage";
import { Layout } from "./components/Layout";

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading } = useAuth();

  if (isLoading) {
    return (
      <div className="flex h-screen items-center justify-center">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary"></div>
      </div>
    );
  }

  if (!isAuthenticated) {
    return <Navigate to="/login" replace />;
  }

  return <>{children}</>;
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        path="/"
        element={
          <ProtectedRoute>
            <Layout />
          </ProtectedRoute>
        }
      >
        <Route index element={<DashboardPage />} />
        <Route path="device/:deviceId" element={<DevicePage />} />
        <Route path="hue-lamp/:lampId" element={<HueLampPage />} />
        <Route path="zigbee-lamp/:lampId" element={<ZigbeeLampPage />} />
        <Route path="meross/:deviceId" element={<MerossPlugPage />} />
        <Route path="tempo-predictions" element={<TempoPredictionPage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
