import { ArrowDown, ArrowUp, RotateCcw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../i18n";
import {
  ORDERABLE_KINDS,
  hasCustomOrder,
  isOrderDirty,
  moveWedge,
  orderWedgeIds,
  presetWedgeIds,
  savedOrderFor,
  wedgeLabelKey,
  type FileKind,
  type RadialRing,
  type WedgeOrders,
} from "../lib/radialOrder";

/**
 * The reorder screen for the radial rings.
 *
 * It edits one preset at a time — a file kind plus a ring — and can only rearrange the
 * wedges that ring actually offers, so a preset can never invent an action. The list is
 * always the live catalogue in a saved order; saving an empty list (Reset) hands the
 * ring back to the built-in order. Every rule the screen applies lives in
 * `src/lib/radialOrder.ts`, which keeps them testable without mounting this panel.
 */
export function RadialOrderPanel({
  orders,
  onApply,
}: {
  orders: WedgeOrders;
  onApply: (kind: FileKind, ring: RadialRing, order: string[]) => void;
}) {
  const { t } = useI18n();
  const [kind, setKind] = useState<FileKind>("image");
  const [ring, setRing] = useState<RadialRing>("actions");
  const [draft, setDraft] = useState<string[]>([]);

  const catalogue = useMemo(() => presetWedgeIds(kind, ring), [ring, kind]);
  const saved = savedOrderFor(orders, kind, ring);

  useEffect(() => {
    setDraft(orderWedgeIds(catalogue, saved));
  }, [catalogue, saved]);

  const custom = Boolean(saved);

  return (
    <>
      <section className="home-settings-section">
        <div className="home-section-title">{t("radialKind")}</div>
        <div className="radial-kind-grid">
          {ORDERABLE_KINDS.map((entry) => (
            <button
              key={entry}
              className={entry === kind ? "is-active" : ""}
              onClick={() => setKind(entry)}
            >
              {t(`kind_${entry}`)}
              {hasCustomOrder(orders, entry) ? <span className="radial-kind-dot" /> : null}
            </button>
          ))}
        </div>
        <div className="home-segment">
          <button
            className={ring === "actions" ? "is-active" : ""}
            onClick={() => setRing("actions")}
          >
            {t("ringActions")}
          </button>
          <button className={ring === "tools" ? "is-active" : ""} onClick={() => setRing("tools")}>
            {t("ringTools")}
          </button>
        </div>
      </section>

      <section className="home-settings-section">
        <div className="runtime-section-head">
          <div className="home-section-title">{t("radialOrder", { count: draft.length })}</div>
          <button
            className="settings-link"
            onClick={() => onApply(kind, ring, [])}
            disabled={!custom}
          >
            <RotateCcw size={12} />
            {t("reset")}
          </button>
        </div>
        <p className="runtime-summary">{t("radialOrderBody")}</p>
        <ol className="radial-order-list">
          {draft.map((id, index) => (
            <li className="radial-order-row" key={id}>
              <span className="radial-order-index">{index + 1}</span>
              <span className="radial-order-label">{t(wedgeLabelKey(id))}</span>
              <span className="radial-order-buttons">
                <button
                  onClick={() => setDraft(moveWedge(draft, index, -1))}
                  disabled={index === 0}
                  title={t("moveUp")}
                  aria-label={t("moveUp")}
                >
                  <ArrowUp size={12} />
                </button>
                <button
                  onClick={() => setDraft(moveWedge(draft, index, 1))}
                  disabled={index === draft.length - 1}
                  title={t("moveDown")}
                  aria-label={t("moveDown")}
                >
                  <ArrowDown size={12} />
                </button>
              </span>
            </li>
          ))}
        </ol>
        <button
          className="settings-wide-button radial-order-save"
          onClick={() => onApply(kind, ring, draft)}
          disabled={!isOrderDirty(draft, catalogue, saved)}
        >
          {t("radialOrderSave")}
        </button>
      </section>
    </>
  );
}
