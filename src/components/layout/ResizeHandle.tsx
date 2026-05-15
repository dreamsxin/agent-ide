import { useCallback, useRef, useEffect, useState } from "react";

interface ResizeHandleProps {
  direction: "horizontal" | "vertical";
  onResize: (delta: number, phase?: "start" | "move" | "end") => void;
  className?: string;
}

export default function ResizeHandle({ direction, onResize, className = "" }: ResizeHandleProps) {
  const handleRef = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(false);
  const startPos = useRef<number>(0);
  const activePointerId = useRef<number | null>(null);

  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      e.stopPropagation();
      activePointerId.current = e.pointerId;
      e.currentTarget.setPointerCapture(e.pointerId);
      setActive(true);
      startPos.current = direction === "horizontal" ? e.clientX : e.clientY;
      onResize(0, "start");

      const onPointerMove = (ev: PointerEvent) => {
        if (activePointerId.current !== ev.pointerId) return;
        const current = direction === "horizontal" ? ev.clientX : ev.clientY;
        onResize(current - startPos.current, "move");
      };

      const onPointerUp = (ev: PointerEvent) => {
        if (activePointerId.current !== ev.pointerId) return;
        activePointerId.current = null;
        setActive(false);
        onResize(0, "end");
        document.removeEventListener("pointermove", onPointerMove);
        document.removeEventListener("pointerup", onPointerUp);
        document.removeEventListener("pointercancel", onPointerUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      };

      document.body.style.cursor = direction === "horizontal" ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
      document.addEventListener("pointermove", onPointerMove);
      document.addEventListener("pointerup", onPointerUp);
      document.addEventListener("pointercancel", onPointerUp);
    },
    [direction, onResize]
  );

  useEffect(() => {
    return () => {
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
  }, []);

  const isH = direction === "horizontal";

  return (
    <div
      ref={handleRef}
      className={`${isH ? "resize-handle" : "resize-handle-h"} ${active ? "active" : ""} ${className}`}
      onPointerDown={onPointerDown}
    />
  );
}
