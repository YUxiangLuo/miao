import type { Config, Node } from "./types";
import { Button } from "@/components/ui/button";
import { useEffect, useState, useMemo } from "react";
import { NodeList } from "./Node";

export default function () {
  const [config, set_config] = useState<Config>();
  const [is_generating, set_is_generating] = useState(false);
  const [error, set_error] = useState<boolean | null>(null);
  const [success, set_success] = useState<boolean | null>(null);

  const nodes = useMemo(() => {
    if (!config) return [];
    return JSON.parse(config.config_content).outbounds as Node[];
  }, [config]);

  const fetch_config_status = async () => {
    const res = await fetch("/api/config");
    const res_json = await res.json();
    set_config(res_json);
  };

  const gen_config = async () => {
    set_success(null);
    set_error(null);
    set_is_generating(true);
    const res = await fetch("/api/config/generate");
    set_is_generating(false);
    if (!res.ok) {
      set_error(true);
      return;
    } else {
      set_success(true);
    }
    await fetch_config_status();
  };

  useEffect(() => {
    fetch_config_status();
  }, []);

  return (
    <div className="flex flex-col gap-4 mt-8 border-1 rounded-2xl p-8">
      <div className="flex items-center gap-8">
        <Button size={"lg"} onClick={fetch_config_status}>
          Check
        </Button>
        <Button size={"lg"} onClick={gen_config} disabled={is_generating}>
          重新生成配置文件
        </Button>
        {success && <span className="text-green-500">成功</span>}
        {error && <span className="text-red-500">失败</span>}
      </div>
      <div className="flex flex-col gap-4 bg-background text-foreground">
        {config && (
          <div className="flex flex-col gap-4">
            <span>
              更新时间: {new Date(config.config_stat.mtimeMs!).toLocaleString()}
            </span>
            <span>文件大小: {config.config_stat.size!}字节</span>
          </div>
        )}
      </div>
      <NodeList nodes={nodes} />
    </div>
  );
}
