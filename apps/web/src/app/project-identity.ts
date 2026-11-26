const palettes = [
  { palette: "jade", accent: "#a2d0b2", surface: "#254b39", cover: "jade" },
  { palette: "copper", accent: "#d6b097", surface: "#634c3c", cover: "copper" },
  { palette: "slate", accent: "#b4cbd8", surface: "#354d5b", cover: "slate" },
  { palette: "iris", accent: "#c4b7dd", surface: "#4c405e", cover: "slate" },
  { palette: "sand", accent: "#d3c6a6", surface: "#5f5640", cover: "copper" },
  { palette: "rose", accent: "#dbb8b6", surface: "#654848", cover: "copper" },
];

export function projectIdentity(project: { id: string; name: string }) {
  let hash = 0;
  for (const character of project.id) hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  const words = project.name.match(/[\p{L}\p{N}]+/gu) ?? [];
  return { ...palettes[hash % palettes.length], initials: words.slice(0, 2).map(word => Array.from(word)[0]).join("").toLocaleUpperCase() || "P" };
}
