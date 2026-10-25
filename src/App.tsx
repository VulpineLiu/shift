import { CaptureOverlay } from "./components/CaptureOverlay";
import { Editor } from "./components/Editor";
import { MainWindow } from "./components/MainWindow";
import { PinnedImage } from "./components/PinnedImage";
import { RadialMenu } from "./components/RadialMenu";
import { RecorderHud } from "./components/RecorderHud";
import { ResultCard } from "./components/ResultCard";
import { ShareNotice } from "./components/ShareNotice";
import type { View } from "./types";

function resolveView(): View {
  const value = new URLSearchParams(window.location.search).get("view");
  if (
    value === "radial" ||
    value === "capture" ||
    value === "recorder" ||
    value === "editor" ||
    value === "pin" ||
    value === "result-card" ||
    value === "share-notice"
  ) {
    return value;
  }
  return "main";
}

export default function App() {
  const view = resolveView();
  document.body.dataset.view = view;

  switch (view) {
    case "radial":
      return <RadialMenu />;
    case "capture":
      return <CaptureOverlay />;
    case "recorder":
      return <RecorderHud />;
    case "editor":
      return <Editor />;
    case "pin":
      return <PinnedImage />;
    case "result-card":
      return <ResultCard />;
    case "share-notice":
      return <ShareNotice />;
    default:
      return <MainWindow />;
  }
}
