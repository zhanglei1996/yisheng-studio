import { memo, useEffect, useRef } from "react";

export const WaveformCanvas = memo(function WaveformCanvas({ color = "#8a9099", seed = 1 }: { color?: string; seed?: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const lastSize = useRef("");
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const resize = () => {
      const ratio = window.devicePixelRatio || 1;
      const rect = canvas.getBoundingClientRect();
      const size = `${Math.round(rect.width)}x${Math.round(rect.height)}@${ratio}`;
      if (size === lastSize.current) return;
      lastSize.current = size;
      canvas.width = Math.max(1, Math.round(rect.width * ratio));
      canvas.height = Math.max(1, Math.round(rect.height * ratio));
      const context = canvas.getContext("2d");
      if (!context) return;
      context.scale(ratio, ratio);
      context.clearRect(0, 0, rect.width, rect.height);
      context.fillStyle = color;
      let value = seed * 999 + 17;
      const random = () => { value = (value * 9301 + 49297) % 233280; return value / 233280; };
      const middle = rect.height / 2;
      for (let x = 0; x < rect.width; x += 2) {
        const envelope = 0.35 + Math.sin(x * 0.037 + seed) * 0.12 + Math.sin(x * 0.113) * 0.1;
        const height = Math.max(1, (random() * 0.55 + envelope) * middle);
        context.globalAlpha = 0.6 + random() * 0.35;
        context.fillRect(x, middle - height, 1, height * 2);
      }
      context.globalAlpha = 1;
    };
    resize();
    let frame = 0;
    const observer = new ResizeObserver(() => {
      if (frame) return;
      frame = window.requestAnimationFrame(() => { frame = 0; resize(); });
    });
    observer.observe(canvas);
    return () => { if (frame) window.cancelAnimationFrame(frame); observer.disconnect(); };
  }, [color, seed]);
  return <canvas ref={ref} className="waveform-canvas" aria-hidden="true" />;
});
