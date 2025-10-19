import { Button } from "@/components/ui/button";
import { useState } from "react";

export default function () {
  const [is_restarting, set_is_restarting] = useState(false);

  const restart = async () => {
    set_is_restarting(true);
    await fetch("/api/sing/restart");
    set_is_restarting(false);
  };

  return (
    <div className="mt-8 m-h-32 flex justify-center items-center">
      <Button size={"lg"} disabled={is_restarting} onClick={restart}>
        重新启动
      </Button>
    </div>
  );
}
