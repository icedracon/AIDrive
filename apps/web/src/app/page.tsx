"use client";

import { useEffect, useRef, useState } from "react";
import { House, FolderSimple, SquaresFour, Cube, GearSix, MagnifyingGlass, Plus, ArrowRight, CloudArrowUp, FileText, Sparkle, ChatsCircle, Code, Graph, ShieldCheck, Pulse, ClockCounterClockwise, CalendarBlank, ChartBar, CaretUpDown, Check } from "@phosphor-icons/react";
import { requestSignOut } from "../lib/session.mjs";
import AgentAccess from "./agent-access";
import FilesPanel, { confirmFileNavigation } from "./files-panel";
import HandoffsPanel from "./handoffs-panel";
import ActivityPanel from "./activity-panel";
import SessionsPanel from "./sessions-panel";
import KnowledgePanel from "./knowledge-panel";
import MessagesPanel from "./messages-panel";
import ReviewPanel from "./review-panel";
import WorkspaceConsole from "./workspace-console";
import MapPanel from "./map/map-panel";
import ProjectGallery, { isProjectSummary, type ProjectSummary } from "./project-gallery";
import ProjectMark from "./project-mark";
import styles from "./shell.module.css";

function Icon({ name }: { name: string }) {
  const icons = { home: House, drive: FolderSimple, spaces: SquaresFour, memory: Cube, settings: GearSix, search: MagnifyingGlass, plus: Plus, arrow: ArrowRight, cloud: CloudArrowUp, file: FileText, spark: Sparkle, chat: ChatsCircle, handoff: Code, map: Graph, review: ShieldCheck, activity: Pulse, sessions: ClockCounterClockwise, calendar: CalendarBlank };
  const Component = icons[name as keyof typeof icons] || FolderSimple;
  return <Component size={20} weight="regular" aria-hidden="true" />;
}

type ApiStatus = "checking" | "ready" | "degraded" | "offline";

const apiLabels: Record<ApiStatus, string> = {
  checking: "Checking API",
  ready: "API and database ready",
  degraded: "API up, database not ready",
  offline: "API offline",
};

type SessionUser = { id: string; email: string };
type AuthState = { phase: "loading" } | { phase: "signed-out" } | { phase: "signed-in"; user: SessionUser; local?: boolean } | { phase: "unavailable"; reason?: string };

const authErrors: Record<string, string> = {
  invalid_credentials: "Email or password is incorrect.",
  email_taken: "An account with this email already exists.",
  invalid_email: "Enter a valid email address.",
  password_too_short: "Use at least 15 characters.",
  password_too_long: "Use at most 256 characters.",
  password_blocklisted: "Choose a less common password that does not contain your email.",
  signup_disabled: "Account creation is disabled on this server.",
  cross_origin_request: "The request was blocked as cross-origin.",
  unavailable: "The server cannot reach its database. Try again shortly.",
};

