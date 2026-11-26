"use client";
import styles from "./agent-scene.module.css";
export const agentProviders = [{ client: "claude", name: "Claude Code" }, { client: "codex", name: "Codex" }, { client: "opencode", name: "OpenCode" }];
export function agentLabel(client: string) { return agentProviders.find(provider => provider.client === client)?.name ?? client; }
type Props = { chats: { client: string }[]; client: string; loading: boolean; unavailable: boolean; onSelect: (client: string) => void };
export default function AgentScene({ chats, client, loading, unavailable, onSelect }: Props) {
  const providers = [...agentProviders, ...Array.from(new Set(chats.map(chat => chat.client))).filter(value => !agentProviders.some(provider => provider.client === value)).map(value => ({ client: value, name: value }))];
  return <div className={styles.scene} role="group" aria-label="Choose an assistant">
    {providers.map(provider => {
      const count = chats.filter(chat => chat.client === provider.client).length;
      return <div key={provider.client} className={styles[provider.client] ?? styles.other}>
        <button type="button" data-agent-client={provider.client} aria-pressed={client === provider.client} onClick={() => onSelect(provider.client)}>
          <strong>{provider.name}</strong><span>{loading ? "Checking chats…" : unavailable ? "Registry unavailable" : `${count} registered ${count === 1 ? "chat" : "chats"} in this project`}</span>
        </button>
      </div>;
    })}
  </div>;
}
