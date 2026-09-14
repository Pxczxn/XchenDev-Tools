import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";
import { ThemeProvider } from "./context/ThemeContext";
import { AppLayout } from "./layout/AppLayout";
import { DashboardPage } from "./pages/DashboardPage";
import { EnvironmentsPage } from "./pages/EnvironmentsPage";
import { PortsPage } from "./pages/PortsPage";
import { ProcessesPage } from "./pages/ProcessesPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import { ServicesPage } from "./pages/ServicesPage";
import { SettingsPage } from "./pages/SettingsPage";

function App() {
  return (
    <ThemeProvider>
      <BrowserRouter>
        <Routes>
          <Route element={<AppLayout />}>
            <Route path="/" element={<DashboardPage />} />
            <Route path="/environments" element={<EnvironmentsPage />} />
            <Route path="/ports" element={<PortsPage />} />
            <Route path="/processes" element={<ProcessesPage />} />
            <Route path="/projects" element={<ProjectsPage />} />
            <Route path="/services" element={<ServicesPage />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </ThemeProvider>
  );
}

export default App;
