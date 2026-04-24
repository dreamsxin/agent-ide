import { useCallback, useRef, useEffect, useState } from "react";

interface ResizeHandleProps {
  direction: "horizontal" | "vertical";
  onResize: (delta: number) => void;
  className?: string;
}

export default function ResizeHandle({ direction, onResize, className = "" }: ResizeHandleProps) {
  const handleRef = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(false);
  const startPos = useRef<number>(0);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      setActive(true);
      startPos.current = direction === "horizontal" ? e.clientX : e.clientY;

      const onMouseMove = (ev: MouseEvent) => {
        const current = direction === "horizontal" ? ev.clientX : ev.clientY;
        onResize(current - startPos.current);
        startPos.current = current;
      };

      const onMouseUp = () => {
        setActive(false);
        document.removeEventListener("mousemove", onMouseMove);
        document.removeEventListener("mouseup", onMouseUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      };

      document.body.style.cursor = direction === "horizontal" ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
      document.addEventListener("mousemove", onMouseMove);
      document.addEventListener("mouseup", onMouseUp);
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
      onMouseDown={onMouseDown}
    />
  );
}
