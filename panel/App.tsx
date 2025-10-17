import { useState, useCallback } from "react";
import { Button } from "antd";
import Config from "./Config";
import Checks from "./Checks";

export function App() {
  const [configStatus, setConfigStatus] = useState<any>({});
  const [configStatusLoading, setConfigStatusLoading] = useState<boolean>(false);

  const [restartStatus, setRestartStatus] = useState<string>("");
  const [restarting, setRestarting] = useState<boolean>(false);

  const restart = useCallback(async () => {
    setRestarting(true);
    const res = await fetch("/api/sing/restart");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const text = await res.text();
    setRestartStatus(text);
    setRestarting(false);
  }, []);

  const gen_config = useCallback(async () => {
    setConfigStatusLoading(true);
    const res = await fetch("/api/config/generate");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    setConfigStatus(await res.json());
    setConfigStatusLoading(false);
  }, []);

  return (
    <>
      <div style={{ height: "300px", display: "flex", justifyContent: "center", alignItems: "center", gap: "50px" }}>
        <div>
          <Button type="primary" size="large" onClick={gen_config} disabled={configStatusLoading}>
            生成配置文件
          </Button>
        </div>

        <div>
          <Button type="dashed" size="large" onClick={restart} disabled={restarting}>
            重启sing-box
          </Button>
        </div>
      </div>




      <div style={{ display: "flex", justifyContent: "center" }}>
        <div style={{ width: "800px" }}>
          {configStatus.mtimeMs && <div>最新文件生成时间： {new Date(configStatus.mtimeMs).toLocaleString()}</div>}
          {restartStatus && <div>sing-box进程ID: {restartStatus}</div>}
          <Checks />
          <Config />
        </div>
      </div>
    </>
  );
}
