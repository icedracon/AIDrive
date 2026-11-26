"use client";

import { useState, type CSSProperties } from "react";
import { ArrowUpRight, ArrowsClockwise, ChatsCircle, Check, FileText, FolderSimple, MagnifyingGlass, Plus, SlidersHorizontal } from "@phosphor-icons/react";
import { projectIdentity } from "./project-identity";
import ProjectMark from "./project-mark";
import styles from "./project-gallery.module.css";

export type Project = { id: string; name: string; role: string };
export type ProjectSummary = Project & { file_count: number; registered_chat_count: number; last_visible_activity_at: string | null };

export function isProjectSummary(value: unknown): value is ProjectSummary {
  if (!value || typeof value !== "object") return false;
  const item = value as Record<string, unknown>;
  return typeof item.id === "string" && !!item.id && typeof item.name === "string"
    && ["owner", "editor", "viewer"].includes(String(item.role))
    && typeof item.file_count === "number" && Number.isSafeInteger(item.file_count) && item.file_count >= 0
    && typeof item.registered_chat_count === "number" && Number.isSafeInteger(item.registered_chat_count) && item.registered_chat_count >= 0
    && (item.last_visible_activity_at === null || (typeof item.last_visible_activity_at === "string" && Number.isFinite(Date.parse(item.last_visible_activity_at))));
}

export default function ProjectGallery({ projects, summaries, detailsLoading, detailsError, activeId, accessId, error, onSelect, onAccess, onRetry, onRetryDetails }: {
  projects: Project[] | null;
  summaries: Record<string, ProjectSummary>;
  detailsLoading: boolean;
  detailsError: string;
  activeId?: string;
  accessId?: string;
  error: boolean;
  onSelect: (project: Project) => void;
  onAccess: (project: Project) => void;
  onRetry: () => void;
  onRetryDetails: () => void;
}) {
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState("all");
  const [order, setOrder] = useState("asc");
  const matching = (projects ?? []).filter(project => (scope !== "owned" || project.role === "owner") && `${project.name} ${project.id}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())).sort((a, b) => (order === "asc" ? 1 : -1) * (a.name.localeCompare(b.name) || a.id.localeCompare(b.id)));

  return <div className={styles.gallery}>
    <header className={styles.heading}>
      <div><p className={styles.eyebrow}>YOUR WORK, IN ITS OWN SPACE</p><h2 id="spaces-title" tabIndex={-1}>Choose a project<span>.</span></h2><p className={styles.description}>A home for every idea. Pick up where you left off.</p></div>
      <a className={styles.newProject} href="#create-space"><Plus size={18} aria-hidden="true"/>New project</a>
    </header>
    <div className={styles.toolbar}>
      <label className={styles.search}><MagnifyingGlass size={19} aria-hidden="true"/><span className={styles.srOnly}>Search projects</span><input type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Find a project…" /></label>
      <div className={styles.filters} role="group" aria-label="Project ownership"><button type="button" aria-pressed={scope === "all"} onClick={() => setScope("all")}>All projects</button><button type="button" aria-pressed={scope === "owned"} onClick={() => setScope("owned")}>Owned by you</button></div>
      <label className={styles.sort}><SlidersHorizontal size={17} aria-hidden="true"/><span className={styles.srOnly}>Sort projects</span><select value={order} onChange={event => setOrder(event.target.value)}><option value="asc">Name A–Z</option><option value="desc">Name Z–A</option></select></label>
      <button className={styles.refresh} type="button" onClick={onRetry} aria-label="Refresh projects"><ArrowsClockwise size={18} aria-hidden="true"/></button>
    </div>
    {detailsError && <p className={styles.detailsState} role="status">{detailsError}<button type="button" onClick={onRetryDetails}>Retry details</button></p>}
    {error ? <div className={styles.notice} role="alert"><h3>Projects could not be loaded.</h3><p>Check your connection and try again.</p><button type="button" onClick={onRetry}>Try again</button></div>
      : projects === null ? <p className={styles.notice} role="status">Loading your projects…</p>
      : <>
        <p className={styles.resultCount} role="status">{matching.length} {matching.length === 1 ? "project" : "projects"}{query || scope !== "all" ? ` of ${projects.length}` : ""}</p>
        {matching.length === 0 ? <div className={styles.notice}><FolderSimple size={30} aria-hidden="true"/><h3>{projects.length ? "No matching projects." : "Make room for your next idea."}</h3><p>{projects.length ? "Try another name or search by project ID." : "Create a project to bring its assistants, files and decisions together."}</p>{projects.length > 0 && <button type="button" onClick={() => { setQuery(""); setScope("all"); }}>Clear filters</button>}</div>
          : <ul className={styles.grid}>{matching.map(project => {
            const identity = projectIdentity(project);
            const active = project.id === activeId;
            const summary = summaries[project.id];
            return <li key={project.id} className={styles.card} data-palette={identity.palette} data-active={active} style={{ "--accent": identity.accent } as CSSProperties}>
              <button type="button" className={styles.open} onClick={() => onSelect(project)} aria-label={`Open project ${project.name} · ${project.id.slice(0, 8)}`} aria-pressed={active}>
                <div className={styles.cover} data-cover={identity.cover}><span className={styles.projectEmblem}><ProjectMark project={project} size="large"/></span><span className={styles.coverLabel}>{active ? <><Check size={14} aria-hidden="true"/>Current project</> : <><FolderSimple size={15} aria-hidden="true"/>Workspace</>}</span><ArrowUpRight className={styles.openIcon} size={22} aria-hidden="true"/></div>
                <div className={styles.cardBody}><span className={styles.projectId}>PROJECT / {project.id.slice(0, 8)}</span><h3 title={project.name}>{project.name}</h3>
                  <div className={styles.cardMeta} aria-busy={detailsLoading && !summary}>{summary ? <>
                    <div className={styles.counts}><span><FileText size={14} aria-hidden="true"/>{summary.file_count} {summary.file_count === 1 ? "file" : "files"}</span><span><ChatsCircle size={14} aria-hidden="true"/>{summary.registered_chat_count} registered {summary.registered_chat_count === 1 ? "chat" : "chats"}</span></div>
                    <span className={styles.activity}>{summary.last_visible_activity_at ? <>Visible activity · <time dateTime={summary.last_visible_activity_at} title={new Date(summary.last_visible_activity_at).toLocaleString()}>{new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }).format(new Date(summary.last_visible_activity_at))}</time></> : "No visible activity yet"}</span>
                  </> : <span className={styles.activity}>{detailsLoading ? "Loading project details…" : "Project details unavailable"}</span>}</div>
                  <div className={styles.cardFooter}><span>{project.role === "owner" ? "Owner" : project.role === "editor" ? "Can edit" : "Read only"}</span><span className={styles.openLabel}>{active ? "Continue working" : "Open project"}<ArrowUpRight size={16} aria-hidden="true"/></span></div></div>
              </button>
              {project.role === "owner" && <button type="button" className={styles.access} aria-expanded={accessId === project.id} aria-label={`Manage AI access for ${project.name} · ${project.id.slice(0, 8)}`} onClick={() => onAccess(project)}>Manage AI access<ArrowUpRight size={13} aria-hidden="true"/></button>}
            </li>;
          })}</ul>}
      </>}
  </div>;
}
