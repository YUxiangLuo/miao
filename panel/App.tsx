import { useState, useCallback, useRef } from "react";
import Config from "./Config";

export function App() {
  const [goRes, setGoRes] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 保存当前请求的 AbortController，防止连点产生并发
  const controllerRef = useRef<AbortController | null>(null);

  const go = useCallback(async () => {
    // 若已有请求在进行，先中断它（可选）
    controllerRef.current?.abort();
    const controller = new AbortController();
    controllerRef.current = controller;

    setLoading(true);
    setError(null);

    try {
      const res = await fetch("/api/go", { signal: controller.signal });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const text = await res.text();
      setGoRes(text);
    } catch (e: any) {
      if (e?.name === "AbortError") return; // 被中断就忽略
      setError(e?.message ?? "请求失败");
    } finally {
      setLoading(false);
    }
  }, []);

  return (
    <>
      <div>hello world from bun!</div>

      <button onClick={go} disabled={loading}>
        {loading ? "请求中..." : "Let's go LGD"}
      </button>

      {error && <div style={{ color: "crimson" }}>错误：{error}</div>}
      {!error && <div>{goRes}</div>}

      <Config />
    </>
  );
}
