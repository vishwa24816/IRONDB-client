// Inline-SVG port of DatabaseIcons.tsx color mapping (no React).
// Shapes: single-letter glyph in engine color — textContent-safe, no innerHTML by callers.
export const DB_COLORS = {
  mysql: "#00758F", mariadb: "#003545", oceanbase: "#0052CC", postgres: "#336791",
  redis: "#DC382D", mongodb: "#47A248", elasticsearch: "#FEC514", kingbase: "#1890FF",
  dameng: "#E6002D", oracle: "#F80000", sqlserver: "#CC2927", clickhouse: "#FFBF00",
  trino: "#DD00A1", sqlite: "#003B57", duckdb: "#FFC107", opengauss: "#2446A8",
  kafka: "#F97316", rabbitmq: "#FF6B35", custom: "#64748B",
};

export function getDbColor(type) {
  return DB_COLORS[String(type ?? "").toLowerCase()] ?? DB_COLORS.custom;
}

function svgBadge(letter, color, size) {
  const s = Number(size) || 16;
  const esc = String(letter).slice(0, 1).toUpperCase().replace(/[<>&"]/g, "");
  return `<svg width="${s}" height="${s}" viewBox="0 0 16 16" aria-hidden="true"><rect x="1" y="1" width="14" height="14" rx="3" fill="${color}"/><text x="8" y="11.5" font-size="9" font-weight="700" fill="#fff" text-anchor="middle" font-family="system-ui,sans-serif">${esc}</text></svg>`;
}

export function dbIcon(type, size = 16) {
  const key = String(type ?? "custom").toLowerCase();
  return svgBadge(key.slice(0, 1), getDbColor(key), size);
}
