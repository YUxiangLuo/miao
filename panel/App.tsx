import Config from "./Config";
import Control from "./Control";
import Rule from "./Rule";

export default function () {
  return (
    <div className="h-full w-full flex flex-col gap-4 p-20 dark bg-background">
      <Control />
      <Rule />
      <Config />
    </div>
  );
}
