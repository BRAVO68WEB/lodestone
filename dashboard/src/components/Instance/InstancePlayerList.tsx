import { InstanceContext } from 'data/InstanceContext';
import { useContext, useRef, useState, useMemo } from 'react';
import { PlayerListItem, PlayerListCard } from 'components/PlayerListCard';
import {
  faArrowDown,
  faArrowUp,
  faUserCircle,
} from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { renderConsoleText } from 'utils/ansi';
import axios from 'axios';
import { MinecraftPlayer } from 'bindings/MinecraftPlayer';
import Button from 'components/Atoms/Button';
import ConfirmDialog from 'components/Atoms/ConfirmDialog';
import { useUserAuthorized } from 'data/UserInfo';
import { toast } from 'react-toastify';
import { errorToString } from 'utils/util';

interface PlayerWhere {
  dimension: string;
  x: number | null;
  y: number | null;
  z: number | null;
  biome: string;
  rcon_connected: boolean;
  message: string | null;
}

interface PlayerItem {
  slot: number;
  id: string;
  count: number;
}

interface PlayerInventory {
  rcon_connected: boolean;
  message: string | null;
  xp_level: number | null;
  xp_progress: number | null;
  inventory: PlayerItem[];
  ender_chest: PlayerItem[];
}

function formatItems(items: PlayerItem[]): string {
  if (items.length === 0) return 'empty';
  const shown = items.slice(0, 6).map((item) => `${item.count} ${item.id}`);
  if (items.length > 6) shown.push(`+${items.length - 6} more`);
  return shown.join(', ');
}

const whereCache = new Map<string, { at: number; value: PlayerWhere }>();
const WHERE_TTL_MS = 10_000;

function formatCoord(value: number | null): string {
  if (value === null || Number.isNaN(value)) return 'unknown';
  return value.toFixed(1);
}

const inventoryCache = new Map<string, { at: number; value: PlayerInventory }>();

function PlayerRow({ player, uuid }: { player: MinecraftPlayer; uuid: string }) {
  const [hover, setHover] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [where, setWhere] = useState<PlayerWhere | null>(null);
  const [inventory, setInventory] = useState<PlayerInventory | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmBan, setConfirmBan] = useState(false);
  const request = useRef(0);
  const canModerate = useUserAuthorized('can_access_instance_console', uuid);
  const open = hover || pinned;

  const show = () => {
    setHover(true);
    const key = `${uuid}:${player.name}`;
    const whereHit = whereCache.get(key);
    const inventoryHit = inventoryCache.get(key);
    const fresh = Date.now();
    if (whereHit && fresh - whereHit.at < WHERE_TTL_MS) {
      setWhere(whereHit.value);
    } else {
      const ticket = request.current + 1;
      request.current = ticket;
      setWhere(null);
      axios
        .get<PlayerWhere>(
          `/instance/${uuid}/player/${encodeURIComponent(player.name)}/where`
        )
        .then((response) => {
          whereCache.set(key, { at: Date.now(), value: response.data });
          if (request.current === ticket) setWhere(response.data);
        })
        .catch((err) => {
          if (request.current !== ticket) return;
          setWhere({
            dimension: 'unknown',
            x: null,
            y: null,
            z: null,
            biome: 'unknown',
            rcon_connected: false,
            message: errorToString(err),
          });
        });
    }
    if (inventoryHit && fresh - inventoryHit.at < WHERE_TTL_MS) {
      setInventory(inventoryHit.value);
      return;
    }
    axios
      .get<PlayerInventory>(
        `/instance/${uuid}/player/${encodeURIComponent(player.name)}/inventory`
      )
      .then((response) => {
        inventoryCache.set(key, { at: Date.now(), value: response.data });
        setInventory(response.data);
      })
      .catch((err) => {
        setInventory({
          rcon_connected: false,
          message: errorToString(err),
          xp_level: null,
          xp_progress: null,
          inventory: [],
          ender_chest: [],
        });
      });
  };

  const runAction = async (action: string) => {
    setBusy(true);
    try {
      await axios.post(
        `/instance/${uuid}/player/${encodeURIComponent(player.name)}/action`,
        { action }
      );
      toast.success(`${action.replace('_', ' ')} sent for ${player.name}`);
    } catch (err) {
      toast.error(errorToString(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="relative"
      onMouseEnter={show}
      onMouseLeave={() => setHover(false)}
      onFocus={show}
      onBlur={() => setHover(false)}
      onClick={() => setPinned((value) => !value)}
    >
      <PlayerListItem>
        <img
          src={`https://mc-heads.net/avatar/${player.uuid}/16.png`}
          alt=""
          className="mx-1 h-4 w-4"
          draggable="false"
          style={{ imageRendering: 'pixelated', userSelect: 'none' }}
        />
        <div className="mx-1 text-medium">{renderConsoleText(player.name)}</div>
      </PlayerListItem>
      {open && (
        <div
          role="tooltip"
          className="absolute left-0 top-full z-20 mt-1 w-72 rounded-md border border-gray-faded/30 bg-gray-900 p-2 text-small text-gray-300 shadow-lg"
        >
          {!where && <p className="text-white/50">Looking up location…</p>}
          {where && !where.rcon_connected && (
            <p>{where.message || 'Enable RCON in server properties to see dimension, coordinates, and biome.'}</p>
          )}
          {where && where.rcon_connected && (
            <div className="flex flex-col gap-1">
              <p>Dimension: {where.dimension || 'unknown'}</p>
              <p>
                Coordinates: {formatCoord(where.x)}, {formatCoord(where.y)}, {formatCoord(where.z)}
              </p>
              <p>Biome: {where.biome || 'unknown'}</p>
              {where.message && <p className="text-white/50">{where.message}</p>}
            </div>
          )}
          <div className="mt-2 flex flex-col gap-1 border-t border-gray-faded/30 pt-2">
            {!inventory && <p className="text-white/50">Looking up inventory…</p>}
            {inventory && !inventory.rcon_connected && (
              <p>{inventory.message || 'Enable RCON to see inventory.'}</p>
            )}
            {inventory && inventory.rcon_connected && (
              <>
                <p>
                  Level: {inventory.xp_level ?? 'unknown'}
                  {inventory.xp_progress != null
                    ? ` (${Math.round(inventory.xp_progress * 100)}%)`
                    : ''}
                </p>
                <p>Inventory: {formatItems(inventory.inventory)}</p>
                <p>Ender chest: {formatItems(inventory.ender_chest)}</p>
                {inventory.message && <p className="text-white/50">{inventory.message}</p>}
              </>
            )}
            {canModerate && (
              <div className="mt-1 flex flex-row flex-wrap gap-1">
                <Button
                  size="slim"
                  label="Kick"
                  intention="info"
                  disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    void runAction('kick');
                  }}
                />
                <Button
                  size="slim"
                  label="Clear"
                  intention="danger"
                  disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    void runAction('clear');
                  }}
                />
                <Button
                  size="slim"
                  label="Ban"
                  intention="danger"
                  disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    setConfirmBan(true);
                  }}
                />
                <Button
                  size="slim"
                  label="Unwhitelist"
                  intention="info"
                  disabled={busy}
                  onClick={(event) => {
                    event.stopPropagation();
                    void runAction('whitelist_remove');
                  }}
                />
              </div>
            )}
          </div>
        </div>
      )}
      <ConfirmDialog
        title={`Ban ${player.name}?`}
        type="danger"
        isOpen={confirmBan}
        confirmButtonText="Ban"
        closeButtonText="Cancel"
        onClose={() => setConfirmBan(false)}
        onConfirm={() => {
          setConfirmBan(false);
          void runAction('ban');
        }}
      >
        This bans {player.name} from the server through RCON.
      </ConfirmDialog>
    </div>
  );
}

