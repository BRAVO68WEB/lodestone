export interface FolderGroup<T> {
  folder: string;
  items: T[];
}

/** Empty folder stays first. Other names are alphabetical. Items are alphabetical. */
export function groupInstances<T extends { folder?: string | null; name: string }>(
  instances: T[]
): FolderGroup<T>[] {
  const groups = new Map<string, T[]>();
  for (const instance of instances) {
    const folder = (instance.folder ?? '').trim();
    const list = groups.get(folder) ?? [];
    list.push(instance);
    groups.set(folder, list);
  }
  return [...groups.keys()]
    .sort((left, right) => {
      if (left === right) return 0;
      if (left === '') return -1;
      if (right === '') return 1;
      return left.localeCompare(right, undefined, { sensitivity: 'base' });
    })
    .map((folder) => ({
      folder,
      items: (groups.get(folder) ?? []).sort((left, right) =>
        left.name.localeCompare(right.name, undefined, { sensitivity: 'base' })
      ),
    }));
}
