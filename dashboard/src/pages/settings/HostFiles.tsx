import { useQuery } from '@tanstack/react-query';
import axios from 'axios';
import { Base64 } from 'js-base64';
import { useState } from 'react';
import { useDocumentTitle } from 'usehooks-ts';
import { ClientFile } from 'bindings/ClientFile';
import Button from 'components/Atoms/Button';
import { formatFileDate, sortFiles } from 'components/FileViewer/FileList';
import { isInsideRoot, joinPath, normalizePath, parentPath } from 'data/hostPath';
import { errorToString } from 'utils/util';

interface FsRoot {
  path: string;
}

const textLimit = 262144;

function encodePath(path: string): string {
  return Base64.encode(path, true);
}

export default function HostFiles() {
  useDocumentTitle('Host files - Lodestone');
  const [chosen, setChosen] = useState<string | null>(null);
  const [opened, setOpened] = useState<ClientFile | null>(null);
  const rootQuery = useQuery<FsRoot>(
    ['host-files', 'root'],
    () => axios.get<FsRoot>('/fs/root').then((response) => response.data),
    { retry: false }
  );
  const root = rootQuery.data?.path ?? '';
  const current =
    root && chosen && isInsideRoot(root, chosen) ? normalizePath(chosen) : root;

  const listQuery = useQuery<ClientFile[]>(
    ['host-files', 'list', current],
    () =>
      axios
        .get<ClientFile[]>(`/fs/${encodePath(current)}/ls`)
        .then((response) => sortFiles(response.data, 'name', 'asc')),
    { enabled: current.length > 0, retry: false }
  );

  const canPreview =
    opened?.file_type === 'File' && (opened.size ?? 0) <= textLimit;
  const contentQuery = useQuery<string>(
    ['host-files', 'read', opened?.path],
    () =>
      axios
        .get<string>(`/fs/${encodePath(opened?.path ?? '')}/read`, {
          transformResponse: (data) => data,
        })
        .then((response) => response.data),
    { enabled: Boolean(canPreview && opened && isInsideRoot(root, opened.path)), retry: false }
  );

  const openEntry = (file: ClientFile) => {
    const next = isInsideRoot(root, file.path)
      ? normalizePath(file.path)
      : joinPath(current, file.name);
    if (!isInsideRoot(root, next)) return;
    if (file.file_type === 'Directory') {
      setOpened(null);
      setChosen(next);
      return;
    }
    setOpened({ ...file, path: next });
  };

  const relative = root && current.startsWith(root) ? current.slice(root.length) : '';
  const crumbs = relative.replace(/^[/\\]+/, '').split(/[/\\]/).filter(Boolean);

  return (
    <div className="relative mx-auto flex h-full w-full max-w-4xl flex-col px-4 pt-8 pb-10">
      <h1 className="dashboard-instance-heading">Host files</h1>
      <p className="mt-2 text-medium text-white/50">
        Files in the Lodestone data directory. This screen does not leave that
        directory. Safe mode lets only the owner open it.
      </p>
      {rootQuery.isError && (
        <p className="mt-4 text-medium text-red-200">
          {errorToString(rootQuery.error)}
        </p>
      )}
      {root && (
        <div className="mt-4 flex flex-row flex-wrap items-center gap-2 text-small text-gray-300">
          <Button
            size="slim"
            label="Data directory"
            intention="info"
            onClick={() => {
              setChosen(root);
              setOpened(null);
            }}
          />
          {crumbs.map((crumb, index) => {
            const target = joinPath(root, crumbs.slice(0, index + 1).join('/'));
            return (
              <Button
                key={target}
                size="slim"
                label={crumb}
                intention="info"
                onClick={() => {
                  setChosen(target);
                  setOpened(null);
                }}
              />
            );
          })}
          {current !== normalizePath(root) && (
            <Button
              size="slim"
              label="Up"
              intention="info"
              onClick={() => {
                const parent = parentPath(current);
                if (isInsideRoot(root, parent)) {
                  setChosen(parent);
                  setOpened(null);
                }
              }}
            />
          )}
        </div>
      )}
      {listQuery.isError && (
        <p className="mt-4 text-medium text-red-200">
          {errorToString(listQuery.error)}
        </p>
      )}
      <ul className="mt-4 flex flex-col divide-y divide-gray-faded/30 overflow-y-auto rounded-lg border border-gray-faded/30">
        {(listQuery.data ?? []).map((file) => (
          <li key={file.path}>
            <button
              type="button"
              className="flex w-full flex-row items-center gap-4 px-4 py-2 text-left hover:bg-gray-800"
              onClick={() => openEntry(file)}
            >
              <span className="min-w-0 grow truncate text-medium text-gray-300">
                {file.file_type === 'Directory' ? `${file.name}/` : file.name}
              </span>
              <span className="hidden text-small text-white/50 @xs:inline">
                {formatFileDate(file.modification_time ?? file.creation_time)}
              </span>
            </button>
          </li>
        ))}
        {listQuery.isSuccess && listQuery.data.length === 0 && (
          <li className="px-4 py-3 text-medium text-white/50">This directory is empty.</li>
        )}
      </ul>
      {opened && (
        <div className="mt-4">
          <h2 className="text-h3 font-bold text-gray-300">{opened.name}</h2>
          {!canPreview && (
            <p className="mt-2 text-medium text-white/50">
              This file is too large or not opened as text here.
            </p>
          )}
          {contentQuery.isError && (
            <p className="mt-2 text-medium text-red-200">
              {errorToString(contentQuery.error)}
            </p>
          )}
          {contentQuery.data != null && (
            <pre className="mt-2 max-h-96 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-gray-900 p-4 font-mono text-small text-gray-300">
              {contentQuery.data}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
