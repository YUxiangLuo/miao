import { Button } from "@/components/ui/button";
import { useState } from "react";

export default function () {
  const [is_restarting, set_is_restarting] = useState(false);
  const [restart_error, set_restart_error] = useState<boolean | null>(null);
  const [restart_success, set_restart_success] = useState<boolean | null>(null);

  const [is_stopping, set_is_stopping] = useState(false);
  const [stop_error, set_stop_error] = useState<boolean | null>(null);
  const [stop_success, set_stop_success] = useState<boolean | null>(null);

  const restart = async () => {
    set_restart_error(null);
    set_restart_success(null);
    set_is_restarting(true);
    const res = await fetch("/api/sing/restart");
    set_is_restarting(false);
    if (!res.ok) {
      set_restart_error(true);
    } else {
      set_restart_success(true);
    }
  };

  const stop = async () => {
    set_stop_error(null);
    set_stop_success(null);
    set_is_stopping(true);
    const res = await fetch("/api/sing/stop");
    set_is_stopping(false);
    if (!res.ok) {
      set_stop_error(true);
    } else {
      set_stop_success(true);
    }
  };

  return (
    <div className="flex items-center gap-8">
      <div>
        <Button size={"lg"} disabled={is_restarting} onClick={restart}>
          启动
        </Button>
        {restart_error && <p className="text-red-500">重启失败</p>}
        {restart_success && <p className="text-green-500">重启成功</p>}
      </div>
      <div>
        <Button size={"lg"} disabled={is_stopping} onClick={stop}>
          停止
        </Button>
        {stop_error && <p className="text-red-500">停止失败</p>}
        {stop_success && <p className="text-green-500">停止成功</p>}
      </div>
    </div>
  );
}
