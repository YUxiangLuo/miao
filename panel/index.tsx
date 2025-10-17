import { createRoot } from "react-dom/client"
import { App } from "./App"

document.addEventListener("DOMContentLoaded", () => {
  const root = createRoot(document.getElementById("root")!);
  root.render(<App />);
});