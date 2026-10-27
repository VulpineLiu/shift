import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Apple,
  Check,
  CircleAlert,
  File as FileIcon,
  GripVertical,
  Laptop,
  LoaderCircle,
  Monitor,
  Radar,
  RefreshCw,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type DragEvent } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import { parseSharePaths, SHARE_PATHS_TYPE } from "../lib/dragOut";
import { basename, dirname, formatBytes } from "../lib/utils";
import type { SharePeer, ShareTransfer } from "../types";

interface SharePanelProps {
  paths: string[];
  onClose: () => void;
  onNotice: (message: string) => void;
}

const PLATFORM_LABEL: Record<string, string> = {
  macos: "shareMac",
  windows: "shareWindows",
  linux: "shareLinux",
};

function PlatformIcon({ platform }: { platform: string }) {
  if (platform === "macos") return <Apple size={20} />;
  if (platform === "windows") return <Monitor size={20} />;
  return <Laptop size={20} />;
}

/**
 * AirDrop-style picker: Shift copies that answer our LAN beacon appear as cards and
 * tapping a card sends the file straight away.
 */
export function SharePanel({ paths, onClose, onNotice }: SharePanelProps) {
  const { t } = useI18n();
  const [peers, setPeers] = useState<SharePeer[]>([]);
  const [searching, setSearching] = useState(true);
  const [busyPeer, setBusyPeer] = useState<string | null>(null);
  const [progress, setProgress] = useState<ShareTransfer | null>(null);
  const [failed, setFailed] = useState(false);
  const sendingRef = useRef(false);
  const [selectedPaths, setSelectedPaths] = useState(paths);
  const [dragOverPeer, setDragOverPeer] = useState<string | null>(null);
  // Set while the peer decides, cleared by the first progress event or the reply.
  const [waitingFor, setWaitingFor] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      setSearching(false);
      return;
    }
    const unlisteners: Array<() => void> = [];
    void listen<SharePeer[]>("share://peers", (event) => {
      setPeers(event.payload);
      if (event.payload.length > 0) setSearching(false);
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<ShareTransfer>("share://transfer", (event) => {
      const transfer = event.payload;
      // While this panel is sending, the progress events belong to us. Inbound
      // transfers are announced by the main window, which always listens.
      if (sendingRef.current) {
        setWaitingFor(null);
        setProgress(transfer);
      }
    }).then((unlisten) => unlisteners.push(unlisten));
    void api
      .shareRefresh()
      .then((list) => {
        setPeers(list);
        if (list.length > 0) setSearching(false);
      })
      .catch(() => setSearching(false));
    // Beacons arrive within a couple of seconds; stop the spinner after that.
    const timer = window.setTimeout(() => setSearching(false), 4_000);
    return () => {
      unlisteners.forEach((unlisten) => unlisten());
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  async function send(peer: SharePeer, files: string[] = selectedPaths) {
    if (busyPeer || files.length === 0) return;
    setBusyPeer(peer.id);
    setFailed(false);
    setProgress(null);
    setWaitingFor(peer.name);
    sendingRef.current = true;
    try {
      const result = await api.shareSend(peer.id, files);
      setProgress(null);
      setWaitingFor(null);
      const peerName = result[0]?.peer ?? peer.name;
      onNotice(
        result.length > 1
          ? t("shareSentMany", { count: result.length, peer: peerName })
          : t("shareSent", { name: result[0]?.name ?? basename(paths[0]), peer: peerName })
      );
      onClose();
    } catch (error) {
      const message = String(error);
      setFailed(true);
      setProgress(null);
      setWaitingFor(null);
      const name =
        files.length > 1
          ? t("shareFilesCount", { count: files.length })
          : basename(files[0]);
      if (message.includes("share-peer-gone")) onNotice(t("sharePeerGone"));
      else if (message.includes("share-unreachable")) onNotice(t("shareUnreachable"));
      // The peer's own answer arrives as an error, so these are not failures of ours.
      else if (message.includes("share-declined"))
        onNotice(t("shareDeclined", { name, peer: peer.name }));
      else if (message.includes("share-timeout"))
        onNotice(t("shareTimeout", { name, peer: peer.name }));
      else onNotice(t("shareFailed", { name }));
    } finally {
      sendingRef.current = false;
      setBusyPeer(null);
    }
  }

  function searchAgain() {
    setSearching(true);
    setFailed(false);
    if (!isTauri()) {
      setSearching(false);
      return;
    }
    void api
      .shareRefresh()
      .then((list) => {
        setPeers(list);
        if (list.length > 0) setSearching(false);
      })
      .catch(() => setSearching(false));
    window.setTimeout(() => setSearching(false), 4_000);
  }

  const sending = progress && progress.state === "sending";
  const ratio =
    sending && progress && progress.total > 0
      ? Math.min(1, progress.sent / progress.total)
      : 0;
  // Keep the modal open while a transfer is live so the progress bar is visible.
  const closeDisabled = Boolean(busyPeer) || Boolean(sending);
  const allSelected = paths.length > 0 && selectedPaths.length === paths.length;

  function togglePath(path: string) {
    setSelectedPaths((current) =>
      current.includes(path)
        ? current.filter((candidate) => candidate !== path)
        : [...current, path],
    );
  }

  function toggleAll() {
    setSelectedPaths(allSelected ? [] : paths);
  }

  function dragPaths(event: DragEvent, fallback: string[]) {
    const raw = event.dataTransfer.getData(SHARE_PATHS_TYPE);
    return parseSharePaths(raw) ?? fallback;
  }

  return (
    <>
      <div
        className="share-backdrop"
        onClick={() => {
          if (!closeDisabled) onClose();
        }}
      />
      <div
        className="share-panel"
        role="dialog"
        aria-modal="true"
        aria-label={t("shareTitle")}
      >
        <header className="share-head">
          <span className="share-head-glyph">
            <Radar size={17} className={searching && peers.length === 0 ? "spin" : ""} />
          </span>
          <div className="share-head-copy">
            <h2>{t("shareTitle")}</h2>
          </div>
          <button
            className="home-icon-button"
            onClick={() => {
              if (!closeDisabled) onClose();
            }}
            aria-label={t("close")}
            title={t("close")}
            disabled={closeDisabled}
          >
            <X size={16} />
          </button>
        </header>

        <section className="share-files" aria-label={t("shareFiles")}>
          <div className="share-files-head">
            <strong>{t("shareFiles")}</strong>
            <span>{t("shareSelectionCount", {
              selected: selectedPaths.length,
              total: paths.length,
            })}</span>
            {paths.length > 1 ? (
              <label className="share-select-all">
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={toggleAll}
                  disabled={closeDisabled}
                />
                <span className="share-checkmark" aria-hidden="true">
                  {allSelected ? <Check size={10} /> : null}
                </span>
                {t("shareSelectAll")}
              </label>
            ) : null}
          </div>
          <div className="share-file-list" role="group" aria-label={t("shareFiles")}>
            {paths.map((path) => {
              const checked = selectedPaths.includes(path);
              return (
                <label
                  key={path}
                  className={`share-file-row ${checked ? "is-checked" : ""}`}
                  title={path}
                  draggable={!closeDisabled}
                  onDragStart={(event) => {
                    const files = checked ? selectedPaths : [path];
                    event.dataTransfer.effectAllowed = "copy";
                    event.dataTransfer.setData(
                      SHARE_PATHS_TYPE,
                      JSON.stringify(files),
                    );
                  }}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => togglePath(path)}
                    disabled={closeDisabled}
                  />
                  <span className="share-checkmark" aria-hidden="true">
                    {checked ? <Check size={10} /> : null}
                  </span>
                  <FileIcon size={13} />
                  <span className="share-file-copy">
                    <strong>{basename(path)}</strong>
                    <small>{dirname(path)}</small>
                  </span>
                  <GripVertical className="share-file-grip" size={13} aria-hidden="true" />
                </label>
              );
            })}
          </div>
        </section>

        {peers.length === 0 ? (
          <div className="share-empty">
            <span className="share-empty-glyph">
              {searching ? <LoaderCircle size={17} className="spin" /> : <Radar size={17} />}
            </span>
            <strong>{searching ? t("shareSearching") : t("shareNoPeers")}</strong>
          </div>
        ) : (
          <div className="share-grid">
            {peers.map((peer) => {
              const active = busyPeer === peer.id;
              const dropTarget = dragOverPeer === peer.id;
              return (
                <button
                  key={peer.id}
                  className={`share-peer ${active ? "is-active" : ""} ${dropTarget ? "is-drop-target" : ""}`}
                  onClick={() => void send(peer)}
                  disabled={selectedPaths.length === 0 || (closeDisabled && !active)}
                  title={peer.address}
                  onDragEnter={(event) => {
                    if (!closeDisabled) {
                      event.preventDefault();
                      setDragOverPeer(peer.id);
                    }
                  }}
                  onDragOver={(event) => {
                    if (!closeDisabled) {
                      event.preventDefault();
                      event.dataTransfer.dropEffect = "copy";
                    }
                  }}
                  onDragLeave={(event) => {
                    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
                      setDragOverPeer((current) => (current === peer.id ? null : current));
                    }
                  }}
                  onDrop={(event) => {
                    event.preventDefault();
                    setDragOverPeer(null);
                    const files = dragPaths(event, selectedPaths);
                    if (files.length > 0) void send(peer, files);
                  }}
                >
                  <span className="share-peer-avatar">
                    {active ? <LoaderCircle size={20} className="spin" /> : <PlatformIcon platform={peer.platform} />}
                  </span>
                  <span className="share-peer-name">{peer.name}</span>
                  <span className="share-peer-meta">
                    {t(PLATFORM_LABEL[peer.platform] ?? "shareLinux")}
                  </span>
                </button>
              );
            })}
          </div>
        )}

        {waitingFor ? (
          <div className="share-progress">
            <div className="share-progress-head">
              <span className="share-progress-wait">
                <LoaderCircle size={12} className="spin" />
                {t("shareWaiting", { peer: waitingFor })}
              </span>
            </div>
            <div className="share-bar is-indeterminate">
              <span />
            </div>
          </div>
        ) : null}

        {sending && progress && !waitingFor ? (
          <div className="share-progress">
            <div className="share-progress-head">
              <span>
                {t("shareSendingTo", {
                  name: progress.fileName,
                  peer: progress.peerName,
                })}
              </span>
              <small>
                {formatBytes(progress.sent)} / {formatBytes(progress.total)}
              </small>
            </div>
            <div className="share-bar">
              <span style={{ width: `${(ratio * 100).toFixed(1)}%` }} />
            </div>
          </div>
        ) : null}

        {failed ? (
          <div className="share-error">
            <CircleAlert size={13} />
            {t("shareFailed", {
              name:
                paths.length > 1 ? t("shareFilesCount", { count: paths.length }) : basename(paths[0]),
            })}
          </div>
        ) : null}

        <footer className="share-foot">
          <button
            className="share-reshow"
            onClick={searchAgain}
            disabled={closeDisabled}
            title={t("shareRefresh")}
          >
            <RefreshCw size={12} className={searching ? "spin" : ""} />
            {searching ? t("shareSearching") : t("shareRefresh")}
          </button>
          <kbd>Esc</kbd>
        </footer>
      </div>
    </>
  );
}
