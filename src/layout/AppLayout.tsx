import { NavLink, Outlet } from "react-router-dom";
import { ThemeToggle } from "../components/ThemeToggle";
import "../styles/app.css";
import "../styles/shadcn.css";

const links = [
  { to: "/", label: "系统概览" },
  { to: "/environments", label: "环境管理" },
  { to: "/ports", label: "端口管理" },
  { to: "/processes", label: "进程管理" },
  { to: "/projects", label: "项目管理" },
  { to: "/services", label: "基础服务" },
  { to: "/settings", label: "设置" },
];

export function AppLayout() {
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="brand">XchenDev-Tools</div>
          <div className="brand-sub">本机开发工具</div>
        </div>
        <nav className="sidebar-nav" aria-label="主导航">
          {links.map((l) => (
            <NavLink
              key={l.to}
              to={l.to}
              end={l.to === "/"}
              className={({ isActive }) =>
                isActive ? "nav-link active" : "nav-link"
              }
            >
              {l.label}
            </NavLink>
          ))}
        </nav>
        <div className="sidebar-footer">
          <ThemeToggle />
        </div>
      </aside>
      <main className="main">
        <div className="main-inner">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
