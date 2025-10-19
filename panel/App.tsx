import Config from "./Config";
import Control from "./Control";

export default function () {
  return (
    <div className="p-32">
      <h1 className="text-3xl text-center">Miao.</h1>
      <Control />
      <Config />
    </div>
  );
}
