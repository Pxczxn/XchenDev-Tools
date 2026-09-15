from pathlib import Path


def replace_exact(path: str, old: str, new: str, count: int = 1) -> None:
    target = Path(path)
    text = target.read_text(encoding="utf-8")
    actual = text.count(old)
    if actual != count:
        raise SystemExit(f"{path}: expected {count} match(es), found {actual}")
    target.write_text(text.replace(old, new, count), encoding="utf-8", newline="\n")


replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<div className="card-body env-candidate-list">\n            {projects.map((project) => (',
    '<div className="card-body project-grid">\n            {projects.map((project) => (',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<div key={project.project_id} className="env-candidate">',
    '<div\n                key={project.project_id}\n                className={`env-candidate project-card ${\n                  projectId === project.project_id ? "active" : ""\n                }`}\n              >',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<span className="env-tag">{project.name}</span>',
    '<span className="project-name">{project.name}</span>',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<div className="card toolbar-card env-toolbar">',
    '<div className="card toolbar-card env-toolbar project-toolbar">\n        <div className="project-toolbar-label">项目路径</div>',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '{candidates.length > 0 && (\n        <div className="env-candidate-list">',
    '{candidates.length > 0 && (\n        <div className="project-stack-grid">',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<div key={c.id} className="env-candidate">',
    '<div key={c.id} className="env-candidate project-stack-card">',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '{selected && (\n        <div className="card">',
    '{selected && (\n        <div className="card project-config-card">',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '{profiles.length > 0 && (\n        <div className="card">',
    '{profiles.length > 0 && (\n        <div className="card project-runtime-card">',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<div className="card-body env-candidate-list">\n            {profiles.map((profile) => {',
    '<div className="card-body project-profile-list">\n            {profiles.map((profile) => {',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<div key={profile.profile_id} className="env-candidate">',
    '<div key={profile.profile_id} className="env-candidate project-profile-card">',
)
replace_exact(
    "src/pages/ProjectsPage.tsx",
    '<div className="log-panel">\n                      {sessionLogs.join("\\n") || "暂无输出"}',
    '<div className="log-panel project-log-panel">\n                      {sessionLogs.join("\\n") || "暂无输出"}',
)

css_path = Path("src/styles/app.css")
css = css_path.read_text(encoding="utf-8")
anchor = '''.project-runtime-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-2);
  flex-wrap: wrap;
}
'''
styles = anchor + '''

/* Project management */
.project-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: var(--space-3);
}

.project-card {
  position: relative;
  min-height: 142px;
  display: flex;
  flex-direction: column;
  background: linear-gradient(145deg, var(--surface-raised), var(--input-bg));
  border-color: var(--border);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.08);
}

.project-card::before {
  content: "";
  position: absolute;
  inset: 0 auto 0 0;
  width: 3px;
  border-radius: var(--radius-md) 0 0 var(--radius-md);
  background: transparent;
}

.project-card.active {
  border-color: var(--accent);
  box-shadow: 0 0 0 1px var(--nav-active-bg), 0 12px 30px rgba(0, 0, 0, 0.12);
}

.project-card.active::before {
  background: var(--accent);
}

.project-card .list-card-actions {
  margin-top: auto;
}

.project-name {
  min-width: 0;
  font-size: 0.95rem;
  font-weight: 650;
  color: var(--text-strong);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project-toolbar {
  display: grid;
  grid-template-columns: auto minmax(220px, 1fr) auto auto;
  gap: var(--space-2);
  align-items: center;
  background: var(--surface-raised);
}

.project-toolbar-label {
  font-size: 0.76rem;
  font-weight: 600;
  color: var(--muted);
  letter-spacing: 0.02em;
}

.project-toolbar .env-path-input {
  width: 100%;
  min-width: 0;
}

.project-stack-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: var(--space-3);
  margin-bottom: var(--space-4);
}

.project-stack-card {
  min-height: 168px;
  display: flex;
  flex-direction: column;
  background: var(--surface);
}

.project-stack-card .list-card-actions {
  margin-top: auto;
}

.project-config-card {
  border-color: var(--nav-active-border);
}

.project-config-card .card-header {
  background: var(--nav-active-bg);
}

.project-runtime-card {
  border-color: var(--border);
}

.project-runtime-card > .card-header {
  position: sticky;
  top: 0;
  z-index: 2;
  backdrop-filter: blur(10px);
}

.project-runtime-actions .env-badge {
  min-height: 28px;
  padding-inline: 0.7rem;
}

.project-profile-list {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  gap: var(--space-3);
}

.project-profile-card {
  display: flex;
  flex-direction: column;
  min-width: 0;
  background: var(--input-bg);
}

.project-profile-card .env-path {
  padding: 0.55rem 0.65rem;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
  background: var(--log-bg);
}

.project-profile-card .list-card-actions {
  margin-top: auto;
}

.project-log-panel {
  margin-top: var(--space-3);
  min-height: 88px;
  max-height: 220px;
}

@media (max-width: 760px) {
  .project-toolbar {
    grid-template-columns: 1fr 1fr;
  }

  .project-toolbar-label,
  .project-toolbar .env-path-input {
    grid-column: 1 / -1;
  }

  .project-runtime-card > .card-header {
    align-items: flex-start;
    flex-direction: column;
  }

  .project-runtime-actions {
    width: 100%;
    justify-content: flex-start;
  }

  .project-profile-list,
  .project-stack-grid,
  .project-grid {
    grid-template-columns: 1fr;
  }
}
'''
if css.count(anchor) != 1:
    raise SystemExit(f"project runtime css anchor count={css.count(anchor)}")
css_path.write_text(css.replace(anchor, styles, 1), encoding="utf-8", newline="\n")
