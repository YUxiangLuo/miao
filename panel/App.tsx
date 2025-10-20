import Config from "./Config";
import Control from "./Control";

export default function () {
  return (
    <div className="h-full w-full flex flex-col p-20">
      <Control />
      <Config />
    </div>
  );
}
