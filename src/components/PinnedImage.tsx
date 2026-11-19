import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { Copy, Maximize2, Minimize2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import { decodeHexParam, messageFrom } from "../lib/utils";

interface PinnedPreview {
  id: number;
  path: string;
  dataUrl: string;
  width: number;
  height: number;
}

export function PinnedImage() {
  const { t } = useI18n();
  const [path, setPath] = useState(() =>
    decodeHexParam(new URLSearchParams(window.location.search).get("path")),
  );
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Whether the window is showing the enlarged size. Rust owns the real answer and
  // the button only asks it to flip, so this exists to label the button the way the
  // window looks rather than to remember a size the page cannot measure.
  const [enlarged, setEnlarged] = useState(false);
  const latestId = useRef(0);
  const renderedId = useRef(0);

  useEffect(() => {
    if (!isTauri()) {
      setSrc("/demo-preview.svg");
      return;
    }
    let disposed = false;
    const apply = (preview: PinnedPreview | null) => {
      if (disposed) return;
      if (!preview) {
        if (latestId.current === 0) setSrc(null);
        return;
      }
      if (preview.id < latestId.current) return;
      if (preview.id === renderedId.current) return;
      latestId.current = preview.id;
      setPath(preview.path);
      // A new pin always opens small, whatever the last one was left at.
      setEnlarged(false);
      const image = new Image();
      image.onload = () => {
        if (disposed) return;
        renderedId.current = preview.id;
        setSrc(preview.dataUrl);
        setError(null);
        // Let React commit the loaded image before the native window is shown.
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            if (!disposed) void api.pinReady(preview.id);
          });
        });
      };
      image.onerror = () => {
        if (disposed) return;
        renderedId.current = preview.id;
        setError(t("pinLoadFailed"));
        void api.pinReady(preview.id);
      };
      image.src = preview.dataUrl;
    };
    const refresh = () => {
      void api
        .getPinnedImage()
        .then(apply)
        .catch((reason) => {
          if (disposed) return;
          setError(t(messageFrom(reason)));
          void api.pinReady(null);
        });
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") refresh();
    };
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    let unlisten: Promise<() => void> | null = null;
    // Register first: a reused pin window can receive a new image immediately
    // after the previous pin is closed. Querying first would lose that event.
    unlisten = listen<PinnedPreview>("pin://show", (event) => apply(event.payload)).then(
      (stopListening) => {
        if (!disposed) refresh();
        return stopListening;
      },
    );
    return () => {
      disposed = true;
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
      void unlisten?.then((release) => release());
    };
    // Language changes only affect error text, not the image payload.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function copy() {
    if (!path) return;
    await writeImage(path);
  }

  async function zoom() {
    try {
      setEnlarged(await api.zoomPin());
    } catch {
      // The window stayed the size it was, so the label stays true and the press is
      // simply repeated. A pin that cannot be resized has nothing the user is waiting
      // to read, which is not worth covering the picture with an error card.
    }
  }

  if (error) {
    return <div className="pin-error" role="alert">{error}</div>;
  }

  return (
    <div className="pin-shell">
      <div
        className="pin-surface"
        data-tauri-drag-region
        onDoubleClick={() => void api.closePin()}
      >
        {src ? (
          <img
            src={src}
            alt=""
            draggable={false}
            data-tauri-drag-region
            onError={() => setError(t("pinLoadFailed"))}
          />
        ) : (
          <div className="pin-loading" />
        )}
      </div>
      <div className="pin-actions">
        <button
          type="button"
          title={t(enlarged ? "restorePin" : "enlargePin")}
          aria-pressed={enlarged}
          onClick={() => void zoom()}
        >
          {enlarged ? <Minimize2 size={13} /> : <Maximize2 size={13} />}
        </button>
        <button type="button" title={t("copy")} onClick={() => void copy()}>
          <Copy size={13} />
        </button>
        <button type="button" title={t("close")} onClick={() => void api.closePin()}>
          <X size={13} />
        </button>
      </div>
    </div>
  );
}
