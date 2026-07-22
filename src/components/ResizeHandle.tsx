import { useEffect, useRef, useState } from "react";

import { tr } from "../i18n";

interface ResizeHandleProps {
  variable: string;
  storageKey: string;
  min: number;
  max: number;
  defaultValue: number;
  edge: "left" | "right";
  label: string;
}

export function ResizeHandle({
  variable,
  storageKey,
  min,
  max,
  defaultValue,
  edge,
  label,
}: ResizeHandleProps) {
  const value = useRef(defaultValue);
  const [currentValue, setCurrentValue] = useState(defaultValue);

  const apply = (next: number, persist = true) => {
    const bounded = Math.round(Math.max(min, Math.min(max, next)));
    value.current = bounded;
    setCurrentValue(bounded);
    document.documentElement.style.setProperty(variable, `${bounded}px`);
    if (persist) localStorage.setItem(storageKey, String(bounded));
  };

  useEffect(() => {
    const stored = Number(localStorage.getItem(storageKey));
    apply(Number.isFinite(stored) && stored > 0 ? stored : defaultValue, false);
    // Each handle owns one stable CSS variable.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [storageKey]);

  return (
    <div
      className={`panel-resize-handle is-${edge}`}
      role="separator"
      aria-label={tr(label)}
      aria-orientation="vertical"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={currentValue}
      tabIndex={0}
      onDoubleClick={() => apply(defaultValue)}
      onKeyDown={(event) => {
        if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
        event.preventDefault();
        const direction = event.key === "ArrowRight" ? 1 : -1;
        const edgeDirection = edge === "right" ? direction : -direction;
        apply(value.current + edgeDirection * (event.shiftKey ? 24 : 8));
      }}
      onPointerDown={(event) => {
        if (event.pointerType === "touch") return;
        event.preventDefault();
        const startX = event.clientX;
        const startValue = value.current;
        const move = (moveEvent: PointerEvent) => {
          const delta = moveEvent.clientX - startX;
          apply(startValue + (edge === "right" ? delta : -delta), false);
        };
        const finish = () => {
          localStorage.setItem(storageKey, String(value.current));
          document.body.classList.remove("is-resizing-panel");
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", finish);
          window.removeEventListener("pointercancel", finish);
        };
        document.body.classList.add("is-resizing-panel");
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", finish, { once: true });
        window.addEventListener("pointercancel", finish, { once: true });
      }}
    />
  );
}
