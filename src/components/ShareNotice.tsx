import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check, Inbox, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import { decodeHexParam, formatBytes } from "../lib/utils";
import type { ShareOffer } from "../types";

function readOffer(): ShareOffer | null {
  const raw = decodeHexParam(new URLSearchParams(window.location.search).get("offer"));
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as ShareOffer;
    return parsed.transferId && parsed.fileName ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * The receiving half of a LAN transfer: a small always-on-top card that asks
 * whether to accept an incoming file. The Rust thread that opened this window is
 * blocked on the answer, so every path out of here has to answer or close.
 */
export function ShareNotice() {
  const { t } = useI18n();
  const offer = readOffer();
  const [left, setLeft] = useState(offer?.timeoutSeconds ?? 45);
  const [expired, setExpired] = useState(false);
  const settled = useRef(false);

  // Closing the card without answering declines it on the Rust side, which is the
  // right outcome when the offer lapsed or was dismissed by hand.
  const close = useCallback(() => {
    void getCurrentWindow()
      .close()
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!offer) {
      close();
      return;
    }
    const started = Date.now();
    const timer = window.setInterval(() => {
      const elapsed = Math.floor((Date.now() - started) / 1000);
      const remaining = Math.max(0, offer.timeoutSeconds - elapsed);
      setLeft(remaining);
      if (remaining === 0) {
        window.clearInterval(timer);
        setExpired(true);
        // Give the "expired" label a beat to render before the card goes away.
        window.setTimeout(close, 900);
      }
    }, 250);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function answer(accepted: boolean) {
    if (!offer || settled.current) return;
    settled.current = true;
    try {
      await api.shareAnswer(offer.transferId, accepted);
    } catch {
      // A dead thread means the offer already timed out; closing is still right.
    }
    close();
  }

  if (!offer) return <div className="share-notice is-empty" />;

  const ratio = offer.timeoutSeconds > 0 ? left / offer.timeoutSeconds : 0;

  return (
    <div className="share-notice" data-tauri-drag-region>
      <div className="share-notice-head" data-tauri-drag-region>
        <span className="share-notice-glyph">
          <Inbox size={16} />
        </span>
        <span className="share-notice-kicker" data-tauri-drag-region>
          {t("shareNoticeKicker", { peer: offer.peerName })}
        </span>
        <span className={`share-notice-count ${ratio < 0.25 ? "is-low" : ""}`}>
          {expired ? t("shareNoticeExpired") : `${left}s`}
        </span>
      </div>
      <div className="share-notice-file" title={offer.fileName}>
        <strong data-tauri-drag-region>{offer.fileName}</strong>
        <small>{formatBytes(offer.size)}</small>
      </div>
      <div className="share-notice-actions">
        <button className="share-notice-decline" onClick={() => void answer(false)}>
          <X size={14} />
          {t("shareNoticeDecline")}
        </button>
        <button className="share-notice-accept" onClick={() => void answer(true)}>
          <Check size={14} />
          {t("shareNoticeAccept")}
        </button>
      </div>
      <div className="share-notice-bar">
        <span style={{ width: `${(ratio * 100).toFixed(1)}%` }} />
      </div>
    </div>
  );
}