function AuthPanel({ auth, onChange }: { auth: AuthState; onChange: (next: AuthState) => void }) {
  const [reauth, setReauth] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(path: "login" | "register") {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/auth/${path}`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email: reauth && auth.phase === "signed-in" ? auth.user.email : email, password }),
      });
      const body = await response.json().catch(() => null);
      if (response.ok && body?.user) {
        if (reauth && auth.phase === "signed-in" && body.user.id !== auth.user.id) {
          setError("The renewed session belongs to another account. Reload after saving your drafts locally.");
          return;
        }
        setPassword("");
        setReauth(false);
        onChange({ phase: "signed-in", user: body.user });
      } else {
        setError(authErrors[body?.error] ?? "Something went wrong. Nothing was saved.");
      }
    } catch {
      setError("The API is unreachable.");
    } finally {
      setBusy(false);
    }
  }

  async function signOut() {
    if (!confirmFileNavigation()) return;
    setBusy(true);
    setError(null);
    try {
      if (!await requestSignOut()) {
        setError("Sign-out could not be confirmed. Retry to finish signing out.");
        return;
      }
      onChange({ phase: "signed-out" });
    } finally {
      setBusy(false);
    }
  }

  if (auth.phase === "loading") return <section className="auth-card" aria-busy="true"><p>Checking your session…</p></section>;
  if (auth.phase === "unavailable") return <section className="auth-card"><p role="alert">{auth.reason ?? "Sign-in is unavailable: the API or database is not ready."}</p></section>;
  if (auth.phase === "signed-in" && auth.local) {
    // Local install: this computer's account is the owner; nothing to sign in or out of.
    return (
      <section className="auth-card signed-in" aria-label="Account">
        <p>On this computer as <strong>{auth.user.email}</strong></p>
        <button type="button" className="secondary-button" disabled={busy} onClick={async () => {
          setBusy(true); setError(null);
          try {
            const response = await fetch("/api/local/session", { method: "POST" });
            const body = await response.json().catch(() => null);
            if (response.ok && body?.user?.id === auth.user.id) onChange({ phase: "signed-in", user: body.user, local: true });
            else setError("The local session could not be renewed. Your open drafts remain on this page.");
          } catch { setError("The local session could not be renewed. Try again."); }
          finally { setBusy(false); }
        }}>Renew local session</button>
        {error && <p className="auth-error" role="alert">{error}</p>}
      </section>
    );
  }
  if (auth.phase === "signed-in" && !reauth) {
    return (
      <section className="auth-card signed-in" aria-label="Account">
        <p>Signed in as <strong>{auth.user.email}</strong></p>
        <button className="secondary-button" type="button" onClick={() => setReauth(true)}>Sign in again without closing drafts</button>
        <button className="secondary-button" type="button" onClick={signOut} disabled={busy}>Sign out</button>
        {error && <p className="auth-error" role="alert">{error}</p>}
      </section>
    );
  }
  return (
    <section className="auth-card" aria-labelledby="auth-title">
      <h2 id="auth-title">Sign in to your workspace</h2>
      <form onSubmit={(event) => { event.preventDefault(); submit("login"); }}>
        <label>Email<input type="email" autoComplete="username" required readOnly={reauth} value={reauth && auth.phase === "signed-in" ? auth.user.email : email} onChange={(e) => setEmail(e.target.value)} /></label>
        <label>Password<input type="password" autoComplete="current-password" required minLength={15} maxLength={256} value={password} onChange={(e) => setPassword(e.target.value)} /></label>
        <div className="auth-actions">
          <button className="primary-button" type="submit" disabled={busy}>{busy ? "Working…" : "Sign in"}</button>
          {!reauth && <button className="secondary-button" type="button" disabled={busy} onClick={(event) => { if (event.currentTarget.form?.reportValidity()) submit("register"); }}>Create account</button>}
          {reauth && <button className="secondary-button" type="button" disabled={busy} onClick={() => { setPassword(""); setReauth(false); }}>Cancel</button>}
        </div>
        <p className="auth-hint">Passwords need at least 15 characters. No other rules.</p>
        {error && <p className="auth-error" role="alert">{error}</p>}
      </form>
    </section>
  );
}

type Workspace = { id: string; name: string; role: string };

const workspaceErrors: Record<string, string> = {
  invalid_name: "Use 1 to 120 characters.",
  unauthenticated: "Your session ended. Sign in again.",
  cross_origin_request: "The request was blocked as cross-origin.",
  unavailable: "The server cannot reach its database. Nothing was saved.",
};

function WorkspacesPanel({ nameInput, view, onWorkspaceChange, currentEmail, activeWorkspace, onDirectoryChange }: { currentEmail: string; nameInput: React.RefObject<HTMLInputElement | null>; view: string; activeWorkspace: Workspace | null; onWorkspaceChange: (workspace: Workspace) => void; onDirectoryChange: (workspaces: Workspace[]) => void }) {
  const filesSpace = activeWorkspace;
  const [reload, setReload] = useState(0);
  const [createdProject, setCreatedProject] = useState<Workspace | null>(null);
  const [summaries, setSummaries] = useState<Record<string, ProjectSummary>>({});
  const [detailsLoading, setDetailsLoading] = useState(true);
  const [detailsError, setDetailsError] = useState("");
  const [detailsReload, setDetailsReload] = useState(0);
  const mounted = useRef(false);
  const creating = useRef<AbortController | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; creating.current?.abort(); };
  }, []);
  const [selected, setSelected] = useState<Workspace | null>(null);
  const [items, setItems] = useState<Workspace[] | null>(null);
  const [loadError, setLoadError] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    fetch("/api/workspaces", { signal: controller.signal, cache: "no-store" })
      .then(async (response) => {
        const body = await response.json().catch(() => null);
        if (controller.signal.aborted) return;
        if (response.ok && Array.isArray(body?.workspaces)) {
          setItems(body.workspaces);
          onDirectoryChange(body.workspaces);
          try {
            const remembered = sessionStorage.getItem("aidrive:last-workspace");
            const workspace = body.workspaces.find((item: Workspace) => item.id === remembered);
            if (workspace) onWorkspaceChange(workspace);
          } catch {}
        }
        else setLoadError(true);
      })
      .catch((err) => {
        if (err?.name !== "AbortError") setLoadError(true);
      });
    return () => controller.abort();
  }, [onWorkspaceChange, onDirectoryChange, reload]);

  useEffect(() => { if (view === "create-space") nameInput.current?.focus(); }, [view, nameInput]);

  useEffect(() => {
    if (!items || !["spaces", "create-space", "overview"].includes(view)) return;
    if (view === "overview" && filesSpace) return;
    const controller = new AbortController();
    async function load() {
      setDetailsLoading(true);
      try {
        const response = await fetch("/api/workspaces/summaries", { cache: "no-store", signal: controller.signal });
        const body = await response.json().catch(() => null);
        if (!response.ok || !Array.isArray(body?.summaries) || !body.summaries.every(isProjectSummary)) throw Error();
        if (!controller.signal.aborted) {
          setSummaries(Object.fromEntries(body.summaries.map((item: ProjectSummary) => [item.id, item])));
          setDetailsError("");
        }
      } catch {
        if (!controller.signal.aborted) setDetailsError("Project details could not be refreshed. Available projects can still be opened.");
      } finally {
        if (!controller.signal.aborted) setDetailsLoading(false);
      }
    }
    void load();
    return () => controller.abort();
  }, [items, view, filesSpace, detailsReload]);

  function openProject(workspace: Workspace) {
    if (filesSpace?.id !== workspace.id && !confirmFileNavigation()) return;
    onWorkspaceChange(workspace);
    try { sessionStorage.setItem("aidrive:last-workspace", workspace.id); } catch {}
    window.location.hash = "panel-overview";
    requestAnimationFrame(() => document.getElementById("main-content")?.focus());
  }

  async function create() {
    const controller = new AbortController();
    creating.current = controller;
    setBusy(true);
    setError(null);
    try {
      const response = await fetch("/api/workspaces", {
        method: "POST",
        signal: controller.signal,
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ name }),
      });
      const body = await response.json().catch(() => null);
      if (!mounted.current || controller.signal.aborted) return;
      if (response.status === 201 && body?.workspace) {
        setItems((current) => [...(current ?? []), body.workspace]);
        onDirectoryChange([...(items ?? []), body.workspace]);
        setCreatedProject(body.workspace);
        setName("");
      } else {
        setError(workspaceErrors[body?.error] ?? "Something went wrong. Nothing was saved.");
      }
    } catch {
      if (mounted.current && !controller.signal.aborted) setError("Creation was not confirmed. Refresh projects before trying again.");
    } finally {
      if (creating.current === controller) creating.current = null;
      if (mounted.current) setBusy(false);
    }
  }

  return (
    <section className={styles.workspaces} data-open={!!filesSpace} data-view={view} id="spaces" aria-labelledby="spaces-title">
      <div className={styles.directory} hidden={!!filesSpace && !["spaces", "create-space"].includes(view)}>
        <ProjectGallery projects={items} summaries={summaries} detailsLoading={detailsLoading} detailsError={detailsError} onRetryDetails={() => setDetailsReload(value => value + 1)} activeId={filesSpace?.id} accessId={selected?.id} error={loadError}
          onRetry={() => { setLoadError(false); setItems(null); setReload(value => value + 1); }}
          onSelect={openProject}
          onAccess={workspace => { setSelected(selected?.id === workspace.id ? null : workspace); requestAnimationFrame(() => document.getElementById("project-access")?.scrollIntoView({ block: "start" })); }} />
        <form className={styles.createSpace} id="create-space" onSubmit={event => { event.preventDefault(); void create(); }}>
          <label htmlFor="space-name">New project<span>A separate workspace for its assistants and files.</span></label>
          <input id="space-name" ref={nameInput} required maxLength={120} placeholder="Project name" value={name} onChange={event => setName(event.target.value)} />
          <button className="primary-button" type="submit" disabled={busy}><Icon name="plus"/>{busy ? "Creating…" : "Create project"}</button>
          {error && <p className="auth-error" role="alert">{error}</p>}
        </form>
        {createdProject && <p className={styles.createdNotice} role="status">Project “{createdProject.name}” created.<button type="button" onClick={() => openProject(createdProject)}>Open project<Icon name="arrow"/></button></p>}
        {selected && <div id="project-access" className={styles.accessPanel}><AgentAccess key={selected.id} workspace={selected} /></div>}
      </div>
      {filesSpace && <div className={styles.workspaceBody}>
        <div className={styles.workspaceContext} hidden={["overview", "spaces", "create-space", "account"].includes(view)}><a href="#spaces" onClick={() => requestAnimationFrame(() => document.getElementById("main-content")?.focus())}><ProjectMark project={filesSpace}/><span><strong title={filesSpace.name}>{filesSpace.name}</strong><small>PROJECT / {filesSpace.id.slice(0,8)}</small></span><CaretUpDown size={16} aria-hidden="true"/></a><span className={styles.contextRole}>{filesSpace.role === "owner" ? "Owner" : filesSpace.role === "editor" ? "Can edit" : "Read only"}</span></div>

        <div hidden={view !== "overview"} id="panel-overview"><WorkspaceConsole workspace={filesSpace} projects={items ?? [filesSpace]} onProjectChange={openProject} currentEmail={currentEmail} /></div>
        <div hidden={view !== "map"}><MapPanel key={`map-${filesSpace.id}`} workspace={filesSpace} /></div>
        <div hidden={view !== "messages"} id="panel-messages"><MessagesPanel key={`m-${filesSpace.id}`} workspace={filesSpace} active={view === "messages"} /></div>
        <div hidden={view !== "review"} id="panel-review"><ReviewPanel key={`r-${filesSpace.id}`} workspace={filesSpace} /></div>
        <div hidden={view !== "knowledge"} id="panel-knowledge"><KnowledgePanel key={`k-${filesSpace.id}`} workspace={filesSpace} /></div>
        <div hidden={view !== "handoffs"} id="panel-handoffs"><HandoffsPanel key={`h-${filesSpace.id}`} workspace={filesSpace} /></div>
        <div hidden={view !== "files"} id="panel-files"><FilesPanel key={filesSpace.id} workspace={filesSpace} /></div>
        <div hidden={view !== "activity"} id="panel-activity"><ActivityPanel key={`a-${filesSpace.id}`} workspace={filesSpace} /></div>
        <div hidden={view !== "sessions"} id="panel-sessions"><SessionsPanel key={`s-${filesSpace.id}`} workspace={filesSpace} /></div>
      </div>}
    </section>
  );
}

export default function Home() {
  const [apiStatus, setApiStatus] = useState<ApiStatus>("checking");
  const [auth, setAuth] = useState<AuthState>({ phase: "loading" });
  const spaceNameInput = useRef<HTMLInputElement>(null);
  const [view, setView] = useState("overview");
  const [activeWorkspace, setActiveWorkspace] = useState<Workspace | null>(null);
  const [availableWorkspaces, setAvailableWorkspaces] = useState<Workspace[]>([]);
  const focusOnView = useRef<{ view: string; id: string } | null>(null);
  useEffect(() => {
    if (focusOnView.current?.view === view) {
      document.getElementById(focusOnView.current.id)?.focus();
      focusOnView.current = null;
    }
  }, [view]);
  useEffect(() => {
    const navigate = () => {
      const hash = window.location.hash.slice(1).replace(/^panel-/, "");
      if (hash === "main-content") return;
      setView(["overview", "messages", "spaces", "create-space", "handoffs", "knowledge", "account", "map", "review", "files", "activity", "sessions"].includes(hash) ? hash : "overview");
    };
    navigate();
    window.addEventListener("hashchange", navigate);
    return () => window.removeEventListener("hashchange", navigate);
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    const options = { signal: controller.signal, cache: "no-store" as const };
    (async () => {
      const response = await fetch("/api/auth/session", options);
      const body = await response.json().catch(() => null);
      const mode = await fetch("/api/local/mode", options).then((r) => r.json()).catch(() => null);
      const local = mode?.local === true;
      if (response.ok && body?.user) return setAuth({ phase: "signed-in", user: body.user, local });
      if (response.status !== 401) return setAuth({ phase: "unavailable" });
      if (!local) return setAuth({ phase: "signed-out" });
      // Local install: opening the page is the sign-in.
      const opened = await fetch("/api/local/session", { ...options, method: "POST" });
      const session = await opened.json().catch(() => null);
      if (opened.ok && session?.user) return setAuth({ phase: "signed-in", user: session.user, local: true });
      setAuth({
        phase: "unavailable",
        reason: session?.error === "not_single_person_install"
          ? "This install has more than one account, so it does not sign anyone in automatically."
          : "This computer could not open your space. Open it as http://localhost:3000 and reload.",
      });
    })().catch((error) => {
      if (error?.name !== "AbortError") setAuth({ phase: "unavailable" });
    });
    return () => controller.abort();
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    fetch("/api/readyz", { signal: controller.signal, cache: "no-store" })
      .then(async (response) => {
        if (response.ok) return setApiStatus("ready");
        // A 503 body from our API means the process is up but a dependency is not;
        // anything else (proxy error, no body) means the API itself is unreachable.
        const body = await response.json().catch(() => null);
        setApiStatus(body?.service === "aidrive-api" ? "degraded" : "offline");
      })
      .catch((error) => {
        if (error?.name !== "AbortError") setApiStatus("offline");
      });
    return () => controller.abort();
  }, []);

  return (
    <main className={styles.shell} data-active={!!activeWorkspace} data-signed-in={auth.phase === "signed-in"} data-view={view} id="home">
      <a className={styles.skipLink} href="#main-content" onClick={event => { event.preventDefault(); document.getElementById("main-content")?.focus(); }}>Skip to content</a>
      <aside className={styles.sidebar} aria-label="AIDrive navigation">
        <a className={styles.brand} href="#panel-overview" aria-label="AIDrive home"><span className={styles.brandMark}><ChartBar size={25} weight="fill" aria-hidden="true" /></span>AIDrive</a>
        <nav className={styles.navigation} aria-label="Main navigation">
          {[["Workspace", "overview", "home"], ["Conversations", "messages", "chat"], ["Projects", "spaces", "drive"], ["Handoff", "handoffs", "handoff"], ["Memory", "knowledge", "memory"], ["Account", "account", "settings"]].map(([label, id, icon]) => <a key={id} href={id === "spaces" || id === "account" ? `#${id}` : `#panel-${id}`} aria-current={view === id ? "page" : undefined}><Icon name={icon}/><span>{label}</span></a>)}
          <details className={styles.moreNavigation}><summary>More tools</summary>{[["Map", "map", "map"], ["Review", "review", "review"], ["Files", "files", "file"], ["Activity", "activity", "activity"], ["Sessions", "sessions", "sessions"]].map(([label,id,icon]) => <a key={id} href={`#panel-${id}`} aria-current={view === id ? "page" : undefined}><Icon name={icon}/>{label}</a>)}</details>
        </nav>
        <div className={styles.sidebarBottom}>
          <details className={styles.projectSwitcher}>
            <summary className={styles.projectLink}>{activeWorkspace ? <ProjectMark project={activeWorkspace} size="small"/> : <Icon name="drive"/>}<span><strong title={activeWorkspace?.name}>{activeWorkspace?.name || "Choose a project"}</strong><small>Switch project</small></span><CaretUpDown size={15} aria-hidden="true"/></summary>
            <div className={styles.switcherMenu}>
              <p>Your projects</p>
              {availableWorkspaces.map(workspace => <button key={workspace.id} type="button" aria-label={`Switch to project ${workspace.name} · ${workspace.id.slice(0, 8)}`} aria-pressed={activeWorkspace?.id === workspace.id} onClick={event => {
                if (activeWorkspace?.id !== workspace.id && !confirmFileNavigation()) return;
                setActiveWorkspace(workspace);
                try { sessionStorage.setItem("aidrive:last-workspace", workspace.id); } catch {}
                window.location.hash = "panel-overview";
                const disclosure = event.currentTarget.closest("details");
                disclosure?.removeAttribute("open");
                disclosure?.querySelector("summary")?.focus();
              }}><ProjectMark project={workspace} /><span><strong>{workspace.name}</strong><small>{workspace.id.slice(0,8)}</small></span>{activeWorkspace?.id === workspace.id && <Check size={16} aria-hidden="true"/>}</button>)}
              <a href="#spaces" onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); if (view === "spaces") document.getElementById("spaces-title")?.focus(); else focusOnView.current = { view: "spaces", id: "spaces-title" }; }}>Browse all projects<Icon name="arrow"/></a>
              <a href="#create-space" onClick={event => { event.currentTarget.closest("details")?.removeAttribute("open"); if (view === "create-space") spaceNameInput.current?.focus(); else focusOnView.current = { view: "create-space", id: "space-name" }; }}>New project<Icon name="plus"/></a>
            </div>
          </details>
          <div className={styles.date}><Icon name="calendar"/><time>{new Intl.DateTimeFormat("en-CA").format(new Date())}</time></div>
        </div>
      </aside>
      <section className={styles.mainArea} aria-label="Mission control">
        <header className={styles.topbar}>
          <div className={styles.breadcrumbs}><span>Workspace console</span><span>/</span><strong>Mission control</strong></div>
          <div className={styles.apiStatus} data-status={apiStatus} role="status"><span/>{apiLabels[apiStatus]}</div>
        </header>
        <div className={styles.content} id="main-content" tabIndex={-1}>
          <header className={styles.intro}>
            <div><p className={styles.eyebrow}><span/> AIDRIVE MISSION CONTROL</p><h1>Your project.<br/><span>Every assistant, in view.</span></h1><p className={styles.subtitle}>Your spaces, connected assistants and project context.</p></div>
            <div className={styles.introAction}>{auth.phase === "signed-in" ? <button className="primary-button" type="button" onClick={() => spaceNameInput.current?.focus()}><Icon name="plus"/>New space</button> : <a className={styles.signInLink} href="#account">Open your account <Icon name="arrow"/></a>}</div>
            <div className={styles.introLines} aria-hidden="true"><span/><span/><span/></div>
          </header>
          <div id="account" className={styles.account} hidden={auth.phase === "signed-in" && view !== "account"}><AuthPanel auth={auth} onChange={next => { setAuth(next); if (next.phase !== "signed-in") { setActiveWorkspace(null); setAvailableWorkspaces([]); } }} /></div>
          {auth.phase === "signed-in" && <WorkspacesPanel key={auth.user.id} nameInput={spaceNameInput} currentEmail={auth.user.email} view={view} activeWorkspace={activeWorkspace} onDirectoryChange={setAvailableWorkspaces} onWorkspaceChange={setActiveWorkspace} />}
          <footer className={styles.footer}><span>AIDrive</span><span>Your work. Your context. Your control.</span></footer>
        </div>
      </section>
    </main>
  );
}
