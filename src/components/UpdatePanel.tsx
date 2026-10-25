import {
  Check,
  Download,
  LoaderCircle,
  Monitor,
  PackageOpen,
  RefreshCw,
  Rss,
  ShieldCheck,
  Sparkle,
  X,
} from "lucide-react";
import { useI18n } from "../i18n";
import { useUpdateFlow } from "../lib/useUpdate";
import { formatBytes } from "../lib/utils";

interface Props {
  /** Raw URL of the manifest, so the panel can name the feed it just queried. */
  feedUrl: string;
  autoCheck: boolean;
  onAutoCheckChange: (enabled: boolean) => void;
  /**
   * The host this build is running on. The panel needs it because the update
   * question is per-operating-system: which artifact the feed will serve, and
   * whether a download can be installed at all, both follow from it.
   */
  system: UpdateSystemInfo | null;
}

/** Operating-system facts, read from the same probe the Runtime tab uses. */
export interface UpdateSystemInfo {
  platform: string;
  osVersion: string;
  architecture: string;
  /** Version of the copy the user is looking at, so the header needs no check. */
  appVersion: string;
}

/** Rust reports `std::env::consts::ARCH`; the channel names the same thing differently. */
function architectureLabel(arch: string): string {
  if (arch === "aarch64") return "ARM64";
  if (arch === "x86_64") return "x64";
  return arch;
}

/**
 * Update channel backed by the git repository itself: the release branch carries
 * the installers plus a manifest of SHA256 hashes, and this panel is the only
 * place a download can be trusted. A hash mismatch is shown as an error and the
 * file is deleted, so a half-good download can never be launched.
 */
export function UpdatePanel({ feedUrl, autoCheck, onAutoCheckChange, system }: Props) {
  const { t } = useI18n();
  // One state machine shared with any other view of the channel, driven by the
  // events Rust already broadcasts, so two views cannot offer different actions.
  const {
    status,
    checking,
    downloading,
    installing,
    installError,
    progress,
    percent,
    check,
    download,
    install,
    clear,
  } = useUpdateFlow();

  const verified = status?.state === "ready";
  // Rust reports the key it resolved; the panel only repeats it.
  const artifactKey = status?.artifactKey ?? "";
  const available = status?.state === "available" || verified;
  const failed = status?.state === "error" || status?.state === "unavailable";

  return (
    <section className="home-settings-section">
      {/* The two facts an update answer has to be read against come first: which
          machine and which Shift version is asking, then where updates come from.
          Both precede the Updates heading so a screenshot of this panel already
          says what was upgraded and from where. */}
      <div className="update-meta">
        <div className="update-meta-line">
          <Monitor size={13} />
          <span className="update-meta-label">{t("updateSystem")}</span>
          {system ? (
            <span className="update-system-host">
              <strong>{system.osVersion || system.platform}</strong>
              <small>{architectureLabel(system.architecture)}</small>
              <em>Shift {system.appVersion}</em>
            </span>
          ) : (
            <span className="update-system-pending">{t("updateSystemPending")}</span>
          )}
          {artifactKey ? (
            <span className="update-system-key">
              {t("updateSystemArtifact", { key: artifactKey })}
            </span>
          ) : status && status.state === "unavailable" ? (
            <span className="update-system-key is-unknown">
              {t("updateSystemArtifactUnknown")}
            </span>
          ) : null}
        </div>
        <div className="update-meta-line">
          <Rss size={13} />
          <span className="update-meta-label">{t("updateFeed")}</span>
          {/* Only a status backed by a manifest may claim anything about trust: an
              errored check has no manifest to have verified. */}
          {status && status.state !== "error" ? (
            <span className={`update-trust is-${status.signed ? "signed" : "unsigned"}`}>
              <ShieldCheck size={11} />
              {t(status.signed ? "updateSigned" : "updateUnsigned")}
            </span>
          ) : null}
          <code title={feedUrl}>{feedUrl || t("updateFeedNone")}</code>
        </div>
      </div>

      <div className="runtime-section-head">
        <div className="home-section-title">
          <Sparkle size={15} />
          {t("updates")}
        </div>
        <button
          className="settings-link runtime-refresh"
          onClick={() => void check()}
          disabled={checking || downloading}
        >
          <RefreshCw size={12} className={checking ? "spin" : ""} />
          {checking ? t("updateChecking") : t("updateCheckNow")}
        </button>
      </div>

      <div className="update-status-row">
        {available && status ? (
          <span className="update-arrow">
            <span className="update-new">
              <Sparkle size={11} />
              {status.availableVersion}
            </span>
            <small>{status.date}</small>
          </span>
        ) : null}
        <span className={`update-state is-${status?.state ?? "idle"}`}>
          {status
            ? t(
                status.state === "ready"
                  ? "updateReady"
                  : status.state === "available"
                    ? "updateAvailable"
                    : status.state === "current"
                      ? "updateCurrent"
                      : "updateUnavailable",
              )
            : t("updateIdle")}
        </span>
      </div>

      {status?.notes ? <p className="update-notes">{status.notes}</p> : null}

      {status?.fileName ? (
        <div className="update-artifact">
          <PackageOpen size={13} />
          <span className="update-artifact-name" title={status.fileName}>
            {status.fileName}
          </span>
          <span className="update-artifact-size">{formatBytes(status.size)}</span>
        </div>
      ) : null}

      {percent !== null ? (
        <div className="update-progress" role="progressbar" aria-valuenow={percent}>
          <div className="update-progress-fill" style={{ width: `${percent}%` }} />
          <span>
            {progress ? `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}` : ""}
          </span>
        </div>
      ) : null}

      {status?.sha256 ? (
        <code className="update-hash" title={status.sha256}>
          <ShieldCheck size={11} />
          SHA256 {verified ? status.sha256 : `${status.sha256.slice(0, 16)}…`}
        </code>
      ) : null}

      {status?.message && (failed || verified) ? (
        <div className={`runtime-error ${verified ? "is-ok" : ""}`}>
          <span>{verified ? t("updateVerified") : status.message}</span>
        </div>
      ) : null}

      {/* An install that could not run leaves the verified download in place, so it
          is reported beside the button instead of rewriting the status. */}
      {installError ? (
        <div className="runtime-error">
          <span>{installError}</span>
        </div>
      ) : null}

      <div className="update-actions">
        {available && !verified ? (
          <button className="runtime-install" onClick={() => void download()} disabled={downloading}>
            {downloading ? <LoaderCircle size={12} className="spin" /> : <Download size={12} />}
            {downloading ? t("updateDownloading") : t("updateDownload")}
          </button>
        ) : null}
        {verified ? (
          <button className="runtime-install" onClick={() => void install()} disabled={installing}>
            {installing ? <LoaderCircle size={12} className="spin" /> : <Check size={12} />}
            {installing ? t("updateInstalling") : t("updateInstall")}
          </button>
        ) : null}
        {verified ? (
          <button className="settings-link" onClick={() => void clear()}>
            <X size={12} />
            {t("updateDiscard")}
          </button>
        ) : null}
      </div>

      <label className="home-toggle-row home-toggle-divider">
        <strong>{t("updateAutoCheck")}</strong>
        <button
          className={`home-toggle ${autoCheck ? "is-on" : ""}`}
          onClick={() => onAutoCheckChange(!autoCheck)}
          aria-pressed={autoCheck}
        >
          <span />
        </button>
      </label>
      <p className="runtime-summary">{t("updateBody")}</p>
    </section>
  );
}
