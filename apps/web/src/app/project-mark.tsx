import { type CSSProperties } from "react";
import { projectIdentity } from "./project-identity";
import styles from "./project-mark.module.css";

export default function ProjectMark({ project, size = "medium" }: { project: { id: string; name: string }; size?: "small" | "medium" | "large" }) {
  const identity = projectIdentity(project);
  return <span className={styles.mark} data-size={size} aria-hidden="true" style={{ "--project-accent": identity.accent, "--project-surface": identity.surface } as CSSProperties}>{identity.initials}</span>;
}
