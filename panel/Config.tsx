import type { Config } from "./types";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useEffect, useState } from "react";

export default function () {
  const [config, set_config] = useState<Config>();
  const [is_loading, set_is_loading] = useState(false);

  const fetch_config_status = async () => {
    set_is_loading(true);
    const res = await fetch("/api/config");
    const res_json = await res.json();
    set_is_loading(false);
    set_config(res_json);
  };

  const gen_config = async () => {
    set_is_loading(true);
    await fetch("/api/config/generate");
    set_is_loading(false);
  };

  useEffect(() => {
    fetch_config_status();
  }, []);

  return (
    <div className="flex flex-col gap-4 mt-8">
      <div className="flex gap-8">
        <Button size={"lg"} onClick={fetch_config_status} disabled={is_loading}>
          Check
        </Button>
        <Button size={"lg"} onClick={gen_config} disabled={is_loading}>
          Generate
        </Button>
      </div>
      <div className="flex flex-col gap-4">
        {config && (
          <div className="flex flex-col gap-4">
            <span>
              更新时间: {new Date(config.config_stat.mtimeMs!).toLocaleString()}
            </span>
            <span>文件大小: {config.config_stat.size!}字节</span>
          </div>
        )}
        {config && (
          <Textarea readOnly value={JSON.stringify(config.config_content)} />
        )}
      </div>
    </div>
  );
}
