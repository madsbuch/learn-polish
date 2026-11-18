import { useEffect, useRef } from "react";
import jsQR from "jsqr";

/**
 * The computer's camera, read frame by frame until a QR code starting with
 * `prefix` shows up (the pairing code another device shows). The phone has
 * the barcode scanner plugin; a computer reads its webcam in the page.
 */
export function WebcamScanner({
  prefix,
  onCode,
  onError,
}: {
  prefix: string;
  onCode: (code: string) => void;
  onError: (message: string) => void;
}) {
  const video = useRef<HTMLVideoElement | null>(null);
  useEffect(() => {
    let stream: MediaStream | null = null;
    let frame = 0;
    let done = false;
    const canvas = document.createElement("canvas");
    const context = canvas.getContext("2d", { willReadFrequently: true });
    const tick = () => {
      if (done) return;
      const element = video.current;
      if (
        element &&
        context &&
        element.readyState >= 2 &&
        element.videoWidth > 0
      ) {
        canvas.width = element.videoWidth;
        canvas.height = element.videoHeight;
        context.drawImage(element, 0, 0);
        const image = context.getImageData(0, 0, canvas.width, canvas.height);
        const hit = jsQR(image.data, image.width, image.height);
        if (hit?.data.startsWith(prefix)) {
          done = true;
          onCode(hit.data);
          return;
        }
      }
      frame = requestAnimationFrame(tick);
    };
    if (!navigator.mediaDevices?.getUserMedia) {
      onError("There's no camera here. Paste the code instead.");
      return;
    }
    navigator.mediaDevices
      .getUserMedia({ video: { facingMode: "environment" } })
      .then((media) => {
        if (done) {
          media.getTracks().forEach((track) => track.stop());
          return;
        }
        stream = media;
        if (video.current) {
          video.current.srcObject = media;
          void video.current.play();
        }
        frame = requestAnimationFrame(tick);
      })
      .catch((cause: unknown) => {
        const reason = cause instanceof Error ? cause.message : String(cause);
        onError(
          `The camera isn't available (${reason}). Allow Polski to use it, or paste the code instead.`,
        );
      });
    return () => {
      done = true;
      cancelAnimationFrame(frame);
      stream?.getTracks().forEach((track) => track.stop());
    };
  }, [prefix, onCode, onError]);
  return <video ref={video} className="webcam-scanner" muted playsInline />;
}