export default function InstancePlayerList() {
  const { selectedInstance: instance } = useContext(InstanceContext);
  const uuid = instance?.uuid;

  // Ascending order is referring to alphabetical order
  // Note: Arrow will point DOWN when sorting in ascending order, as seen on the figma
  const [sortAscending, setSortAscending] = useState(true);

  // Update playerList every time there a player leaves or joins
  // Also sorts and updates when sort button is pressed
  const updatedPlayerList = useMemo(() => {
    if (!instance?.player_list) return null;
    const playerList = [...instance.player_list];
    playerList.sort((a, b) => {
      if (sortAscending) {
        return a.name.localeCompare(b.name);
      } else {
        return b.name.localeCompare(a.name);
      }
    });
    return playerList;
  }, [instance, sortAscending]);

  // Catch case where server instance is not available; return early
  if (!instance || !uuid) {
    return (
      <div
        className="relative flex h-full w-full flex-row justify-center overflow-y-auto px-4 pt-8 pb-10 @container"
        key={uuid}
      >
        <div className="flex h-fit min-h-full w-full grow flex-col items-start gap-2">
          <div className="flex min-w-0 flex-row items-center gap-4">
            <h1 className="dashboard-instance-heading truncate whitespace-pre">
              Instance not found
            </h1>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div>
      <h2 className="text-h2 font-extrabold tracking-medium">Player List</h2>
      <h3 className="text-h3 font-medium italic tracking-medium text-white/50">
        All players that are currently online
      </h3>
      <button
        className="mt-4 mb-2 ml-2 flex items-center justify-center text-small font-medium tracking-medium text-white/50"
        onClick={() => setSortAscending(!sortAscending)}
      >
        NAME
        {sortAscending ? (
          <FontAwesomeIcon icon={faArrowDown} className="mx-1.5" />
        ) : (
          <FontAwesomeIcon icon={faArrowUp} className="mx-1.5" />
        )}
      </button>
      {updatedPlayerList && updatedPlayerList.length > 0 ? (
        <PlayerListCard>
          {updatedPlayerList.map((player) => (
            <PlayerRow key={player.uuid} player={player} uuid={uuid} />
          ))}
        </PlayerListCard>
      ) : (
        <PlayerListCard className="border-2 border-dashed">
          <PlayerListItem key="not found">
            <FontAwesomeIcon
              icon={faUserCircle}
              className="mx-1 h-4 w-4 text-white/50"
            />
            <div className="mx-1 text-medium italic text-white/50">
              No players online
            </div>
          </PlayerListItem>
        </PlayerListCard>
      )}
    </div>
  );
}
