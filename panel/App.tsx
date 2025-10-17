import { useState, useCallback, useRef } from "react";
import Config from "./Config";

export function App() {
  const [genRes, setGenRes] = useState<string>("");
  const [restartRes, setRestartRes] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const restart = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/sing/restart");
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const text = await res.text();
      setRestartRes(text);
    } catch (e: any) {
      if (e?.name === "AbortError") return; // 被中断就忽略
      setError(e?.message ?? "请求失败");
    } finally {
      setLoading(false);
    }
  }, []);

  const gen_config = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/config/generate");
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const text = await res.text();
      setGenRes(text);
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

      <div>
        <button onClick={gen_config} disabled={loading}>
          {loading ? "请求中..." : "生成配置文件"}
        </button>
      </div>

      <div>
        <button onClick={restart} disabled={loading}>
          {loading ? "请求中..." : "重启sing-box"}
        </button>
      </div>

      {error && <div style={{ color: "crimson" }}>错误：{error}</div>}
      {!error && <div>{genRes}</div>}
      {!error && <div>{restartRes}</div>}

      <Config key={genRes} />
    </>
  );
}
