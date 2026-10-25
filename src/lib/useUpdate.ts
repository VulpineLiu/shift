import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { UpdateProgress, UpdateStatus } from "../types";

export interface UpdateFlow {
  /** Last known result, or null when nothing has been checked this session. */
  status: UpdateStatus | null;
  checking: boolean;
  downloading: boolean;
  installing: boolean;
  progress: UpdateProgress | null;
  /** Percent of the download, or null until the first byte-aligned report. */
  percent: number | null;
  check: () => Promise<void>;
  download: () => Promise<void>;
  install: () => Promise<void>;
  clear: () => Promise<void>;
}

/**
 * The git update channel as one state machine: check the manifest, download the
 * artifact it names, refuse the install unless the recomputed SHA256 matches, then
 * hand the verified installer to the system.
 *
 * Both the main window's bar button and Settings > Updates run this hook rather than
 * keeping their own copy. Rust broadcasts `update://status` and `update://progress`,
 * so every mounted copy converges on the same state and neither can offer an action
 * the other has already finished.
 */
export function useUpdateFlow(): UpdateFlow {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [percent, setPercent] = useState<number | null>(null);
  const mounted = useRef(true);
  // Install must refuse a status with no verified file on disk, and the callback
  // cannot read the state variable without being rebuilt on every progress tick.
  const statusRef = useRef<UpdateStatus | null>(null);
  const publish = useCallback((next: UpdateStatus | null) => {
    statusRef.current = next;
    if (mounted.current) setStatus(next);
  }, []);

  const check = useCallback(async () => {
    if (checking) return;
    setChecking(true);
    try {
      const outcome = await api.updateCheck();
      publish(outcome.status);
    } catch {
      // A failed probe still has to be reported, and Rust holds the last result.
      publish(await api.updateStatus().catch(() => null));
    } finally {
      if (mounted.current) setChecking(false);
    }
  }, [checking, publish]);

  const download = useCallback(async () => {
    if (downloading) return;
    setDownloading(true);
    setProgress(null);
    setPercent(null);
    try {
      const ready = await api.updateDownload();
      publish(ready);
    } catch {
      publish(await api.updateStatus().catch(() => null));
    } finally {
      if (mounted.current) {
        setDownloading(false);
        setProgress(null);
        setPercent(null);
      }
    }
  }, [downloading, publish]);

  const install = useCallback(async () => {
    if (installing || !statusRef.current?.path) return;
    setInstalling(true);
    try {
      await api.updateInstall();
    } catch (error) {
      const last = statusRef.current;
      if (last) publish({ ...last, message: String(error) });
    } finally {
      if (mounted.current) setInstalling(false);
    }
  }, [installing, publish]);

  const clear = useCallback(async () => {
    await api.updateClear().catch(() => undefined);
    publish(null);
  }, [publish]);

  useEffect(() => {
    mounted.current = true;
    // Rust keeps the last result, so a fresh window shows it without a request.
    void api
      .updateStatus()
      .then((last) => {
        if (last) publish(last);
      })
      .catch(() => undefined);
    // Event names live in the api layer, the same one every other view uses.
    const unlisteners: (() => void)[] = [];
    void api
      .onUpdateProgress((next) => {
        if (!mounted.current) return;
        setProgress(next);
        setPercent(next.total > 0 ? Math.min(100, Math.round((next.downloaded / next.total) * 100)) : null);
      })
      .then((stop) => unlisteners.push(stop));
    void api
      .onUpdateStatus((next) => {
        statusRef.current = next;
        if (mounted.current) setStatus(next);
      })
      .then((stop) => unlisteners.push(stop));
    return () => {
      mounted.current = false;
      unlisteners.forEach((stop) => stop());
    };
  }, [publish]);

  return {
    status,
    checking,
    downloading,
    installing,
    progress,
    percent,
    check,
    download,
    install,
    clear,
  };
}
