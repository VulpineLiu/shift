import { Circle, LoaderCircle, Square } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import { formatDuration } from "../lib/utils";
import type { RecordingInfo } from "../types";

export function RecorderHud() {
  const { t } = useI18n();
  const [recording, setRecording] = useState<RecordingInfo | null>(null);
  const [now, setNow] = useState(Date.now());
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api.recordingStatus().then(setRecording);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<RecordingInfo>("recording://started", (event) => {
      setRecording(event.payload);
      setNow(Date.now());
      setStopping(false);
      setError(null);
    }).then((stop) => {
      unlisten = stop;
    });
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, []);

  async function stop() {
    if (stopping) return;
    setStopping(true);
    setError(null);
    try {
      await api.stopRecording();
    } catch (stopError) {
      setError(String(stopError));
    } finally {
      setStopping(false);
    }
  }

  return (
    <div className="recorder-hud">
      <span className="recorder-pulse">
        <Circle size={12} fill="currentColor" />
      </span>
      <div className="recorder-copy" data-tauri-drag-region>
        <strong>{recording ? formatDuration(now - recording.startedAt) : "00:00"}</strong>
        <span className={error ? "recorder-error" : ""} title={error ?? undefined}>
          {error || t("record")}
        </span>
      </div>
      <button type="button" className="recorder-stop" onClick={() => void stop()} disabled={stopping}>
        {stopping ? <LoaderCircle size={15} className="spin" /> : <Square size={14} fill="currentColor" />}
        {stopping ? t("processing") : t("stopRecording")}
      </button>
    </div>
  );
}
