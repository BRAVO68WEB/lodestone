/** Collapse `.` and `..` and use forward slashes. */
export function normalizePath(path: string): string {
  const slash = path.replace(/\\/g, '/');
  const absolute = slash.startsWith('/');
  const parts: string[] = [];
  for (const part of slash.split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') {
      parts.pop();
      continue;
    }
    parts.push(part);
  }
  if (absolute) return `/${parts.join('/')}` || '/';
  return parts.join('/') || '.';
}

/** True when `candidate` is `root` or a directory inside it. `root/../etc` is not inside. */
export function isInsideRoot(root: string, candidate: string): boolean {
  const base = normalizePath(root);
  const next = normalizePath(candidate);
  if (next === base) return true;
  const prefix = base.endsWith('/') ? base : `${base}/`;
  return next.startsWith(prefix);
}

export function parentPath(path: string): string {
  const normal = normalizePath(path);
  const slash = normal.lastIndexOf('/');
  if (slash <= 0) return normal.startsWith('/') ? '/' : normal;
  return normal.slice(0, slash);
}

export function joinPath(parent: string, name: string): string {
  const base = parent.endsWith('/') ? parent.slice(0, -1) : parent;
  return normalizePath(`${base}/${name}`);
}
