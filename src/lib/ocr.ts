type OcrProgress = (progress: number, status: string) => void;

let workerPromise: Promise<any> | null = null;

async function getWorker(onProgress?: OcrProgress) {
  if (workerPromise) return workerPromise;
  workerPromise = (async () => {
    const { createWorker, OEM } = await import("tesseract.js");
    return createWorker(["eng", "chi_sim"], OEM.LSTM_ONLY, {
      workerPath: "/tesseract/worker.min.js",
      corePath: "/tesseract/core/",
      langPath: "/tesseract/lang/",
      workerBlobURL: true,
      logger: (message: { progress?: number; status?: string }) => {
        onProgress?.(message.progress ?? 0, message.status ?? "");
      },
    });
  })();
  return workerPromise;
}

export async function recognizeImage(
  dataUrl: string,
  onProgress?: OcrProgress,
): Promise<{ text: string; confidence: number }> {
  const worker = await getWorker(onProgress);
  const result = await worker.recognize(dataUrl);
  return {
    text: String(result.data.text || "").trim(),
    confidence: Number(result.data.confidence || 0),
  };
}

export async function disposeOcrWorker() {
  if (!workerPromise) return;
  const worker = await workerPromise.catch(() => null);
  workerPromise = null;
  await worker?.terminate?.();
}
