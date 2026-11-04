import { faCircle, faServer } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import axios from 'axios';
import { ConsoleEvent, useConsoleStream } from 'data/ConsoleStream';
import { InstanceContext } from 'data/InstanceContext';
import { CommandHistoryContext } from 'data/CommandHistoryContext';
import { useUserAuthorized } from 'data/UserInfo';
import Tooltip from 'rc-tooltip';
import { useContext, useEffect } from 'react';
import { useRef, useState } from 'react';
import { DISABLE_AUTOFILL, errorToString } from 'utils/util';
import { renderConsoleText } from 'utils/ansi';
import ErrorGraphic from './ErrorGraphic';
import { useDocumentTitle } from 'usehooks-ts';
import Button from './Atoms/Button';
import Checkbox from './Atoms/Checkbox';
import { toast } from 'react-toastify';

const autoScrollThreshold = 10;
const consoleWindow = 300;

export default function GameConsole() {
  useDocumentTitle('Instance Console - Lodestone');
  const { selectedInstance: instance } = useContext(InstanceContext);
  if (!instance) throw new Error('No instance selected');
  const uuid = instance.uuid;
  const canAccessConsole = useUserAuthorized(
    'can_access_instance_console',
    uuid
  );
  const defaultFilters = {
    "PlayerMessage": true, 
    "SystemMessage": true, 
    "InstanceOutput": true
  }
  const { consoleLog, consoleStatus, clearConsoleLog, loadOlder } = useConsoleStream(uuid);
  const [command, setCommand] = useState('');
  const [filter, setFilter] = useState('');
  const [regex, setRegex] = useState(true);
  const { commandHistory, appendCommandHistory } = useContext(
    CommandHistoryContext
  );
  const [commandNav, setCommandNav] = useState(commandHistory.length);
  const [shownLines, setShownLines] = useState(consoleWindow);
  const [historyExhausted, setHistoryExhausted] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const listRef = useRef<HTMLOListElement>(null);
  const pinnedToBottom = useRef(true);

  useEffect(() => {
    setShownLines(consoleWindow);
    setHistoryExhausted(false);
  }, [uuid]);

  const scrollToBottom = () => {
    if (listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  };

  const onConsoleScroll = () => {
    const list = listRef.current;
    if (!list) return;
    pinnedToBottom.current =
      list.scrollHeight - list.scrollTop - list.clientHeight <
      autoScrollThreshold;
  };

  // Open on the latest line. Keep following new lines only while the user
  // has not scrolled up.
  useEffect(() => {
    if (pinnedToBottom.current) scrollToBottom();
  }, [consoleLog]);

  const clearServerConsole = async () => {
    try {
      await axios.delete(`/instance/${uuid}/console/buffer`);
      clearConsoleLog();
      setShownLines(consoleWindow);
      setHistoryExhausted(true);
    } catch (err) {
      toast.error(errorToString(err));
    }
  };

  const sendCommand = (command: string) => {
    axios({
      method: 'post',
      url: `/instance/${uuid}/console`,
      data: JSON.stringify(command),
      headers: {
        'Content-Type': 'application/json',
      },
    });
    scrollToBottom();
  };

  let consoleStatusMessage = '';
  let consoleStatusColor = 'text-gray-500';
  switch (consoleStatus) {
    case 'no-permission':
      consoleStatusMessage = 'No permission to access console';
      consoleStatusColor = 'text-gray-500';
      break;
    case 'loading':
      consoleStatusMessage = 'Loading console...';
      consoleStatusColor = 'text-gray-500';
      break;
    case 'buffered':
      consoleStatusMessage = 'History messages. No live updates';
      consoleStatusColor = 'text-gray-500';
      break;
    case 'live':
      consoleStatusMessage = 'Console is live';
      consoleStatusColor = 'text-green-200';
      break;
    case 'live-no-buffer':
      consoleStatusMessage =
        'Console is live but failed to fetch history. Your internet connection may be unstable';
      consoleStatusColor = 'text-yellow';
      break;
    case 'closed':
      consoleStatusMessage = 'Console is closed';
      consoleStatusColor = 'text-red-200';
      break;
    case 'error':
      consoleStatusMessage = 'Connection lost or error';
      consoleStatusColor = 'text-red-200';
  }
  // overwrites
  if (instance.state === 'Stopped') {
    consoleStatusMessage = `Instance is ${instance.state.toLowerCase()}`;
    consoleStatusColor = 'text-gray-500';
  }

  let consoleInputMessage = '';
  if (!canAccessConsole || consoleStatus === 'no-permission')
    consoleInputMessage = 'No permission';
  else if (instance.state === 'Stopped')
    consoleInputMessage = `Instance is ${instance.state.toLowerCase()}`;
  else if (consoleStatus === 'closed')
    consoleInputMessage = 'Console is closed';

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowUp') {
      setCommandNav((prev) => {
        prev = Math.max(prev - 1, 0);
        setCommand(commandHistory[prev]);
        return prev;
      });
    } else if (event.key === 'ArrowDown') {
      setCommandNav((prev) => {
        prev = Math.min(prev + 1, commandHistory.length - 1);
        setCommand(commandHistory[prev]);
        return prev;
      });
    } else {
      setCommandNav(commandHistory.length);
    }
  };

  const filterMessage = (line: ConsoleEvent) => {
    if (filter == '') return true;
    if (regex) {
      try {
        const filterRegex = new RegExp(filter, "i");
        if (line.message.match(filterRegex)) return true;
      } catch (e) {
        console.log("regex invalid!!!");
      }
    } else {
      if (line.message.toLowerCase().indexOf(filter.toLowerCase()) != -1) return true;
    }
    return false;
  }

  const checkRegex = (checked: boolean) => {
  setRegex(checked)
  }

  const filteredLog = consoleLog.filter(filterMessage);
  const hiddenLines = Math.max(0, filteredLog.length - shownLines);
  const visibleLog = filteredLog.slice(hiddenLines);
  const canShowOlder = hiddenLines > 0 || (!historyExhausted && consoleLog.length > 0);

  const showOlder = async () => {
    pinnedToBottom.current = false;
    if (hiddenLines > 0) {
      setShownLines((count) => count + consoleWindow);
      return;
    }
    if (historyExhausted || loadingOlder || consoleLog.length === 0) return;
    const list = listRef.current;
    const previousHeight = list?.scrollHeight ?? 0;
    const previousTop = list?.scrollTop ?? 0;
    setLoadingOlder(true);
    try {
      const page = await loadOlder(consoleLog[0].snowflake);
      if (page.exhausted || page.added === 0) setHistoryExhausted(true);
      setShownLines((count) => count + Math.max(page.added, 1));
      requestAnimationFrame(() => {
        const node = listRef.current;
        if (!node) return;
        node.scrollTop = previousTop + (node.scrollHeight - previousHeight);
      });
    } catch (err) {
      toast.error(errorToString(err));
    } finally {
      setLoadingOlder(false);
    }
  };

  return (
    <div className="relative flex h-full w-full grow flex-col pt-0">
      <div className="mb-3 flex w-full flex-row items-center justify-between gap-2 font-mono text-small">
            <input
              className="w-full rounded-lg bg-gray-850 py-1 px-4 text-gray-300 outline-white/50 placeholder:text-gray-500 focus-visible:outline focus:outline-none disabled:placeholder:text-gray-500"
              placeholder={'Search console...'}
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              id="search"
              type="text"
            />
          <Checkbox label='Regex' checked={regex} onChange={checkRegex} className="ml-3"/>
          <Button label="Clear" intention="info" onClick={clearServerConsole} />
      </div>
      <div className="relative flex h-full w-full grow flex-col rounded-lg border border-gray-faded/30">
        <Tooltip
          overlay={<span>{consoleStatusMessage}</span>}
          placement="bottom"
          showArrow={false}
          trigger={['hover']}
          mouseEnterDelay={0}
        >
          <FontAwesomeIcon
            icon={faCircle}
            className={`absolute top-1 right-1 select-none p-1.5 text-small ${consoleStatusColor}`}
          />
        </Tooltip>
        {!canAccessConsole || consoleStatus === 'no-permission' ? (
          <ErrorGraphic
            icon={faServer}
            message="You don't have permission to access this console"
            className="border-b border-gray-faded/30"
            iconClassName="text-gray-400"
            messageClassName="text-white/50"
          />
        ) : consoleLog.length === 0 ? (
          <ErrorGraphic
            icon={faServer}
            message="No console messages yet"
            className="border-b border-gray-faded/30"
            iconClassName="text-gray-400"
            messageClassName="text-white/50"
          />
        ) : (
          <ol
            className="font-light flex h-0 grow rounded-t-lg flex-col overflow-y-auto whitespace-pre-wrap break-words border-b border-gray-faded/30 bg-gray-900 py-3 font-mono text-small tracking-tight text-gray-300"
            ref={listRef}
            onScroll={onConsoleScroll}
          >
            {canShowOlder && (
              <li className="px-4 py-1">
                <button
                  type="button"
                  className="text-blue-200 hover:underline"
                  disabled={loadingOlder}
                  onClick={() => {
                    void showOlder();
                  }}
                >
                  {loadingOlder
                    ? 'Loading older...'
                    : hiddenLines > 0
                    ? `Show older (${hiddenLines})`
                    : 'Show older'}
                </button>
              </li>
            )}
            {visibleLog.map((line) => (
              <li
                key={line.snowflake}
                className="py-[0.125rem] px-4 hover:bg-gray-800"
              >
                {renderConsoleText(line.message)}
              </li>
            ))}
          </ol>
        )}
        <div className="font-mono text-small">
          <form
            noValidate
            autoComplete={DISABLE_AUTOFILL}
            onSubmit={(e: React.SyntheticEvent) => {
              e.preventDefault();
              sendCommand(command);
              appendCommandHistory(command);
              setCommandNav((prev) => prev + 1);
              setCommand('');
            }}
          >
            <input
              className="w-full rounded-b-lg bg-gray-850 py-3 px-4 text-gray-300 outline-white/50 focus:outline-none placeholder:text-gray-500 focus-visible:outline focus-visible:outline-2 disabled:placeholder:text-gray-500"
              placeholder={consoleInputMessage || 'Enter command...'}
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              id="command"
              type="text"
              disabled={consoleInputMessage !== ''}
              onKeyDown={handleKeyDown}
            />
          </form>
        </div>
    </div>
    </div>
  );
}
