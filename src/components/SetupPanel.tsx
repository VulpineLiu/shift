import { useCallback, useEffect, useState } from "react";
import {
  Check,
  FolderOpen,
  Keyboard,
  LoaderCircle,
  RefreshCw,
  ScanLine,
  ShieldCheck,
  X,
} from "lucide-react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import type { HotkeyStatus, RuntimeInfo, WriteCheck } from "../types";

type State = "checking" | "ok" | "problem" | "unknown";

interface Row {
  id: "capture" | "folders" | "shortcuts" | "tools";
  state: State;
  detail: string;
  action?: { label: string; run: () => void };
}

/**
 * Guided first-run checks. Everything here is a probe that Settings also offers, so
 * the flow reports the same truth rather than a first-run guess: it reads the OS
 * permission state, writes a probe file into each output folder, asks which copy owns
 * the global hotkeys, and lists optional tools the platform is missing. Dismissing it
 * never disables anything.
 */
export function SetupPanel({
  onDone,
  onOpenRuntime,
}: {
  onDone: () => void;
  onOpenRuntime: () => void;
}) {
  const { t } = useI18n();
  const [rows, setRows] = useState<Row[]>([
    { id: "capture", state: "checking", detail: "" },
    { id: "folders", state: "checking", detail: "" },
    { id: "shortcuts", state: "checking", detail: "" },
    { id: "tools", state: "checking", detail: "" },
  ]);
  const [checks, setChecks] = useState<WriteCheck[] | null>(null);

  const patch = (id: Row["id"], next: Partial<Row>) =>
    setRows((current) => current.map((row) => (row.id === id ? { ...row, ...next } : row)));

  // Escape leaves the guided flow the way it leaves every other panel. Everything
  // the flow checks is re-runnable from Settings, so closing it loses nothing.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onDone();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onDone]);

  const refresh = useCallback(async () => {
    setRows((current) => current.map((row) => ({ ...row, state: "checking" as State })));

    void api
      .screenCaptureAccessStatus()
      .then((allowed) =>
        patch(
          "capture",
          allowed
            ? { state: "ok", detail: t("setupCaptureOk") }
            : {
                state: "problem",
                detail: t("screenPermissionPrompt"),
                action: {
                  label: t("openScreenSettings"),
                  run: () => void api.openScreenRecordingSettings(),
                },
              },
        ),
      )
      .catch(() => patch("capture", { state: "unknown", detail: t("setupCheckFailed") }));

    void api
      .checkOutputAccess()
      .then((results) => {
        setChecks(results);
        const broken = results.filter((check) => !check.ok);
        patch(
          "folders",
          broken.length === 0
            ? { state: "ok", detail: t("setupFoldersOk") }
            : { state: "problem", detail: t("accessDenied") },
        );
      })
      .catch(() => patch("folders", { state: "unknown", detail: t("setupCheckFailed") }));

    void api
      .hotkeyStatus()
      .then((status: HotkeyStatus) =>
        patch(
          "shortcuts",
          status.live
            ? { state: "ok", detail: t("setupShortcutsOk") }
            : {
                state: "problem",
                detail: status.others.length > 0 ? t("hotkeysDead") : t("hotkeysNotLive"),
                action: {
                  label: t("hotkeysFix"),
                  run: () =>
                    void api
                      .quitOtherInstances()
                      .then(() => api.reclaimHotkeys())
                      .then((live) =>
                        patch(
                          "shortcuts",
                          live.live
                            ? { state: "ok", detail: t("setupShortcutsOk") }
                            : { state: "problem", detail: t("hotkeysNotLive") },
                        ),
                      )
                      .catch(() => patch("shortcuts", { state: "unknown", detail: t("setupCheckFailed") })),
                },
              },
        ),
      )
      .catch(() => patch("shortcuts", { state: "unknown", detail: t("setupCheckFailed") }));

    void api
      .getRuntimeInfo()
      .then((info: RuntimeInfo) => {
        const missing = info.components.filter((component) => !component.available);
        const installable = missing.filter((component) => component.installable);
        // Anything missing is the same state; only the fix differs. A platform with
        // no installer still says what is absent rather than "not checked", which
        // would imply the probe never ran.
        patch(
          "tools",
          missing.length === 0
            ? { state: "ok", detail: t("setupToolsOk") }
            : {
                state: "problem",
                detail: t("setupToolsMissing", { count: missing.length }),
                action: installable.length
                  ? { label: t("runtimeOpenPanel"), run: onOpenRuntime }
                  : undefined,
              },
        );
      })
      .catch(() => patch("tools", { state: "unknown", detail: t("setupCheckFailed") }));
  }, [t, onDone, onOpenRuntime]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const icons = {
    capture: ScanLine,
    folders: FolderOpen,
    shortcuts: Keyboard,
    tools: ShieldCheck,
  } as const;

  return (
    <div className="setup-scrim" role="dialog" aria-modal="true" aria-label={t("setupTitle")}>
      <div className="setup-card">
        <header className="setup-head">
          <div>
            <h2>{t("setupTitle")}</h2>
            <p>{t("setupIntro")}</p>
          </div>
          <button className="setup-refresh" onClick={() => void refresh()}>
            <RefreshCw size={13} />
            {t("setupRecheck")}
          </button>
        </header>

        <ul className="setup-list">
          {rows.map((row) => {
            const Icon = icons[row.id];
            return (
              <li key={row.id} className={`setup-row is-${row.state}`}>
                <span className="setup-row-icon">
                  <Icon size={17} />
                </span>
                <span className="setup-row-body">
                  <strong>{t(`setupRow_${row.id}`)}</strong>
                  <small>
                    {row.state === "checking" ? (
                      <>
                        <LoaderCircle size={11} className="spin" /> {t("setupChecking")}
                      </>
                    ) : (
                      row.detail
                    )}
                  </small>
                  {row.action ? (
                    <button className="settings-link" onClick={row.action.run}>
                      {row.action.label}
                    </button>
                  ) : null}
                </span>
                <span className={`setup-state is-${row.state}`}>
                  {row.state === "ok" ? (
                    <Check size={16} />
                  ) : row.state === "problem" ? (
                    <X size={16} />
                  ) : null}
                  {t(
                    row.state === "ok"
                      ? "setupStateOk"
                      : row.state === "problem"
                        ? "setupStateProblem"
                        : "setupStateUnknown",
                  )}
                </span>
              </li>
            );
          })}
        </ul>

        {checks ? (
          <div className="setup-paths">
            {checks.map((check) => (
              <code key={check.kind}>{check.path}</code>
            ))}
          </div>
        ) : null}

        <footer className="setup-foot">
          <span>{t("setupFootnote")}</span>
          <button className="runtime-install" onClick={onDone}>
            {t("setupStart")}
          </button>
        </footer>
      </div>
    </div>
  );
}
