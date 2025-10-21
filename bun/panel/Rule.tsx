import type { FileStat } from "./types";
import { Button } from "@/components/ui/button";
import { useState } from "react";
export default function () {
  const [is_loading, setIsLoading] = useState(false);
  const [rule_status, set_rule_status] = useState<FileStat | null>(null);

  const generate = async () => {
    setIsLoading(true);
    try {
      const response = await fetch("/api/rule/generate");
      const data = (await response.json()) as FileStat;
      set_rule_status(data);
    } catch (error) {
      console.error(error);
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="flex flex-col items-start gap-4 p-8 border-1 rounded-2xl">
      <Button onClick={generate} disabled={is_loading}>
        Generate Rule
      </Button>
      {rule_status && (
        <div className="flex flex-col gap-4 bg-background text-foreground">
          <p>修改时间: {new Date(rule_status.mtimeMs).toLocaleString()}</p>
          <p>文件大小: {rule_status.size} 字节</p>
        </div>
      )}
    </div>
  );
}
