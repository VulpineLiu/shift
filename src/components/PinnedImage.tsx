import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { Copy, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";

function decodePath(value: string | null): string | null {
  if (!value || value.length % 2 !== 0 || !/^[0-9a-f]+$/i.test(value)) return null;
  try {
    const bytes = new Uint8Array((value.match(/../g) ?? []).map((pair) => parseInt(pair, 16)));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

export function PinnedImage() {
  const { t } = useI18n();
  const path = decodePath(new URLSearchParams(window.location.search).get("path"));
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!path) {
      setError("The pinned image is unavailable.");
      return;
    }
    if (!isTauri()) {
      setSrc("/demo-preview.svg");
      return;
    }
    void api
      .readFileDataUrl(path)
      .then(setSrc)
      .catch((reason) => setError(String(reason)));
  }, [path]);

  async function copy() {
    if (!path) return;
    await writeImage(path);
  }

  if (error) {
    return <div className="pin-error">{error}</div>;
  }

  return (
    <div className="pin-shell">
      <div
        className="pin-surface"
        data-tauri-drag-region
        onDoubleClick={() => void getCurrentWindow().close()}
      >
        {src ? (
          <img src={src} alt="" draggable={false} data-tauri-drag-region />
        ) : (
          <div className="pin-loading" />
        )}
      </div>
      <div className="pin-actions">
        <button type="button" title={t("copy")} onClick={() => void copy()}>
          <Copy size={13} />
        </button>
        <button type="button" title={t("close")} onClick={() => void getCurrentWindow().close()}>
          <X size={13} />
        </button>
      </div>
    </div>
  );
}
